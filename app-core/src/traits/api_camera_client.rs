use crate::domain::camera::Recording;
use async_trait::async_trait;

#[async_trait]
pub trait ApiCameraClient: Send + Sync {
    async fn init(&mut self, host: Option<String>, user: Option<String>, password: Option<String>);
    async fn get_recordings(
        &self,
        time: chrono::DateTime<chrono::Utc>,
        clip_time: chrono::Duration,
    ) -> anyhow::Result<Vec<Recording>>;
    async fn download_recording(
        &self,
        recording: &Recording,
        time: chrono::DateTime<chrono::Utc>,
        clip_time: chrono::Duration,
        target_name: String,
    ) -> anyhow::Result<String>;
}
