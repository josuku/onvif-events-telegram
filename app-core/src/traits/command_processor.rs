use crate::{CameraId, ChatId};
use async_trait::async_trait;

#[async_trait]
pub trait CommandProcessor: Send + Sync {
    async fn help_cmd(&self, chat_id: ChatId, help_text: &str) -> anyhow::Result<()>;
    async fn get_cameras_cmd(&self, chat_id: ChatId) -> anyhow::Result<()>;
    async fn set_camera_name_cmd(
        &self,
        chat_id: ChatId,
        camera_id: CameraId,
        camera_name: &str,
    ) -> anyhow::Result<()>;
    async fn subscribe_cmd(&self, chat_id: ChatId, camera_id: CameraId) -> anyhow::Result<()>;
    async fn unsubscribe_cmd(&self, chat_id: ChatId, camera_id: CameraId) -> anyhow::Result<()>;
    async fn get_snapshot_cmd(
        &self,
        chat_id: ChatId,
        camera_id: Option<CameraId>,
    ) -> anyhow::Result<()>;
    async fn set_polling_time_cmd(&self, chat_id: ChatId, seconds: u64) -> anyhow::Result<()>;
    async fn set_between_time_cmd(&self, chat_id: ChatId, seconds: u64) -> anyhow::Result<()>;
    async fn fix_snapshot_uri_cmd(
        &self,
        chat_id: ChatId,
        camera_id: CameraId,
    ) -> anyhow::Result<()>;
    async fn enable_daily_report_cmd(&self, chat_id: ChatId, enable: bool) -> anyhow::Result<()>;
    async fn add_camera_cmd(
        &self,
        chat_id: ChatId,
        uri: &str,
        username: &str,
        password: &str,
    ) -> anyhow::Result<()>;
    async fn delete_camera_cmd(&self, chat_id: ChatId, camera_id: CameraId) -> anyhow::Result<()>;
    async fn set_credentials_cmd(
        &self,
        chat_id: ChatId,
        camera_id: CameraId,
        username: &str,
        password: &str,
    ) -> anyhow::Result<()>;
}
