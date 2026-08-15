use crate::{CameraId, ChatId, MessageId};
use async_trait::async_trait;
use chrono::{DateTime, Utc};

#[async_trait]
pub trait Notifier: Send + Sync {
    async fn send_text_message(&self, message: String, chat_ids: Vec<ChatId>);

    async fn send_picture_message(
        &self,
        message: &str,
        picture: Vec<u8>,
        chat_id: ChatId,
        camera_id: CameraId,
        time: &DateTime<Utc>,
    ) -> anyhow::Result<()>;

    async fn send_text_with_picture_message(
        &self,
        message: String,
        picture: Vec<u8>,
        chat_ids: Vec<ChatId>,
        camera_id: CameraId,
        time: &DateTime<Utc>,
    );

    async fn send_video_message(
        &self,
        video_path: &str,
        chat_id: ChatId,
        message_id: Option<MessageId>,
    ) -> anyhow::Result<()>;
}
