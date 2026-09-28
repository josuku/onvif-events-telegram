use anyhow::{Context, Result, bail};
use app_core::domain::camera::Recording;
use app_core::traits::api_camera_client::ApiCameraClient;
use async_trait::async_trait;
use chrono::{DateTime, Duration, FixedOffset, Local, Utc};
use futures_util::StreamExt;
use reqwest::Client;
use serde::Deserialize;
use std::path::Path;
use tokio::fs::File;
use tokio::io::AsyncWriteExt;

use crate::{MAX_DOWNLOAD_SECONDS, ffmpeg_trim_and_convert, get_clip_interval};

const REQUEST_TIMEOUT_SECS: u64 = 15;
const DOWNLOAD_TIMEOUT_SECS: u64 = 300;
const DEFAULT_TRACK_ID: u32 = 101;

pub struct HikvisionIsapiApiCameraClient {
    host: String,
    user: String,
    password: String,
    http: Client,
    download_http: Client,
    local_time_labeled_as_utc: bool, // some cameras return recordings in their local time with Z
}

fn normalize_host(host: &str) -> String {
    host.trim_start_matches("http://")
        .trim_start_matches("https://")
        .to_string()
}

impl HikvisionIsapiApiCameraClient {
    pub fn new(
        host: String,
        user: String,
        password: String,
        local_time_labeled_as_utc: bool,
    ) -> Self {
        let normalized_host = normalize_host(&host);
        let http = Client::builder()
            .timeout(std::time::Duration::from_secs(REQUEST_TIMEOUT_SECS))
            .build()
            .expect("Cannot build HTTP client");
        let download_http = Client::builder()
            .timeout(std::time::Duration::from_secs(DOWNLOAD_TIMEOUT_SECS))
            .build()
            .expect("Cannot build download HTTP client");

        tracing::info!(
            "created new HikvisionIsapiCameraClient in host: {} and username: {} (fake utc: {})",
            normalized_host,
            user,
            local_time_labeled_as_utc
        );

        Self {
            host: normalized_host,
            user,
            password,
            http,
            download_http,
            local_time_labeled_as_utc,
        }
    }

    fn base_url(&self) -> String {
        format!("http://{}", self.host)
    }
}

#[async_trait]
impl ApiCameraClient for HikvisionIsapiApiCameraClient {
    async fn init(&mut self, host: Option<String>, user: Option<String>, password: Option<String>) {
        if let Some(h) = host {
            self.host = normalize_host(&h);
        }
        if let Some(u) = user {
            self.user = u;
        }
        if let Some(p) = password {
            self.password = p;
        }
    }

    async fn get_recordings(
        &self,
        time: DateTime<Utc>,
        clip_time: Duration,
    ) -> Result<Vec<Recording>> {
        let (start_time, end_time) = get_clip_interval(time, clip_time);

        let local_offset = self
            .local_time_labeled_as_utc
            .then(|| *time.with_timezone(&Local).offset());

        let (query_start, query_end) = match local_offset {
            Some(offset) => (
                fake_utc_with_offset(start_time, offset),
                fake_utc_with_offset(end_time, offset),
            ),
            None => (start_time, end_time),
        };

        let body = build_search_body(DEFAULT_TRACK_ID, query_start, query_end);

        let resp = self
            .http
            .post(format!("{}/ISAPI/ContentMgmt/search", self.base_url()))
            .basic_auth(&self.user, Some(&self.password))
            .header("Content-Type", "application/xml")
            .body(body)
            .send()
            .await
            .context("Network error calling ISAPI search")?;

        if !resp.status().is_success() {
            bail!("ISAPI search failed (HTTP {})", resp.status());
        }

        let xml = resp
            .text()
            .await
            .context("Cannot read ISAPI search response body")?;

        let parsed: CmSearchResult =
            quick_xml::de::from_str(&xml).context("Cannot parse ISAPI search XML")?;

        let recordings = parsed
            .match_list
            .items
            .into_iter()
            .map(|item| {
                let size_mb = extract_query_param(&item.descriptor.playback_uri, "size")
                    .and_then(|s| s.parse::<f64>().ok())
                    .map(|bytes| bytes / 1024.0 / 1024.0)
                    .unwrap_or(0.0);

                let (begin, end) = match local_offset {
                    Some(offset) => (
                        correct_fake_utc(&item.time_span.start_time, offset)
                            .unwrap_or_else(|_| item.time_span.start_time.clone()),
                        correct_fake_utc(&item.time_span.end_time, offset)
                            .unwrap_or_else(|_| item.time_span.end_time.clone()),
                    ),
                    None => (
                        item.time_span.start_time.clone(),
                        item.time_span.end_time.clone(),
                    ),
                };

                Recording {
                    name: item.descriptor.playback_uri,
                    size_mb,
                    begin,
                    end,
                }
            })
            .collect();

        Ok(recordings)
    }

