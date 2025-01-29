use super::telegram_bot::TelegramBot;
use crate::{
    onvif::{onvif_camera::download_picture, onvif_clients::camera_discovery},
    repository::memory_repository::MemoryRepository,
    utils::{create_onvif_user_and_fix_snapshot_uri, make_caption},
    CameraId,
};
use log::{error, info};
use std::sync::Arc;
use teloxide::{prelude::*, types::Message, utils::command::BotCommands, Bot};

#[derive(BotCommands, Clone)]
#[command(
    rename_rule = "lowercase",
    description = "These commands are supported:"
)]
pub enum BotCommand {
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
    // TODO
    // #[command(description = "get snapshot of camera id every time period. params: camera_id, time (30s, 1m, ...)")]
    // GetSnapshotEvery(CameraId, String),
    #[command(description = "set detection checker polling time in seconds.")]
    SetPollingTime(u64),
    #[command(description = "set time between notifications.")]
    SetBetweenTime(u64),
    #[command(description = "fix snapshot uri camera id. params: camera_id")]
    FixSnapshot(CameraId),
    #[command(description = "enable/disable daily report. params: true/false")]
    DailyReport(bool),
}

pub async fn command_handler(
    bot: Bot,
    msg: Message,
    allowed_chat_ids: Vec<String>,
    repository: Arc<MemoryRepository>,
    telegram_bot: TelegramBot,
    cmd: BotCommand,
) -> ResponseResult<()> {
    if !allowed_chat_ids.contains(&format!("{}", msg.chat.id)) {
        error!(
            "not allowed chat id:{:?} allowed:{:?}",
            msg.chat.id, allowed_chat_ids
        );
    }
    match cmd {
        BotCommand::Help => help_cmd(bot, msg.chat.id).await?,
        BotCommand::GetCameras => get_cameras_cmd(bot, msg.chat.id, repository).await?,
        BotCommand::Subscribe(camera_id) => {
            subscribe_cmd(bot, msg.chat.id, repository, camera_id).await?
        }
        BotCommand::Unsubscribe(camera_id) => {
            unsubscribe_cmd(bot, msg.chat.id, repository, camera_id).await?
        }
        BotCommand::GetSnapshot(camera_id) => {
            get_snapshot_cmd(bot, msg.chat.id, repository, telegram_bot, camera_id).await?
        }
        BotCommand::SetPollingTime(seconds) => {
            set_polling_time_cmd(bot, msg.chat.id, repository, seconds).await?
        }
        BotCommand::SetBetweenTime(seconds) => {
            set_between_time_cmd(bot, msg.chat.id, repository, seconds).await?
        }
        BotCommand::FixSnapshot(camera_id) => {
            fix_snapshot_uri_cmd(bot, msg.chat.id, repository, camera_id).await?
        }
        BotCommand::DailyReport(bool) => {
            enable_daily_report_cmd(bot, msg.chat.id, repository, bool).await?
        }
    };
    Ok(())
}

async fn help_cmd(bot: Bot, chat_id: ChatId) -> ResponseResult<()> {
    info!("command Help - chat_id:{}", chat_id);
    bot.send_message(chat_id, BotCommand::descriptions().to_string())
        .await?;
    Ok(())
}

async fn get_cameras_cmd(
    bot: Bot,
    chat_id: ChatId,
    repository: Arc<MemoryRepository>,
) -> ResponseResult<()> {
    info!("command GetCameras - chat_id:{}", chat_id);
    if let Err(err) = repository
        .update_repository_cameras(&camera_discovery().await)
        .await
    {
        error!("cannot update cameras: {}", err);
    }
    let mut cameras = repository.get_cameras().await;
    if cameras.is_empty() {
        bot.send_message(chat_id, "No available cameras".to_string())
            .await?;
    } else {
        bot.send_message(chat_id, "Available cameras are:".to_string())
            .await?;

        cameras.sort_by(|a, b| a.id.cmp(&b.id));
        for camera in cameras {
            bot.send_message(chat_id, camera.to_string()).await?;
        }
    }
    Ok(())
}

async fn subscribe_cmd(
    bot: Bot,
    chat_id: ChatId,
    repository: Arc<MemoryRepository>,
    camera_id: CameraId,
) -> ResponseResult<()> {
    info!(
        "command SubscribeToCamera - chat id:{} camera_id:{}",
        chat_id, camera_id
    );
    let _ = match repository
        .subscribe_to_camera(camera_id, chat_id, true)
        .await
    {
        Ok(_) => {
            bot.send_message(chat_id, "Subscribed successfully".to_string())
                .await
        }
        Err(err) => bot.send_message(chat_id, format!("{}", err)).await,
    };
    Ok(())
}

async fn unsubscribe_cmd(
    bot: Bot,
    chat_id: ChatId,
    repository: Arc<MemoryRepository>,
    camera_id: CameraId,
) -> ResponseResult<()> {
    info!(
        "command UnsubscribeFromCamera - chat id:{} camera_id:{}",
        chat_id, camera_id
    );
    let _ = match repository
        .unsubscribe_from_camera(camera_id, chat_id, true)
        .await
    {
        Ok(_) => {
            bot.send_message(chat_id, "Unsubscribed successfully".to_string())
                .await
        }
        Err(err) => bot.send_message(chat_id, format!("{}", err)).await,
    };
    Ok(())
}

