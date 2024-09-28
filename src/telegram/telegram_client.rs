use crate::{
    onvif::onvif_client::camera_discovery, repository::memory_repository::MemoryRepository,
    CameraId,
};
use log::{error, info};
use std::sync::Arc;
use teloxide::{
    prelude::*,
    types::{InputFile, Message},
    utils::command::BotCommands,
    Bot,
};

#[derive(BotCommands, Clone)]
#[command(
    rename_rule = "lowercase",
    description = "These commands are supported:"
)]
enum Command {
    #[command(description = "show command's list.")]
    Help,
    #[command(description = "get available cameras.")]
    GetCameras,
    #[command(description = "subscribe to camera id.")]
    SubscribeToCamera(CameraId),
    #[command(description = "unsubscribe from camera id.")]
    UnsubscribeFromCamera(CameraId),
    #[command(description = "get snapshot of camera id.")]
    GetSnapshotOfCamera(CameraId),
    #[command(description = "set polling time.")]
    SetPollingTime(u64),
}

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
                .filter_command::<Command>()
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

    pub async fn send_message_with_picture(
        &self,
        message: String,
        picture: Vec<u8>,
        subscribers: Vec<ChatId>,
    ) {
        for chat_id in subscribers {
            let file = InputFile::memory(picture.clone()).file_name("new_file.jpg");
            if let Err(err) = self
                .client
                .send_photo(chat_id, file)
                .caption(message.clone())
                .await
            {
                println!("cannot send picture to Telegram {:?}", err)
            }
        }
    }
}

async fn command_handler(
    bot: Bot,
    msg: Message,
    allowed_chat_ids: Vec<String>,
    repository: Arc<MemoryRepository>,
    telegram_bot: TelegramBot,
    cmd: Command,
) -> ResponseResult<()> {
    if !allowed_chat_ids.contains(&format!("{}", msg.chat.id)) {
        error!(
            "not allowed chat id:{:?} allowed:{:?}",
            msg.chat.id, allowed_chat_ids
        );
    }

    match cmd {
        Command::Help => {
            info!("command Help - chat_id:{}", msg.chat.id);
            bot.send_message(msg.chat.id, Command::descriptions().to_string())
                .await?;
        }
        Command::GetCameras => {
            info!("command GetCameras - chat_id:{}", msg.chat.id);
            if let Err(err) = repository.update_cameras(&camera_discovery().await).await {
                error!("cannot update cameras: {}", err);
            }
            let cameras = repository.get_cameras().await;
            bot.send_message(msg.chat.id, "Available cameras are:".to_string())
                .await?;
            for camera in cameras {
                bot.send_message(msg.chat.id, camera.to_string()).await?;
            }
        }
        Command::SubscribeToCamera(camera_id) => {
            info!(
                "command SubscribeToCamera - chat id:{} camera_id:{}",
                msg.chat.id, camera_id
            );
            let _ = match repository
                .subscribe_to_camera(camera_id, msg.chat.id, true)
                .await
            {
                Ok(_) => {
                    bot.send_message(msg.chat.id, "Subscribed successfully".to_string())
                        .await
                }
                Err(err) => bot.send_message(msg.chat.id, format!("{}", err)).await,
            };
        }
        Command::UnsubscribeFromCamera(camera_id) => {
            info!(
                "command UnsubscribeFromCamera - chat id:{} camera_id:{}",
                msg.chat.id, camera_id
            );
            let _ = match repository
                .unsubscribe_from_camera(camera_id, msg.chat.id, true)
                .await
            {
                Ok(_) => {
                    bot.send_message(msg.chat.id, "Unsubscribed successfully".to_string())
                        .await
                }
                Err(err) => bot.send_message(msg.chat.id, format!("{}", err)).await,
            };
        }
        Command::GetSnapshotOfCamera(camera_id) => {
            info!(
                "command GetSnapshotOfCamera - chat id:{} camera_id:{}",
                msg.chat.id, camera_id
            );
            let camera = match repository.get_camera(camera_id).await {
                Some(camera) => camera,
                None => {
                    error!("cannot find camera with id: {}", camera_id);
                    return Ok(()); // TODO return error
                }
            };
            let snapshot = match camera.client.get_snapshot().await {
                Ok(snapshot) => snapshot,
                Err(err) => {
                    error!("error getting snapshot: {}", err);
                    return Ok(()); // TODO return error
                }
            };
            telegram_bot
                .send_message_with_picture(
                    make_caption("Snapshot", &camera.name, &chrono::Utc::now()),
                    snapshot.clone(),
                    vec![msg.chat.id],
                )
                .await;
        }
        Command::SetPollingTime(seconds) => {
            info!(
                "command SetPollingTime - chat id:{} seconds:{}",
                msg.chat.id, seconds
            );
            repository.set_polling_seconds(seconds).await;
            let _ = bot
                .send_message(msg.chat.id, "Polling time updated successfully".to_string())
                .await;
        }
    };

    Ok(())
}

pub fn make_caption(title: &str, name: &str, time: &chrono::DateTime<chrono::Utc>) -> String {
    let converted: chrono::DateTime<chrono::Local> = chrono::DateTime::from(*time);
    println!("utc:{} local:{}", time, converted);
    format!(
        r#"
{}
Camera:{}
Time: {}"#,
        title.to_uppercase(),
        name,
        converted
    )
}
