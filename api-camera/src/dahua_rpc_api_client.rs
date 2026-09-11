use anyhow::{Context, Result, anyhow, bail};
use app_core::domain::camera::Recording;
use app_core::traits::api_camera_client::ApiCameraClient;
use async_trait::async_trait;
use chrono::{DateTime, Duration, FixedOffset, Local, NaiveDateTime, TimeZone, Utc};
use reqwest::Client;
use serde_json::{Value, json};
use std::path::Path;
use tokio::fs::File;
use tokio::io::AsyncWriteExt;

use crate::{MAX_DOWNLOAD_SECONDS, get_clip_interval, run_ffmpeg};
const REQUEST_TIMEOUT_SECS: u64 = 15;
const DOWNLOAD_TIMEOUT_SECS: u64 = 300;
const DEFAULT_DAHUA_SD_DIR: &str = "/mnt/sd";

// Implementation for Dahua RPC2 (JSON-RPC over HTTP). Tested with IPC-HFW2320R-ZS (firmware 2016)

pub struct DahuaRpcApiCameraClient {
    host: String,
    user: String,
    password: String,
}

fn normalize_host_for_dahua(host: &str) -> String {
    host.trim_start_matches("http://")
        .trim_start_matches("https://")
        .to_string()
}

impl DahuaRpcApiCameraClient {
    pub fn new(host: String, user: String, password: String) -> Self {
        let normalized_host = normalize_host_for_dahua(&host);
        tracing::info!(
            "created new DahuaRpcApiCameraClient in host: {} and username: {}",
            normalized_host,
            user
        );

        Self {
            host: normalized_host,
            user,
            password,
        }
    }

    async fn connect_and_login(&self) -> anyhow::Result<DahuaClient> {
        let mut client = DahuaClient::new(&self.host, &self.user, &self.password)?;
        client.login().await?;
        Ok(client)
    }
}

#[async_trait]
impl ApiCameraClient for DahuaRpcApiCameraClient {
    async fn init(&mut self, host: Option<String>, user: Option<String>, password: Option<String>) {
        self.host = host.unwrap_or_else(|| self.host.clone());
        self.user = user.unwrap_or_else(|| self.user.clone());
        self.password = password.unwrap_or_else(|| self.password.clone());
    }

    async fn get_recordings(
        &self,
        time: DateTime<Utc>,
        clip_time: chrono::Duration,
    ) -> anyhow::Result<Vec<Recording>> {
        let mut recordings = Vec::new();

        let (start_time, end_time) = get_clip_interval(time, clip_time);

        let mut client = self.connect_and_login().await?;

        let result = client
            .find_recordings(0, start_time.into(), end_time.into())
            .await;

        client.logout().await.ok();

        match result {
            Ok(files) => files.iter().for_each(|file| {
                recordings.push(Recording {
                    name: file.file_path.clone(),
                    size_mb: file.length_bytes as f64 / 1024.0 / 1024.0,
                    begin: file.start_time.clone(),
                    end: file.end_time.clone(),
                })
            }),
            Err(e) => anyhow::bail!("Error listing files: {}", e),
        }

        Ok(recordings)
    }

    async fn download_recording(
        &self,
        recordings: &[Recording],
        event_time: DateTime<Utc>,
        clip_time: Duration,
        target_name: String,
    ) -> anyhow::Result<String> {
        // TODO remove get_chosen_recording and try to download only desired time for files
        let recording = match get_chosen_recording(recordings, event_time) {
            Some(recording) => recording,
            None => bail!("No recordings found"),
        };

        let (offset_secs, duration_secs) = compute_clip_window(&recording, event_time, clip_time)?;

        let mut client = self.connect_and_login().await?;
        tracing::info!("download_recording -> connected to Dahua camera");

        let target_name_with_extension = format!("./{target_name}.dav");

        let result = client
            .download_recording(&recording.name, Path::new(&target_name_with_extension))
            .await;
        tracing::info!(
            "download_recording -> file {target_name_with_extension:?} downloaded from camera"
        );

        client.logout().await.ok();

        if let Err(err) = result {
            anyhow::bail!("Download failed: {:?}", err)
        }

        let output_file = format!("./{target_name}.mp4");
        if let Err(err) = ffmpeg_trim_and_convert(
            Path::new(&target_name_with_extension),
            Path::new(&output_file),
            offset_secs,
            duration_secs,
        )
        .await
        {
            tracing::error!("Error with ffmpeg or not installed: {:?}", err);
            Ok(target_name_with_extension)
        } else {
            tracing::info!(
                "file {output_file:?} succesfully codified with ffmpeg. deleting original file:{target_name_with_extension:?}"
            );
            tokio::fs::remove_file(&target_name_with_extension)
                .await
                .ok();
            Ok(output_file)
        }
    }
}

