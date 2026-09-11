use std::path::Path;

use app_core::{domain::camera::Recording, traits::api_camera_client::ApiCameraClient};
use async_trait::async_trait;
use chrono::{DateTime, Duration, Local, NaiveDateTime, TimeZone, Utc};
use dvrip_rs::{Authentication, Connection, DVRIPCam, FileManagement};

use crate::{MAX_DOWNLOAD_SECONDS, MAX_FILES, get_clip_interval, run_ffmpeg};

// Implementation for XMEye-icSEE compatible chinese cameras. DvrIp is the protocol they use

const CAMERA_TIME_FORMAT: &str = "%Y-%m-%d %H:%M:%S";
const CHANNEL: u8 = 0;

pub struct DvrIpXmeyeApiCameraClient {
    host: String,
    user: String,
    password: String,
}

fn normalize_host_for_xmeye(host: &str) -> String {
    let without_scheme = host
        .trim_start_matches("http://")
        .trim_start_matches("https://");
    without_scheme
        .split(':')
        .next()
        .unwrap_or(without_scheme)
        .to_string()
}

impl DvrIpXmeyeApiCameraClient {
    pub fn new(host: String, user: String, password: String) -> Self {
        let normalized_host = normalize_host_for_xmeye(&host);
        tracing::info!(
            "created new DvrIpXmeyeApiCameraClient in host: {} and username: {}",
            normalized_host,
            user
        );

        Self {
            host: normalized_host,
            user,
            password,
        }
    }

    async fn connect_and_login(&self) -> anyhow::Result<DVRIPCam> {
        let mut cam = DVRIPCam::new(self.host.clone());
        cam.connect(tokio::time::Duration::from_secs(10)).await?;
        cam.login(&self.user, &self.password).await?;
        Ok(cam)
    }
}

#[async_trait]
impl ApiCameraClient for DvrIpXmeyeApiCameraClient {
    async fn init(&mut self, host: Option<String>, user: Option<String>, password: Option<String>) {
        self.host = host.unwrap_or(self.host.clone());
        self.user = user.unwrap_or(self.user.clone());
        self.password = password.unwrap_or(self.password.clone());
    }

