use std::path::Path;

use app_core::{
    domain::camera::{Recording, RecordingSummary},
    traits::api_camera_client::ApiCameraClient,
};
use async_trait::async_trait;
use chrono::{DateTime, Duration, Local, NaiveDateTime, TimeZone, Utc};
use dvrip_rs::{Authentication, Connection, DVRIPCam, FileManagement, SystemInfo};

use crate::{MAX_FILES, ffmpeg_trim_and_convert, get_clip_interval, get_clip_interval_clamped};

// Implementation for XMEye-icSEE compatible chinese cameras. DvrIp is the protocol they use

const CAMERA_TIME_FORMAT: &str = "%Y-%m-%d %H:%M:%S";
const CHANNEL: u8 = 0;
const SEGMENT_TIMEOUT: tokio::time::Duration = tokio::time::Duration::from_secs(30);
const CONNECT_TIMEOUT: tokio::time::Duration = tokio::time::Duration::from_secs(10);

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
        cam.connect(CONNECT_TIMEOUT).await?;
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

    async fn get_recording_summary(
        &self,
        start_time: Option<DateTime<Utc>>,
        end_time: Option<DateTime<Utc>>,
    ) -> anyhow::Result<RecordingSummary> {
        let end = end_time.unwrap_or_else(Utc::now);
        let start = start_time.unwrap_or_else(|| end - Duration::days(30));

        let mut cam = self.connect_and_login().await?;
        let result = cam
            .list_local_files(
                chrono::DateTime::from(start),
                chrono::DateTime::from(end),
                "video",
                0,
            )
            .await;
        cam.close().await?;

        let files = result?;
        let total_files = files.len();
        let mut total_size_mb = 0.0;
        let mut first_begin = None;
        let mut last_end = None;

        for (i, file) in files.iter().enumerate() {
            let size_str = file
                .get("FileLength")
                .and_then(|s| s.as_str())
                .unwrap_or("0");
            let size = u64::from_str_radix(size_str.trim_start_matches("0x"), 16).unwrap_or(0);
            // FileLength in DVRIP is in KB, so dividing by 1024 gives MB.
            total_size_mb += size as f64 / 1024.0;

            if i == 0 {
                first_begin = file
                    .get("BeginTime")
                    .and_then(|t| t.as_str())
                    .map(|s| s.to_string());
            }
            if i == files.len() - 1 {
                last_end = file
                    .get("EndTime")
                    .and_then(|t| t.as_str())
                    .map(|s| s.to_string());
            }
        }

        Ok(RecordingSummary {
            total_files,
            total_size_mb,
            first_begin,
            last_end,
        })
    }

    async fn get_recording_list(
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
                for file in files {
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

        let (start_time, end_time) = get_clip_interval_clamped(event_time, clip_time);

        let target_name_with_extension = self
            .download_window(recordings, event_time, start_time, end_time, &target_name)
            .await?;

        tracing::info!(
            "download_recording -> file {target_name_with_extension:?} downloaded from camera"
        );

        let total_seconds = (end_time - start_time).num_seconds().max(1);
        let output_file = format!("./{target_name}.mp4");

        if let Err(err) = ffmpeg_trim_and_convert(
            Path::new(&target_name_with_extension),
            Path::new(&output_file),
            Some("hevc"),
            0.0,
            total_seconds as f64,
            Some("hvc1"),
            false,
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

    async fn set_optical_zoom(&self, _zoom: u8) -> anyhow::Result<()> {
        anyhow::bail!("not implemented");
    }

    async fn reboot(&self) -> anyhow::Result<()> {
        let mut cam = self.connect_and_login().await?;
        tracing::info!("reboot -> connected to XMeye/DVRIP camera");
        let result = cam.reboot().await;
        tracing::info!("reboot -> DVRIP result: {result:?}");

        match result {
            Ok(true) => {
                // Xmeye cameras take 5-10s to reboo, wait not to cancel reboot
                tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
                cam.close().await.ok();
                Ok(())
            }
            Ok(false) => {
                cam.close().await.ok();
                anyhow::bail!("DVRIP reboot command returned false/failed");
            }
            Err(e) => {
                cam.close().await.ok();
                anyhow::bail!("DVRIP reboot failed: {e}");
            }
        }
    }
}

impl DvrIpXmeyeApiCameraClient {
    async fn download_window(
        &self,
        recordings: &[Recording],
        event_time: DateTime<Utc>,
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

        let mut combined = Vec::new();
        let mut any_downloaded = false;

        // If there's an exact file that matches event_time or contains it, restrict the download
        // strictly to that segment to avoid downloading adjacent segments that share boundary timestamps.
        let local_offset = *event_time.with_timezone(&Local).offset();
        let target_segment = sorted_segments
            .iter()
            .find(|(begin, end)| {
                let seg_b = local_offset
                    .from_local_datetime(begin)
                    .single()
                    .map(|dt| dt.with_timezone(&Utc));
                let seg_e = local_offset
                    .from_local_datetime(end)
                    .single()
                    .map(|dt| dt.with_timezone(&Utc));
                if let (Some(b), Some(e)) = (seg_b, seg_e) {
                    b <= event_time && event_time < e
                } else {
                    false
                }
            })
            .copied();

        let segments_to_download: Vec<(NaiveDateTime, NaiveDateTime)> =
            if let Some(target) = target_segment {
                vec![target]
            } else {
                sorted_segments
            };

        for (idx, (seg_begin, seg_end)) in segments_to_download.into_iter().enumerate() {
            let clamp_begin = seg_begin.max(want_begin);
            let mut clamp_end = seg_end.min(want_end);

            if clamp_end == seg_end {
                // xmeye fix, stop 1 second before or it will be continue
                clamp_end -= Duration::seconds(1);
            }

            if clamp_begin >= clamp_end {
                continue;
            }

            let clamp_begin_local = Local.from_local_datetime(&clamp_begin).unwrap();
            let clamp_end_local = Local.from_local_datetime(&clamp_end).unwrap();
            let part_path = std::env::temp_dir().join(format!("{target_name}_{idx}.h265part"));
            let part_path_str = part_path.to_string_lossy().into_owned();

            let mut cam = match self.connect_and_login().await {
                Ok(cam) => cam,
                Err(e) => {
                    tracing::warn!(
                        "cannot connect to download segment {clamp_begin} - {clamp_end}: {e}"
                    );
                    continue;
                }
            };

            tracing::info!("downloading segment {clamp_begin} - {clamp_end}");
            let result = tokio::time::timeout(
                SEGMENT_TIMEOUT,
                cam.download_file_by_time(
                    clamp_begin_local,
                    clamp_end_local,
                    CHANNEL,
                    &part_path_str,
                ),
            )
            .await;

            match result {
                Ok(Ok(())) => {
                    let bytes = tokio::fs::read(&part_path).await?;
                    combined.extend_from_slice(&bytes);
                    tokio::fs::remove_file(&part_path).await.ok();
                    any_downloaded = true;
                }
                Ok(Err(e)) => {
                    tracing::warn!("cannot download segment {clamp_begin} - {clamp_end}: {e}");
                    if let Ok(bytes) = tokio::fs::read(&part_path).await
                        && !bytes.is_empty()
                    {
                        tracing::info!(
                            "using {} bytes before segment error {clamp_begin} - {clamp_end}",
                            bytes.len()
                        );
                        combined.extend_from_slice(&bytes);
                        any_downloaded = true;
                    }
                    tokio::fs::remove_file(&part_path).await.ok();
                }
                Err(_elapsed) => {
                    tracing::warn!(
                        "timeout ({SEGMENT_TIMEOUT:?}) downloading segment {clamp_begin} - {clamp_end}"
                    );
                    if let Ok(bytes) = tokio::fs::read(&part_path).await
                        && !bytes.is_empty()
                    {
                        tracing::info!(
                            "using {} bytes before segment timeout {clamp_begin} - {clamp_end}",
                            bytes.len()
                        );
                        combined.extend_from_slice(&bytes);
                        any_downloaded = true;
                    }
                    tokio::fs::remove_file(&part_path).await.ok();
                }
            }

            let _ = tokio::time::timeout(std::time::Duration::from_secs(3), cam.close()).await;
        }

        if !any_downloaded {
            anyhow::bail!("cannot download any segment");
        }

        let raw_path = format!("./{target_name}.h265x");
        tokio::fs::write(&raw_path, &combined).await?;
        Ok(raw_path)
    }
}
