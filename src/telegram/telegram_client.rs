use crate::{
    onvif::{
        onvif_camera::download_picture,
        onvif_clients::{
            camera_discovery, create_default_user, get_users, DEFAULT_PASSWORD, DEFAULT_USERNAME,
        },
    },
    repository::memory_repository::{Camera, MemoryRepository},
    CameraId,
};
use anyhow::bail;
use log::{error, info};
use std::sync::Arc;
use teloxide::{
    prelude::*,
    types::{InputFile, Message},
    utils::command::BotCommands,
    Bot,
};
use url::Url;

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
    #[command(description = "subscribe to camera id. params camera_id")]
    Subscribe(CameraId),
    #[command(description = "unsubscribe from camera id. params: camera_id")]
    Unsubscribe(CameraId),
    #[command(description = "get snapshot of camera id. params: camera_id")]
    GetSnapshot(CameraId),
    // #[command(description = "get snapshot of camera id every time period. params: camera_id, time (30s, 1m, ...)")]
    // GetSnapshotEvery(CameraId, String),
    #[command(description = "set polling time.")]
    SetPollingTime(u64),
    #[command(description = "fix snapshot uri camera id. params: camera_id")]
    FixSnapshot(CameraId),
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
        Command::Subscribe(camera_id) => {
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
        Command::Unsubscribe(camera_id) => {
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
        Command::GetSnapshot(camera_id) => {
            info!(
                "command GetSnapshotOfCamera - chat id:{} camera_id:{}",
                msg.chat.id, camera_id
            );
            let camera = match repository.get_camera(camera_id).await {
                Some(camera) => camera,
                None => {
                    let error = format!("cannot find camera with id: {}", camera_id);
                    error!("{}", error);
                    let _ = bot.send_message(msg.chat.id, error).await;
                    return Ok(()); // TODO return error
                }
            };

            if let Some(snapshot_uri) = camera.snapshot_uri {
                let snapshot = match download_picture(&snapshot_uri).await {
                    Ok(snapshot) => snapshot,
                    Err(err) => {
                        print_and_send_error(
                            &bot,
                            &format!(
                                "error getting snapshot from url:{:?} err:{}",
                                snapshot_uri, err
                            ),
                            msg.chat.id,
                        )
                        .await;
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
            } else {
                print_and_send_error(
                    &bot,
                    &format!("no snapshot url specified for camera {}", camera.name),
                    msg.chat.id,
                )
                .await;
            }
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
        Command::FixSnapshot(camera_id) => {
            let camera = match repository.get_camera(camera_id).await {
                Some(camera) => camera,
                None => {
                    print_and_send_error(
                        &bot,
                        &format!("cannot find camera with id: {}", camera_id),
                        msg.chat.id,
                    )
                    .await;
                    return Ok(()); // TODO return error
                }
            };
            match get_users(&camera.client.uri).await {
                Ok(users) => {
                    if !users.contains(&DEFAULT_USERNAME.to_string()) {
                        match create_default_user(&camera.client.uri).await {
                            Ok(_) => {
                                let message =
                                    format!("user {} created successfully", DEFAULT_USERNAME);
                                println!("{}", message);
                                let _ = bot.send_message(msg.chat.id, message).await;
                                if let Ok(fixed_uri) = fix_snapshot_uri(&camera, repository).await {
                                    let message =
                                        format!("camera snapshot uri fixed:{}", fixed_uri);
                                    println!("{}", message);
                                    let _ = bot.send_message(msg.chat.id, message).await;
                                }
                            }
                            Err(err) => {
                                let message = format!("cannot create user {}", DEFAULT_USERNAME);
                                error!("{}. error: {}", message, err);
                                let _ = bot.send_message(msg.chat.id, message).await;
                            }
                        }
                    } else if let Ok(fixed_uri) = fix_snapshot_uri(&camera, repository).await {
                        let message = format!("camera snapshot uri fixed:{}", fixed_uri);
                        println!("{}", message);
                        let _ = bot.send_message(msg.chat.id, message).await;
                    }
                }
                Err(_) => todo!(),
            };
        }
    };

    Ok(())
}

pub async fn print_and_send_error(bot: &Bot, error: &str, chat_id: ChatId) {
    error!("{}", error);
    let _ = bot.send_message(chat_id, error).await;
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

pub fn replace_snapshot_uri_credentials(
    snapshot_uri: &str,
    new_user: &str,
    new_password: &str,
) -> String {
    // Case 1 - for URLS with this format
    // http://192.168.1.217/webcapture.jpg?command=snap&channel=0&user=yfyf&password=aZlg5hk1
    let mut url = Url::parse(snapshot_uri).unwrap();
    let mut query_pairs: Vec<(String, String)> = url
        .query_pairs()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
    for pair in query_pairs.iter_mut() {
        match pair.0.as_str() {
            "user" => pair.1 = new_user.to_string(),
            "password" => pair.1 = new_password.to_string(),
            _ => {}
        }
    }
    let new_query = url::form_urlencoded::Serializer::new(String::new())
        .extend_pairs(query_pairs)
        .finish();
    url.set_query(Some(&new_query));
    url.to_string()
}

pub async fn fix_snapshot_uri(
    camera: &Camera,
    repository: Arc<MemoryRepository>,
) -> anyhow::Result<String> {
    if let Some(ref snapshot_uri) = camera.snapshot_uri {
        let fixed_snapshot_uri =
            replace_snapshot_uri_credentials(snapshot_uri, DEFAULT_USERNAME, DEFAULT_PASSWORD);
        // TODO save in memory_repository and db
        let _ = repository
            .update_camera_snapshot_uri(camera.id, &fixed_snapshot_uri)
            .await;
        Ok(fixed_snapshot_uri)
    } else {
        bail!("camera {} doesnt have snapshot uri", camera.name)
    }
}