    async fn get_recordings(
        &self,
        time: chrono::DateTime<chrono::Utc>,
        clip_time: chrono::Duration,
    ) -> anyhow::Result<Vec<Recording>> {
        let mut recordings = Vec::new();
        let (start_time, end_time) = get_clip_interval(time, clip_time);

        let mut cam = self.connect_and_login().await?;
        let result = cam
            .list_local_files(
                chrono::DateTime::from(start_time),
                chrono::DateTime::from(end_time),
                "video",
                0,
            )
            .await;
        cam.close().await?;

        match result {
            Ok(files) => {
                tracing::info!("Found {} files.", files.len());

                for file in files.iter().take(MAX_FILES) {
                    let name = file
                        .get("FileName")
                        .and_then(|f| f.as_str())
                        .unwrap_or("Unknown")
                        .to_string();
                    let size_str = file
                        .get("FileLength")
                        .and_then(|s| s.as_str())
                        .unwrap_or("0");
                    let size =
                        u64::from_str_radix(size_str.trim_start_matches("0x"), 16).unwrap_or(0);
                    let size_mb = size as f64 / 1024.0;
                    let begin = file
                        .get("BeginTime")
                        .and_then(|t| t.as_str())
                        .unwrap_or("?")
                        .to_string();
                    let end = file
                        .get("EndTime")
                        .and_then(|t| t.as_str())
                        .unwrap_or("?")
                        .to_string();

                    recordings.push(Recording {
                        name,
                        size_mb,
                        begin,
                        end,
                    });
                }
            }
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
        if recordings.is_empty() {
            anyhow::bail!("no recordings to download");
        }

        let (start_time, end_time) = get_clip_interval(event_time, clip_time);

        if (end_time - start_time).num_seconds() > MAX_DOWNLOAD_SECONDS {
            anyhow::bail!(
                "Downloads of more than {MAX_DOWNLOAD_SECONDS} are forbidden in this way"
            );
        }

        let target_name_with_extension = self
            .download_window(recordings, start_time, end_time, &target_name)
            .await?;

        tracing::info!(
            "download_recording -> file {target_name_with_extension:?} downloaded from camera"
        );

        let output_file = format!("./{target_name}.mp4");
        if let Err(err) = ffmpeg_convert_h265(
            Path::new(&target_name_with_extension),
            Path::new(&output_file),
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

impl DvrIpXmeyeApiCameraClient {
    async fn download_window(
        &self,
        recordings: &[Recording],
        start_time: DateTime<Utc>,
        end_time: DateTime<Utc>,
        target_name: &str,
    ) -> anyhow::Result<String> {
        let start_local: DateTime<Local> = DateTime::from(start_time);
        let end_local: DateTime<Local> = DateTime::from(end_time);
        let want_begin = start_local.naive_local();
        let want_end = end_local.naive_local();

        let mut sorted_segments: Vec<(NaiveDateTime, NaiveDateTime)> = recordings
            .iter()
            .filter_map(|r| {
                let seg_begin = NaiveDateTime::parse_from_str(&r.begin, CAMERA_TIME_FORMAT).ok()?;
                let seg_end = NaiveDateTime::parse_from_str(&r.end, CAMERA_TIME_FORMAT).ok()?;
                Some((seg_begin, seg_end))
            })
            .collect();
        sorted_segments.sort_by_key(|(begin, _)| *begin);

        if sorted_segments.is_empty() {
            anyhow::bail!("all segments has not a valid date");
        }

        let mut cam = self.connect_and_login().await?;
        let mut combined = Vec::new();
        let mut any_downloaded = false;

        for (idx, (seg_begin, seg_end)) in sorted_segments.into_iter().enumerate() {
            let clamp_begin = seg_begin.max(want_begin);
            let clamp_end = seg_end.min(want_end);
            if clamp_begin >= clamp_end {
                continue;
            }

            let clamp_begin_local = Local.from_local_datetime(&clamp_begin).unwrap();
            let clamp_end_local = Local.from_local_datetime(&clamp_end).unwrap();
            let part_path = std::env::temp_dir().join(format!("{target_name}_{idx}.h265part"));
            let part_path_str = part_path.to_string_lossy().into_owned();

            tracing::info!("downloading segment {clamp_begin} - {clamp_end}");
            match cam
                .download_file_by_time(clamp_begin_local, clamp_end_local, CHANNEL, &part_path_str)
                .await
            {
                Ok(()) => {
                    let bytes = tokio::fs::read(&part_path).await?;
                    combined.extend_from_slice(&bytes);
                    tokio::fs::remove_file(&part_path).await.ok();
                    any_downloaded = true;
                }
                Err(e) => {
                    tracing::warn!("cannot download segment {clamp_begin} - {clamp_end}: {e}");
                }
            }
        }

        cam.close().await.ok();

        if !any_downloaded {
            anyhow::bail!("cannot download any segment");
        }

        let raw_path = format!("./{target_name}.h265x");
        tokio::fs::write(&raw_path, &combined).await?;
        Ok(raw_path)
    }
}

async fn ffmpeg_convert_h265(input: &Path, output: &Path) -> anyhow::Result<()> {
    let fast_result = run_ffmpeg(&[
        "-f",
        "hevc",
        "-i",
        input.to_str().unwrap(),
        "-c",
        "copy",
        "-tag:v",
        "hvc1",
        "-movflags",
        "+faststart",
        "-y",
        output.to_str().unwrap(),
    ])
    .await;

    let fast_ok = fast_result.is_ok()
        && tokio::fs::metadata(output)
            .await
            .map(|m| m.len() > 0)
            .unwrap_or(false);

    if fast_ok {
        return Ok(());
    }

    tracing::warn!(
        "error doing fast HEVC remux HEVC ({:?}), trying to decoding to H264 as fallback",
        fast_result.err()
    );

    run_ffmpeg(&[
        "-f",
        "hevc",
        "-i",
        input.to_str().unwrap(),
        "-c:v",
        "libx264",
        "-preset",
        "veryfast",
        "-crf",
        "23",
        "-c:a",
        "aac",
        "-b:a",
        "64k",
        "-movflags",
        "+faststart",
        "-y",
        output.to_str().unwrap(),
    ])
    .await
}