struct DahuaClient {
    http: Client,
    download_http: Client,
    base_url: String,
    username: String,
    password: String,
    session: Option<i64>,
    next_id: u32,
}

#[derive(Debug, Clone)]
pub struct DahuaRecording {
    pub file_path: String,
    pub start_time: String,
    pub end_time: String,
    pub length_bytes: i64,
    pub channel: i64,
    pub file_type: String,
}

impl DahuaClient {
    pub fn new(host: &str, username: &str, password: &str) -> Result<Self> {
        let http = Client::builder()
            .cookie_store(true)
            .timeout(std::time::Duration::from_secs(REQUEST_TIMEOUT_SECS))
            .build()
            .context("Cannot build HTTP client")?;
        let download_http = Client::builder()
            .cookie_store(true)
            .timeout(std::time::Duration::from_secs(DOWNLOAD_TIMEOUT_SECS))
            .build()
            .context("Cannot get download HTTP client")?;

        Ok(Self {
            http,
            download_http,
            base_url: format!("http://{host}"),
            username: username.to_string(),
            password: password.to_string(),
            session: None,
            next_id: 1,
        })
    }

    pub async fn login(&mut self) -> Result<()> {
        let id1 = self.next_request_id();
        let step1 = json!({
            "method": "global.login",
            "params": {
                "userName": self.username,
                "password": "",
                "clientType": "Web3.0",
                "loginType": "Direct",
            },
            "id": id1,
        });

        let resp1: Value = self
            .http
            .post(format!("{}/RPC2_Login", self.base_url))
            .json(&step1)
            .send()
            .await
            .context("Network error trying to login (step1)")?
            .json()
            .await
            .context("Not JSON response trying to login (step1)")?;
        let session = resp1
            .get("session")
            .and_then(Value::as_i64)
            .ok_or_else(|| anyhow!("Camera without session in step1: {resp1}"))?;
        let realm = resp1["params"]["realm"]
            .as_str()
            .ok_or_else(|| anyhow!("No realm in step 1 response: {resp1}"))?;
        let random = resp1["params"]["random"]
            .as_str()
            .ok_or_else(|| anyhow!("No random in step 1 response: {resp1}"))?;
        let pwd_md5 = format!(
            "{:X}",
            md5::compute(format!("{}:{}:{}", self.username, realm, self.password))
        );
        let authorization = format!(
            "{:X}",
            md5::compute(format!("{}:{}:{}", self.username, random, pwd_md5))
        );

        let id2 = self.next_request_id();
        let step2 = json!({
            "method": "global.login",
            "params": {
                "userName": self.username,
                "password": authorization,
                "clientType": "Web3.0",
                "loginType": "Direct",
                "authorityType": "Default",
            },
            "id": id2,
            "session": session,
        });

        let resp2: Value = self
            .http
            .post(format!("{}/RPC2_Login", self.base_url))
            .json(&step2)
            .send()
            .await
            .context("Network error trying to login (step2)")?
            .json()
            .await
            .context("Not JSON response trying to login (step2)")?;

        if resp2.get("result") != Some(&Value::Bool(true)) {
            bail!("Error login into camera: {resp2}");
        }

        self.session = resp2
            .get("session")
            .and_then(Value::as_i64)
            .or(Some(session));
        Ok(())
    }

    pub async fn logout(&mut self) -> Result<()> {
        self.call("global.logout", json!(null)).await?;
        self.session = None;
        Ok(())
    }

    pub async fn find_recordings(
        &mut self,
        channel: i64,
        start: DateTime<Local>,
        end: DateTime<Local>,
    ) -> Result<Vec<DahuaRecording>> {
        let create = self
            .call("mediaFileFind.factory.create", json!(null))
            .await?;
        let object_id = create["result"]
            .as_i64()
            .ok_or_else(|| anyhow!("Cannot create recording finder: {create}"))?;
        let result = self
            .find_recordings_inner(object_id, channel, start, end)
            .await;

        if let Err(e) = self.destroy_finder(object_id).await {
            eprintln!("Cannot destroy finder {object_id}: {e:#}");
        }

        result
    }

