use crate::domain::camera::{Recording, RecordingSummary};
use async_trait::async_trait;
use chrono::{DateTime, Duration, Utc};

#[async_trait]
pub trait ApiCameraClient: Send + Sync {
    async fn init(&mut self, host: Option<String>, user: Option<String>, password: Option<String>);
    async fn get_recordings(
        &self,
        time: DateTime<Utc>,
        clip_time: Duration,
    ) -> anyhow::Result<Vec<Recording>>;
    async fn get_recording_summary(
        &self,
        start_time: Option<DateTime<Utc>>,
        end_time: Option<DateTime<Utc>>,
    ) -> anyhow::Result<RecordingSummary>;
    async fn get_recording_list(
        &self,
        time: DateTime<Utc>,
        clip_time: Duration,
    ) -> anyhow::Result<Vec<Recording>>;
    async fn download_recording(
        &self,
        recording: &[Recording],
        time: DateTime<Utc>,
        clip_time: Duration,
        target_name: String,
    ) -> anyhow::Result<String>;
    async fn set_optical_zoom(&self, zoom: u8) -> anyhow::Result<()>;
    async fn reboot(&self) -> anyhow::Result<()>;
}
