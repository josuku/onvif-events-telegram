use app_core::{domain::camera::Recording, traits::api_camera_client::ApiCameraClient};
use async_trait::async_trait;
use chrono::{DateTime, Duration, Utc};
use dvrip_rs::{Authentication, Connection, DVRIPCam, FileManagement};

use crate::{MAX_DOWNLOAD_SECONDS, MAX_FILES, get_clip_interval};

// Implementation for XMEye-icSEE compatible chinese cameras. DvrIp is the protocol they use

pub struct DvrIpXmeyeApiCameraClient {
    host: String,
    user: String,
    password: String,
}

impl DvrIpXmeyeApiCameraClient {
    pub fn new(host: String, user: String, password: String) -> Self {
        Self {
            host,
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
        _recording: &Recording,
        time: DateTime<Utc>,
        clip_time: Duration,
        source_name: String,
        target_name: String,
    ) -> anyhow::Result<String> {
        let (start_time, end_time) = get_clip_interval(time, clip_time);

        if (end_time - start_time).num_seconds() > MAX_DOWNLOAD_SECONDS {
            anyhow::bail!(
                "Downloads of more than {MAX_DOWNLOAD_SECONDS} are forbidden in this way"
            );
        }

        let mut cam = self.connect_and_login().await?;

        tracing::info!("Downloading file to '{target_name}' (this may take time)...");

        let result = cam
            .download_file(
                chrono::DateTime::from(start_time),
                chrono::DateTime::from(end_time),
                &source_name,
                &target_name,
            )
            .await;
        cam.close().await?;

        // TODO reencode with ffmpeg if needed

        if let Err(err) = result {
            anyhow::bail!("Download failed: {:?}", err)
        }
        Ok(target_name)
    }
}
