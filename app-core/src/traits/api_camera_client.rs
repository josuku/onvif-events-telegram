use crate::domain::camera::Recording;
use async_trait::async_trait;

#[async_trait]
pub trait ApiCameraClient: Send + Sync {
    async fn init(&mut self, host: Option<String>, user: Option<String>, password: Option<String>);
    async fn get_recordings(
        &self,
        start_time: chrono::DateTime<chrono::Utc>,
        end_time: chrono::DateTime<chrono::Utc>,
    ) -> anyhow::Result<Vec<Recording>>;
    async fn download_recording(
        &self,
        start_time: chrono::DateTime<chrono::Utc>,
        end_time: chrono::DateTime<chrono::Utc>,
        source_name: String,
        target_name: String,
    ) -> anyhow::Result<()>;
}
