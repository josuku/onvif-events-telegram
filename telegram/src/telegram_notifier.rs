use anyhow::bail;
use app_core::{traits::notifier::Notifier, CameraId, ChatId};
use async_trait::async_trait;
use repository::memory_repository::MemoryRepository;
use std::sync::Arc;
use teloxide::{payloads::SendPhotoSetters, prelude::Requester, types::InputFile, Bot};
use tracing::warn;

#[derive(Clone)]
pub struct TelegramNotifier {
    client: Bot,
    repository: Arc<MemoryRepository>,
}

impl TelegramNotifier {
    pub fn new(bot_token: String, repository: Arc<MemoryRepository>) -> Self {
        Self {
            client: Bot::new(bot_token),
            repository,
        }
    }
}

#[async_trait]
impl Notifier for TelegramNotifier {
    async fn send_text_message(&self, message: String, chat_ids: Vec<ChatId>) {
        for chat_id in chat_ids {
            let _ = self
                .client
                .send_message(teloxide::prelude::ChatId(chat_id), message.clone())
                .await;
        }
    }

    async fn send_picture_message(
        &self,
        message: &str,
        picture: Vec<u8>,
        chat_id: ChatId,
    ) -> anyhow::Result<()> {
        let file = InputFile::memory(picture.clone()).file_name("new_file.jpg");

        if let Err(err) = self
            .client
            .send_photo(teloxide::prelude::ChatId(chat_id), file)
            .caption(message)
            .await
        {
            bail!("cannot send picture to Telegram {:?}", err)
        }
        Ok(())
    }

    async fn send_text_with_picture_message(
        &self,
        message: String,
        picture: Vec<u8>,
        chat_ids: Vec<ChatId>,
        camera_id: CameraId,
    ) {
        for chat_id in chat_ids {
            let last_notification_time = self
                .repository
                .get_last_notification_time(camera_id, chat_id)
                .await;
            let between_seconds = self.repository.get_between_seconds().await;

            if last_notification_time.is_none()
                || chrono::Utc::now().timestamp() - last_notification_time.unwrap().timestamp()
                    > between_seconds.try_into().unwrap()
            {
                if let Ok(()) = self
                    .send_picture_message(&message, picture.clone(), chat_id)
                    .await
                {
                    self.repository
                        .update_last_notification_time(camera_id, chat_id)
                        .await;
                }
            } else {
                warn!(
                    "skipping notification of camera {} to chatId {}",
                    camera_id, chat_id
                );
            }
        }
    }
}