    async fn download_recording(
        &self,
        recordings: &[Recording],
        event_time: DateTime<Utc>,
        clip_time: Duration,
        target_name: String,
    ) -> Result<String> {
        let recording = match get_chosen_isapi_recording(recordings, event_time) {
            Some(recording) => recording,
            None => bail!("No recordings found"),
        };

        let (offset_secs, duration_secs) =
            compute_isapi_clip_window(&recording, event_time, clip_time)?;

        let download_body = build_download_body(&recording.name);

        let raw_name = format!("./{target_name}.raw.mp4");

        let response = self
            .download_http
            .get(format!("{}/ISAPI/ContentMgmt/download", self.base_url()))
            .basic_auth(&self.user, Some(&self.password))
            .header("Content-Type", "application/xml")
            .body(download_body)
            .send()
            .await
            .context("Error starting ISAPI recording download")?;

        if !response.status().is_success() {
            bail!("ISAPI download failed (HTTP {})", response.status());
        }

        tracing::info!("download_recording -> downloading raw block from ISAPI");

        {
            let mut file = File::create(&raw_name)
                .await
                .with_context(|| format!("Cannot create {raw_name}"))?;
            let mut stream = response.bytes_stream();
            while let Some(chunk) = stream.next().await {
                let chunk = chunk.context("Error reading ISAPI download stream")?;
                file.write_all(&chunk).await?;
            }
            file.flush().await?;
        }

        tracing::info!("download_recording -> raw block {raw_name:?} downloaded from camera");

        let output_file = format!("./{target_name}.mp4");

        if let Err(err) = ffmpeg_trim_and_convert(
            Path::new(&raw_name),
            Path::new(&output_file),
            offset_secs,
            duration_secs,
        )
        .await
        {
            tracing::error!("Error with ffmpeg or not installed: {:?}", err);
            Ok(raw_name)
        } else {
            tracing::info!(
                "file {output_file:?} succesfully codified with ffmpeg. deleting raw block:{raw_name:?}"
            );
            tokio::fs::remove_file(&raw_name).await.ok();
            Ok(output_file)
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename = "CMSearchResult")]
struct CmSearchResult {
    #[serde(rename = "matchList", default)]
    match_list: MatchList,
}

#[derive(Debug, Deserialize, Default)]
struct MatchList {
    #[serde(rename = "searchMatchItem", default)]
    items: Vec<SearchMatchItem>,
}

#[derive(Debug, Deserialize)]
struct SearchMatchItem {
    #[serde(rename = "timeSpan")]
    time_span: TimeSpan,
    #[serde(rename = "mediaSegmentDescriptor")]
    descriptor: MediaSegmentDescriptor,
}

#[derive(Debug, Deserialize)]
struct TimeSpan {
    #[serde(rename = "startTime")]
    start_time: String,
    #[serde(rename = "endTime")]
    end_time: String,
}

#[derive(Debug, Deserialize)]
struct MediaSegmentDescriptor {
    #[serde(rename = "playbackURI")]
    playback_uri: String,
}

fn build_search_body(track_id: u32, start: DateTime<Utc>, end: DateTime<Utc>) -> String {
    let search_id = uuid::Uuid::new_v4();
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<CMSearchDescription>
  <searchID>{search_id}</searchID>
  <trackList>
    <trackID>{track_id}</trackID>
  </trackList>
  <timeSpanList>
    <timeSpan>
      <startTime>{}</startTime>
      <endTime>{}</endTime>
    </timeSpan>
  </timeSpanList>
  <maxResults>40</maxResults>
  <searchResultPostion>0</searchResultPostion>
  <metadataList>
    <metadataDescriptor>//metadata.psia.org/VideoMotion</metadataDescriptor>
  </metadataList>
</CMSearchDescription>"#,
        start.to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        end.to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
    )
}

