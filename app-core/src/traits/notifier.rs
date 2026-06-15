use crate::{CameraId, ChatId};
use async_trait::async_trait;

#[async_trait]
pub trait Notifier: Send + Sync {
    async fn send_text_message(&self, message: String, chat_ids: Vec<ChatId>);

    async fn send_picture_message(
        &self,
        message: &str,
        picture: Vec<u8>,
        chat_id: ChatId,
    ) -> anyhow::Result<()>;

    async fn send_text_with_picture_message(
        &self,
        message: String,
        picture: Vec<u8>,
        chat_ids: Vec<ChatId>,
        camera_id: CameraId,
    );
}
