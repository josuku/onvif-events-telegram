use anyhow::{bail, Context};
use app_core::{traits::notifier::Notifier, CameraId, ChatId, MessageId};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use repository::memory_repository::MemoryRepository;
use std::{path::Path, sync::Arc, time::Duration};
use teloxide::{
    payloads::{SendPhotoSetters, SendVideoSetters},
    prelude::Requester,
    types::{InlineKeyboardButton, InlineKeyboardMarkup, InputFile, ReplyParameters},
    Bot,
};
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
        camera_id: CameraId,
        time: &DateTime<Utc>,
    ) -> anyhow::Result<()> {
        let file = InputFile::memory(picture.clone()).file_name("new_file.jpg");

        let go_to_recording = InlineKeyboardMarkup::new([[InlineKeyboardButton::callback(
            "📹 Get Recording",
            format!("recording|{}|{}", camera_id, time.to_rfc3339()),
        )]]);

        if let Err(err) = self
            .client
            .send_photo(teloxide::prelude::ChatId(chat_id), file)
            .caption(message)
            .reply_markup(go_to_recording)
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
        time: &DateTime<Utc>,
    ) {
        for chat_id in chat_ids {
            let last_notification_time = self
                .repository
                .get_last_notification_time(camera_id, chat_id)
                .await;
            let between_seconds = self.repository.get_config().await.between_seconds;

            if last_notification_time.is_none()
                || chrono::Utc::now().timestamp() - last_notification_time.unwrap().timestamp()
                    > between_seconds.try_into().unwrap()
            {
                if let Ok(()) = self
                    .send_picture_message(&message, picture.clone(), chat_id, camera_id, time)
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

    async fn send_video_message(
        &self,
        video_path: &str,
        chat_id: ChatId,
        message_id: Option<MessageId>,
    ) -> anyhow::Result<()> {
        let bytes = tokio::fs::read(video_path)
            .await
            .with_context(|| format!("cannot read video file {video_path}"))?;

        let file_name = Path::new(video_path)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("video.mp4")
            .to_string();

        let file = InputFile::memory(bytes).file_name(file_name);
        // let file = InputFile::file(video_path);
        let chat_id = teloxide::prelude::ChatId(chat_id);

        let client = teloxide::net::default_reqwest_settings()
            .timeout(Duration::from_secs(300))
            .build()
            .unwrap();

        let bot_to_send_video = Bot::with_client(self.client.token(), client);

        let result = if let Some(message_id) = message_id {
            let message_id = teloxide::types::MessageId(message_id);
            bot_to_send_video
                .send_video(chat_id, file)
                .reply_parameters(ReplyParameters::new(message_id))
                .await
        } else {
            self.client.send_video(chat_id, file).await
        };

        if let Err(err) = result {
            bail!("{:?}", err)
        }
        Ok(())
    }
}
