use crate::{CameraId, ChatId, MessageId};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use thiserror::Error;

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
    async fn download_recording(
        &self,
        chat_id: ChatId,
        message_id: MessageId,
        camera_id: CameraId,
        time: DateTime<Utc>,
    ) -> Result<(), DownloadRecordingError>;
}

#[derive(Error, Debug)]
pub enum DownloadRecordingError {
    #[error("Camera not found")]
    CameraNotFound,
    #[error("Max file size reached")]
    MaxFileSize,
    #[error("No recordings")]
    NoRecordings,
    #[error("No recordings yet")]
    NoRecordingsYet,
    #[error("Recording download not available")]
    RecordingDownloadNotAvailable,
    #[error("Error sending video")]
    ErrorSendingVideo,
    #[error("Other error")]
    Other,
}