fn build_download_body(playback_uri: &str) -> String {
    let escaped = quick_xml::escape::escape(playback_uri);
    format!(
        r#"<?xml version="1.0"?><downloadRequest><playbackURI>{escaped}</playbackURI></downloadRequest>"#
    )
}

fn extract_query_param<'a>(uri: &'a str, key: &str) -> Option<&'a str> {
    let query = uri.split('?').nth(1)?;
    query.split('&').find_map(|pair| {
        let mut parts = pair.splitn(2, '=');
        let k = parts.next()?;
        let v = parts.next()?;
        (k == key).then_some(v)
    })
}

fn parse_isapi_time(time_string: &str) -> Result<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(time_string)
        .map(|dt| dt.with_timezone(&Utc))
        .with_context(|| format!("cannot parse ISAPI timestamp: {time_string}"))
}

fn fake_utc_with_offset(real: DateTime<Utc>, local_offset: FixedOffset) -> DateTime<Utc> {
    real + Duration::seconds(local_offset.local_minus_utc() as i64)
}

fn correct_fake_utc(wire_time: &str, local_offset: FixedOffset) -> Result<String> {
    let fake = parse_isapi_time(wire_time)?;
    let real = fake - Duration::seconds(local_offset.local_minus_utc() as i64);
    Ok(real.to_rfc3339_opts(chrono::SecondsFormat::Secs, true))
}

pub fn get_chosen_isapi_recording(
    recordings: &[Recording],
    event_time: DateTime<Utc>,
) -> Option<Recording> {
    let parsed: Vec<(&Recording, DateTime<Utc>, DateTime<Utc>)> = recordings
        .iter()
        .filter_map(|r| {
            let begin = parse_isapi_time(&r.begin).ok()?;
            let end = parse_isapi_time(&r.end).ok()?;
            Some((r, begin, end))
        })
        .collect();

    parsed
        .iter()
        .find(|(_, begin, end)| *begin <= event_time && event_time <= *end)
        .or_else(|| {
            parsed
                .iter()
                .min_by_key(|(_, start, _)| (*start - event_time).num_seconds().abs())
        })
        .map(|(r, _, _)| (*r).clone())
}

fn compute_isapi_clip_window(
    recording: &Recording,
    time: DateTime<Utc>,
    clip_time: Duration,
) -> Result<(f64, f64)> {
    let (clip_start_time, clip_end_time) = get_clip_interval(time, clip_time);
    if (clip_end_time - clip_start_time).num_seconds() > MAX_DOWNLOAD_SECONDS {
        anyhow::bail!("Downloads of more than {MAX_DOWNLOAD_SECONDS} are forbidden in this way");
    }

    let recording_start =
        parse_isapi_time(&recording.begin).context("cannot parse recording begin time")?;
    let recording_end =
        parse_isapi_time(&recording.end).context("cannot parse recording end time")?;

    let desired_start = time - clip_time;
    let desired_end = time + clip_time;
    let desired_len = clip_time * 2;
    let file_len = recording_end - recording_start;

    let (window_start, window_end) = if file_len <= desired_len {
        (recording_start, recording_end)
    } else {
        let mut start = desired_start;
        let mut end = desired_end;
        if start < recording_start {
            let shift = recording_start - start;
            start = recording_start;
            end += shift;
        }
        if end > recording_end {
            let shift = end - recording_end;
            end = recording_end;
            start = (start - shift).max(recording_start);
        }
        (start, end)
    };

    if window_start >= window_end {
        anyhow::bail!(
            "invalid clip window: start={window_start} >= end={window_end} \
            (event time={time}, recording={recording_start}..{recording_end})"
        );
    }

    let offset_secs = (window_start - recording_start).num_milliseconds() as f64 / 1000.0;
    let duration_secs = (window_end - window_start).num_milliseconds() as f64 / 1000.0;

    tracing::info!(
        "download_recording -> time:{time:?} recording:{recording_start:?}-{recording_end:?} \
        window:{window_start:?}-{window_end:?} offset:{offset_secs:?} duration:{duration_secs:?}"
    );

    Ok((offset_secs, duration_secs))
}