    async fn find_recordings_inner(
        &mut self,
        object_id: i64,
        channel: i64,
        start: DateTime<Local>,
        end: DateTime<Local>,
    ) -> Result<Vec<DahuaRecording>> {
        // checked against Dahua IPC-HFW2320R-ZS
        let condition = json!({
            "Channel": channel,
            "Dirs": [DEFAULT_DAHUA_SD_DIR],
            "Types": ["dav"],
            "Order": "Ascent",
            "Redundant": "Exclusion",
            "Events": null,
            "Flags": ["Timing", "Event", "Manual"],
            "StartTime": start.format("%Y-%m-%d %H:%M:%S").to_string(),
            "EndTime": end.format("%Y-%m-%d %H:%M:%S").to_string(),
        });

        let find = self
            .call_on_object(
                "mediaFileFind.findFile",
                json!({ "condition": condition }),
                Some(object_id),
            )
            .await?;
        if find.get("result") != Some(&Value::Bool(true)) {
            bail!("error in findFile: {find}");
        }

        let mut recordings = Vec::new();
        loop {
            let next = self
                .call_on_object(
                    "mediaFileFind.findNextFile",
                    json!({ "count": 100 }),
                    Some(object_id),
                )
                .await?;

            let items = next["params"]["infos"]
                .as_array()
                .cloned()
                .unwrap_or_default();
            if items.is_empty() {
                break;
            }

            for item in &items {
                recordings.push(DahuaRecording {
                    file_path: item["FilePath"].as_str().unwrap_or_default().to_string(),
                    start_time: item["StartTime"].as_str().unwrap_or_default().to_string(),
                    end_time: item["EndTime"].as_str().unwrap_or_default().to_string(),
                    length_bytes: item["Length"].as_i64().unwrap_or_default(),
                    channel: item["Channel"].as_i64().unwrap_or(channel),
                    file_type: item["Type"].as_str().unwrap_or("dav").to_string(),
                });
            }
        }

        Ok(recordings)
    }

    pub async fn download_recording(
        &self,
        remote_file_path: &str,
        local_path: &Path,
    ) -> Result<()> {
        use diqwest::WithDigestAuth;
        use futures_util::StreamExt;

        let url = format!("{}/cgi-bin/RPC_Loadfile{}", self.base_url, remote_file_path);
        let response = self
            .download_http
            .get(&url)
            .send_digest_auth((self.username.as_str(), self.password.as_str()))
            .await
            .context("Error starting recording download")?;

        if !response.status().is_success() {
            bail!("Download failed (HTTP {}): {}", response.status(), url);
        }

        let mut file = File::create(local_path)
            .await
            .with_context(|| format!("Cannot create {}", local_path.display()))?;

        let mut stream = response.bytes_stream();

        while let Some(chunk) = stream.next().await {
            let chunk = chunk.context("Error reading download stream")?;
            file.write_all(&chunk).await?;
        }
        file.flush().await?;

        Ok(())
    }

    fn next_request_id(&mut self) -> u32 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    async fn call(&mut self, method: &str, params: Value) -> Result<Value> {
        self.call_on_object(method, params, None).await
    }

    async fn call_on_object(
        &mut self,
        method: &str,
        params: Value,
        object: Option<i64>,
    ) -> Result<Value> {
        let id = self.next_request_id();
        let mut body = json!({
            "method": method,
            "params": params,
            "id": id,
        });
        if let Some(session) = self.session {
            body["session"] = json!(session);
        }
        if let Some(obj) = object {
            body["object"] = json!(obj);
        }

        let resp: Value = self
            .http
            .post(format!("{}/RPC2", self.base_url))
            .json(&body)
            .send()
            .await
            .with_context(|| format!("Network error calling {method}"))?
            .json()
            .await
            .with_context(|| format!("Not JSON response calling {method}"))?;

        if resp.get("result") == Some(&Value::Bool(false)) {
            bail!("camera refused {method}: {resp}");
        }
        Ok(resp)
    }

    async fn destroy_finder(&mut self, object_id: i64) -> Result<()> {
        self.call_on_object("mediaFileFind.close", json!(null), Some(object_id))
            .await
            .ok();
        self.call_on_object("mediaFileFind.destroy", json!(null), Some(object_id))
            .await?;
        Ok(())
    }
}

