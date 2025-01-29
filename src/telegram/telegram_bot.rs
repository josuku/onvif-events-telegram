use super::bot_command::{command_handler, BotCommand};
use crate::{repository::memory_repository::MemoryRepository, CameraId};
use anyhow::bail;
use std::sync::Arc;
use teloxide::{prelude::*, types::InputFile, Bot};

#[derive(Clone)]
pub struct TelegramBot {
    client: Bot,
    allowed_chat_ids: Vec<String>,
    repository: Arc<MemoryRepository>,
}

impl TelegramBot {
    pub fn new(
        bot_token: String,
        chat_ids: Vec<String>,
        repository: Arc<MemoryRepository>,
    ) -> Self {
        Self {
            client: Bot::new(bot_token),
            allowed_chat_ids: chat_ids,
            repository,
        }
    }

    pub async fn start(&self) {
        let handler = Update::filter_message().branch(
            dptree::entry()
                .filter_command::<BotCommand>()
                .endpoint(command_handler),
        );

        Dispatcher::builder(self.client.clone(), handler)
            .dependencies(dptree::deps![
                self.allowed_chat_ids.clone(),
                self.repository.clone(),
                self.clone()
            ])
            .build()
            .dispatch()
            .await;
    }

    pub async fn send_notification(
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
                if let Ok(()) = self.send_picture(&message, picture.clone(), chat_id).await {
                    self.repository
                        .update_last_notification_time(camera_id, chat_id)
                        .await;
                }
            } else {
                println!(
                    "skipping notification of camera {} to chatId {}",
                    camera_id, chat_id
                );
            }
        }
    }

    pub async fn send_picture(
        &self,
        message: &str,
        picture: Vec<u8>,
        chat_id: ChatId,
    ) -> anyhow::Result<()> {
        let file = InputFile::memory(picture.clone()).file_name("new_file.jpg");
        if let Err(err) = self.client.send_photo(chat_id, file).caption(message).await {
            bail!("cannot send picture to Telegram {:?}", err)
        }
        Ok(())
    }

    pub async fn send_message(&self, message: String, chat_ids: Vec<ChatId>) {
        for chat_id in chat_ids {
            let _ = self.client.send_message(chat_id, message.clone()).await;
        }
    }
}