async fn get_snapshot_cmd(
    bot: Bot,
    chat_id: ChatId,
    repository: Arc<MemoryRepository>,
    telegram_bot: TelegramBot,
    camera_id: CameraId,
) -> ResponseResult<()> {
    info!(
        "command GetSnapshotOfCamera - chat id:{} camera_id:{}",
        chat_id, camera_id
    );
    let camera = match repository.get_camera(camera_id).await {
        Some(camera) => camera,
        None => {
            let error = format!("cannot find camera with id: {}", camera_id);
            print_and_send_error(&bot, &error, chat_id).await;
            return ResponseResult::Err(teloxide::RequestError::Api(teloxide::ApiError::Unknown(
                error,
            )));
        }
    };

    if let Some(snapshot_uri) = camera.snapshot_uri {
        let snapshot = match download_picture(&snapshot_uri).await {
            Ok(snapshot) => snapshot,
            Err(err) => {
                let error = format!(
                    "error getting snapshot from url:{:?} err:{}",
                    snapshot_uri, err
                );
                print_and_send_error(&bot, &error, chat_id).await;
                return ResponseResult::Err(teloxide::RequestError::Api(
                    teloxide::ApiError::Unknown(error),
                ));
            }
        };
        _ = telegram_bot
            .send_picture(
                &make_caption("Snapshot", &camera.name, &chrono::Utc::now()),
                snapshot.clone(),
                chat_id,
            )
            .await;
    } else {
        print_and_send_error(
            &bot,
            &format!("no snapshot url specified for camera {}", camera.name),
            chat_id,
        )
        .await;
    }
    Ok(())
}

async fn set_polling_time_cmd(
    bot: Bot,
    chat_id: ChatId,
    repository: Arc<MemoryRepository>,
    seconds: u64,
) -> ResponseResult<()> {
    info!(
        "command SetPollingTime - chat id:{} seconds:{}",
        chat_id, seconds
    );
    repository.set_polling_seconds(seconds).await;
    let _ = bot
        .send_message(chat_id, "Polling time updated successfully".to_string())
        .await;
    Ok(())
}

async fn set_between_time_cmd(
    bot: Bot,
    chat_id: ChatId,
    repository: Arc<MemoryRepository>,
    seconds: u64,
) -> ResponseResult<()> {
    info!(
        "command SetBetweenTime - chat id:{} seconds:{}",
        chat_id, seconds
    );
    repository.set_between_seconds(seconds).await;
    let _ = bot
        .send_message(chat_id, "Between time updated successfully".to_string())
        .await;
    Ok(())
}

async fn fix_snapshot_uri_cmd(
    bot: Bot,
    chat_id: ChatId,
    repository: Arc<MemoryRepository>,
    camera_id: CameraId,
) -> ResponseResult<()> {
    info!(
        "command FixSnapshotCmd - chat id:{} camera_id:{}",
        chat_id, camera_id
    );
    let camera = match repository.get_camera(camera_id).await {
        Some(camera) => camera,
        None => {
            let error = format!("cannot find camera with id: {}", camera_id);
            print_and_send_error(&bot, &error, chat_id).await;
            return ResponseResult::Err(teloxide::RequestError::Api(teloxide::ApiError::Unknown(
                error,
            )));
        }
    };

    if let Some(snapshot_uri) = camera.snapshot_uri {
        match create_onvif_user_and_fix_snapshot_uri(&camera.client.uri, &snapshot_uri).await {
            Ok(fixed_uri) => {
                match repository
                    .update_snapshot_uri_from_camera(camera_id, &fixed_uri)
                    .await
                {
                    Ok(_) => {
                        let message = format!("camera snapshot uri fixed:{}", fixed_uri);
                        println!("{}", message);
                        let _ = bot.send_message(chat_id, message).await;
                    }
                    Err(err) => {
                        let error = format!("{}", err);
                        print_and_send_error(&bot, &error, chat_id).await;
                        return ResponseResult::Err(teloxide::RequestError::Api(
                            teloxide::ApiError::Unknown(error),
                        ));
                    }
                }
            }
            Err(err) => {
                let error = format!("{}", err);
                print_and_send_error(&bot, &error, chat_id).await;
                return ResponseResult::Err(teloxide::RequestError::Api(
                    teloxide::ApiError::Unknown(error),
                ));
            }
        }
    } else {
        let error = format!("camera {} does not have snapshot uri", camera.client.uri);
        print_and_send_error(&bot, &error, chat_id).await;
        return ResponseResult::Err(teloxide::RequestError::Api(teloxide::ApiError::Unknown(
            error,
        )));
    }
    Ok(())
}

async fn enable_daily_report_cmd(
    bot: Bot,
    chat_id: ChatId,
    repository: Arc<MemoryRepository>,
    enable: bool,
) -> ResponseResult<()> {
    if enable {
        let _ = bot
            .send_message(chat_id, "Subscribed to daily report".to_string())
            .await;
        repository.subscribe_to_daily_report(chat_id, true).await;
    } else {
        let _ = bot
            .send_message(chat_id, "Unsubscribed from daily report".to_string())
            .await;
        repository
            .unsubscribe_from_daily_report(chat_id, true)
            .await;
    }
    Ok(())
}

async fn print_and_send_error(bot: &Bot, error: &str, chat_id: ChatId) {
    error!("{}", error);
    let _ = bot.send_message(chat_id, error).await;
}