pub fn parse_dahua_time(
    time_string: &str,
    local_offset: FixedOffset,
) -> anyhow::Result<DateTime<Utc>> {
    let naive = NaiveDateTime::parse_from_str(time_string, "%Y-%m-%d %H:%M:%S")?;
    let local = local_offset
        .from_local_datetime(&naive)
        .single()
        .ok_or_else(|| anyhow::anyhow!("hora local ambigua o inválida: {time_string}"))?;
    Ok(local.with_timezone(&Utc))
}

pub fn get_chosen_recording(
    recordings: &[Recording],
    event_time: DateTime<Utc>,
) -> Option<Recording> {
    let local_offset = *event_time.with_timezone(&Local).offset();

    let parsed: Vec<(&Recording, DateTime<Utc>, DateTime<Utc>)> = recordings
        .iter()
        .filter_map(|r| {
            let begin = parse_dahua_time(&r.begin, local_offset).ok()?;
            let end = parse_dahua_time(&r.end, local_offset).ok()?;
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

fn compute_clip_window(
    recording: &Recording,
    time: DateTime<Utc>,
    clip_time: Duration,
) -> anyhow::Result<(f64, f64)> {
    let (clip_start_time, clip_end_time) = get_clip_interval(time, clip_time);

    if (clip_end_time - clip_start_time).num_seconds() > MAX_DOWNLOAD_SECONDS {
        anyhow::bail!("Downloads of more than {MAX_DOWNLOAD_SECONDS} are forbidden in this way");
    }

    let local_offset = *time.with_timezone(&Local).offset();
    let recording_start = parse_dahua_time(&recording.begin, local_offset)
        .context("cannot parse recording begin time")?;
    let recording_end = parse_dahua_time(&recording.end, local_offset)
        .context("cannot parse recording end time")?;

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
        "download_recording -> time:{time:?} local_offset:{local_offset:?} recording:{recording_start:?}-{recording_end:?} window:{window_start:?}-{window_end:?} offset:{offset_secs:?} duration:{duration_secs:?}"
    );

    Ok((offset_secs, duration_secs))
}

async fn ffmpeg_trim_and_convert(
    input: &Path,
    output: &Path,
    offset_secs: f64,
    duration_secs: f64,
) -> anyhow::Result<()> {
    let mut args: Vec<String> = Vec::new();
    if offset_secs > 0.05 {
        args.push("-ss".into());
        args.push(offset_secs.to_string());
    }
    args.push("-i".into());
    args.push(input.to_str().unwrap().into());
    args.push("-t".into());
    args.push(duration_secs.to_string());
    args.push("-c:v".into());
    args.push("copy".into());
    args.push("-c:a".into());
    args.push("aac".into());
    args.push("-b:a".into());
    args.push("64k".into());
    args.push("-movflags".into());
    args.push("+faststart".into());
    args.push("-y".into());
    args.push(output.to_str().unwrap().into());
    let args_ref: Vec<&str> = args.iter().map(String::as_str).collect();
    let fast_result = run_ffmpeg(&args_ref).await;

    let fast_ok = fast_result.is_ok()
        && tokio::fs::metadata(output)
            .await
            .map(|m| m.len() > 0)
            .unwrap_or(false);

    if fast_ok {
        return Ok(());
    }

    tracing::warn!(
        "error doing fast remux ({:?}), trying to decoding as fallback",
        fast_result.err()
    );

    args.clear();
    if offset_secs > 0.05 {
        args.push("-ss".into());
        args.push(offset_secs.to_string());
    }
    args.push("-i".into());
    args.push(input.to_str().unwrap().into());
    args.push("-t".into());
    args.push(duration_secs.to_string());
    args.push("-c:v".into());
    args.push("libx264".into());
    args.push("-preset".into());
    args.push("veryfast".into());
    args.push("-crf".into());
    args.push("23".into());
    args.push("-c:a".into());
    args.push("aac".into());
    args.push("-b:a".into());
    args.push("64k".into());
    args.push("-movflags".into());
    args.push("+faststart".into());
    args.push("-y".into());
    args.push(output.to_str().unwrap().into());
    let args_ref: Vec<&str> = args.iter().map(String::as_str).collect();

    run_ffmpeg(&args_ref).await
}
