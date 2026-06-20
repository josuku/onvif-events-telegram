use async_trait::async_trait;

use crate::domain::camera::{CameraConnectionData, OnvifCameraEvent};

#[async_trait]
pub trait CameraClient: Send + Sync {
    async fn get_snapshot_uri(&self) -> anyhow::Result<String>;
    async fn snapshot(&self) -> anyhow::Result<Vec<u8>>;
    async fn get_event_message(&self) -> anyhow::Result<Option<OnvifCameraEvent>>;
    fn connected(&self) -> bool;
    fn get_connection_data(&self) -> CameraConnectionData;
    async fn create_user_and_fix_snapshot_uri(
        &self,
        camera_uri: &str,
        orig_snapshot_uri: &str,
    ) -> anyhow::Result<String>;
}
