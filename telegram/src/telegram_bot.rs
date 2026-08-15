use app_core::{
    domain::event_bus::EventBus,
    make_caption,
    traits::{command_processor::CommandProcessor, notifier::Notifier},
    CameraId,
};
use chrono::{DateTime, Utc};
use std::sync::Arc;
use teloxide::{
    dispatching::{Dispatcher, HandlerExt, UpdateFilterExt},
    dptree,
    payloads::{EditMessageReplyMarkupSetters, SendMessageSetters},
    prelude::Requester,
    types::{
        CallbackQuery, InlineKeyboardButton, InlineKeyboardMarkup, Message, ParseMode, Update,
    },
    utils::command::BotCommands,
    Bot,
};
use tracing::error;

#[derive(BotCommands, Clone)]
#[command(rename_rule = "lowercase")]
pub enum BotCommand {
    // Help
    Help,
    // Cameras
    GetCameras,
    AddCamera(String),
    DeleteCamera(CameraId),
    SetCameraName(String),
    SetCredentials(String),
    FixSnapshot(CameraId),
    // Events
    Subscribe(CameraId),
    Unsubscribe(CameraId),
    GetSnapshot(String),
    // TODO GetSnapshotEvery(CameraId, String), // get snapshot of camera id every time period. params: camera_id, time (30s, 1m, ...)
    SetPollingTime(u64),
    SetBetweenTime(u64),
    DailyReport(bool),
}

#[derive(Clone)]
pub struct TelegramBot {
    client: Bot,
    allowed_chat_ids: Vec<String>,
    command_processor: Arc<dyn CommandProcessor>,
    notifier: Arc<dyn Notifier>,
    event_bus: Arc<EventBus>,
}

impl TelegramBot {
    pub fn new(
        bot_token: String,
        chat_ids: Vec<String>,
        command_processor: Arc<dyn CommandProcessor>,
        notifier: Arc<dyn Notifier>,
        event_bus: Arc<EventBus>,
    ) -> Self {
        Self {
            client: Bot::new(bot_token),
            allowed_chat_ids: chat_ids,
            command_processor,
            notifier,
            event_bus,
        }
    }

    pub async fn start(&self) {
        let handler = dptree::entry()
            .branch(
                Update::filter_message()
                    .branch(
                        dptree::entry()
                            .filter_command::<BotCommand>()
                            .endpoint(process_command),
                    )
                    .branch(
                        dptree::filter(|msg: Message| {
                            msg.text().map(|t| t.starts_with('/')).unwrap_or(false)
                        })
                        .endpoint(unknown_command),
                    ),
            )
            .branch(Update::filter_callback_query().endpoint(process_callback));

        self.event_listener();

        Dispatcher::builder(self.client.clone(), handler)
            .dependencies(dptree::deps![
                self.allowed_chat_ids.clone(),
                self.command_processor.clone(),
                self.clone()
            ])
            .build()
            .dispatch()
            .await;
    }

    fn event_listener(&self) {
        let mut rx = self.event_bus.subscribe();

        let notifier = self.notifier.clone();
        tokio::spawn(async move {
            while let Ok(event) = rx.recv().await {
                notifier
                    .send_text_with_picture_message(
                        make_caption(
                            "New Detection",
                            &event.camera.name,
                            &event.timestamp,
                            Some(event.r#type),
                            &event.objects,
                        ),
                        event.snapshot.clone(),
                        event.camera.subscriptors.clone(),
                        event.camera.id,
                        &event.timestamp,
                    )
                    .await;
            }
        });
    }
}

async fn process_command(
    bot: Bot,
    msg: teloxide::types::Message,
    allowed_chat_ids: Vec<String>,
    command_processor: Arc<dyn CommandProcessor>,
    cmd: BotCommand,
) -> teloxide::requests::ResponseResult<()> {
    let chat_id = msg.chat.id.0;
    if !allowed_chat_ids.contains(&format!("{chat_id}")) {
        let error_msg = format!(
            "not allowed chat id:{:?} allowed:{:?}",
            chat_id, allowed_chat_ids
        );
        error!("{error_msg}");
        return Err(anyhow_to_response_error(anyhow::anyhow!(error_msg)));
    }
    match cmd {
        BotCommand::Help => {
            bot.send_message(msg.chat.id, help_text())
                .parse_mode(ParseMode::MarkdownV2)
                .await
                .map_err(|err| anyhow_to_response_error(anyhow::anyhow!(err.to_string())))?;
        }
        BotCommand::GetCameras => command_processor
            .get_cameras_cmd(chat_id)
            .await
            .map_err(anyhow_to_response_error)?,
        BotCommand::SetCameraName(camera_id_and_name) => {
            let mut parts = camera_id_and_name.split_whitespace();
            let camera_id = match parts.next() {
                Some(id) => match id.parse::<CameraId>() {
                    Ok(parsed_id) => parsed_id,
                    Err(_) => {
                        return Err(string_to_response_error(
                            "cannot parse camera id".to_string(),
                        ))
                    }
                },
                None => {
                    return Err(string_to_response_error(
                        "cannot parse camera id".to_string(),
                    ))
                }
            };
            let camera_name = match parts.next() {
                Some(name) => name,
                None => {
                    return Err(string_to_response_error(
                        "cannot parse camera name".to_string(),
                    ))
                }
            };

            command_processor
                .set_camera_name_cmd(chat_id, camera_id, camera_name)
                .await
                .map_err(anyhow_to_response_error)?
        }
        BotCommand::Subscribe(camera_id) => command_processor
            .subscribe_cmd(chat_id, camera_id)
            .await
            .map_err(anyhow_to_response_error)?,
        BotCommand::Unsubscribe(camera_id) => command_processor
            .unsubscribe_cmd(chat_id, camera_id)
            .await
            .map_err(anyhow_to_response_error)?,
        BotCommand::GetSnapshot(camera_id) => {
            let camera_id = camera_id.parse::<CameraId>().ok();

            command_processor
                .get_snapshot_cmd(chat_id, camera_id)
                .await
                .map_err(anyhow_to_response_error)?
        }
        BotCommand::SetPollingTime(seconds) => command_processor
            .set_polling_time_cmd(chat_id, seconds)
            .await
            .map_err(anyhow_to_response_error)?,
        BotCommand::SetBetweenTime(seconds) => command_processor
            .set_between_time_cmd(chat_id, seconds)
            .await
            .map_err(anyhow_to_response_error)?,
        BotCommand::FixSnapshot(camera_id) => command_processor
            .fix_snapshot_uri_cmd(chat_id, camera_id)
            .await
            .map_err(anyhow_to_response_error)?,
        BotCommand::DailyReport(bool) => command_processor
            .enable_daily_report_cmd(chat_id, bool)
            .await
            .map_err(anyhow_to_response_error)?,
        BotCommand::AddCamera(params) => {
            let mut parts = params.split_whitespace();
            let uri = match parts.next() {
                Some(uri) => uri,
                None => {
                    return Err(string_to_response_error(
                        "usage: /addcamera uri [username] [password]".to_string(),
                    ))
                }
            };
            let username = parts.next().unwrap_or("");
            let password = parts.next().unwrap_or("");

            command_processor
                .add_camera_cmd(chat_id, uri, username, password)
                .await
                .map_err(anyhow_to_response_error)?
        }
        BotCommand::DeleteCamera(camera_id) => command_processor
            .delete_camera_cmd(chat_id, camera_id)
            .await
            .map_err(anyhow_to_response_error)?,
        BotCommand::SetCredentials(params) => {
            let mut parts = params.split_whitespace();
            let (camera_id, username, password) = match (parts.next(), parts.next(), parts.next()) {
                (Some(id), Some(user), Some(pass)) => {
                    let id: CameraId = match id.parse() {
                        Ok(id) => id,
                        Err(_) => {
                            return Err(string_to_response_error(
                                "usage: /setcredentials camera_id username password".to_string(),
                            ))
                        }
                    };
                    (id, user, pass)
                }
                _ => {
                    return Err(string_to_response_error(
                        "usage: /setcredentials camera_id username password".to_string(),
                    ))
                }
            };
            command_processor
                .set_credentials_cmd(chat_id, camera_id, username, password)
                .await
                .map_err(anyhow_to_response_error)?
        }
    };
    Ok(())
}

async fn process_callback(
    bot: Bot,
    callback: CallbackQuery,
    allowed_chat_ids: Vec<String>,
    command_processor: Arc<dyn CommandProcessor>,
) -> teloxide::requests::ResponseResult<()> {
    let Some(message) = &callback.message else {
        error!("Recordings not found");
        return Err(anyhow_to_response_error(anyhow::anyhow!(
            "Recordings not found"
        )));
    };

    let chat_id = message.chat().id.0;
    if !allowed_chat_ids.contains(&format!("{chat_id}")) {
        let error_msg = format!(
            "not allowed chat id:{:?} allowed:{:?}",
            chat_id, allowed_chat_ids
        );
        error!("{error_msg}");
        return Err(anyhow_to_response_error(anyhow::anyhow!(error_msg)));
    }

    let teloxide_chat_id = message.chat().id;
    let message_id = message.id();

    let Some(data) = &callback.data else {
        error!("Recordings not found");
        return Err(anyhow_to_response_error(anyhow::anyhow!(
            "Recordings not found"
        )));
    };

    let Some(data) = data.strip_prefix("recording|") else {
        bot.answer_callback_query(callback.id).await?;
        return Ok(());
    };

    let (camera_id, time) = match data.split_once('|') {
        Some((cam, ts_str)) => {
            let camera_id: CameraId = cam.parse().unwrap_or_default();
            let time = match DateTime::parse_from_rfc3339(ts_str) {
                Ok(t) => t.with_timezone(&Utc),
                Err(e) => {
                    return Err(anyhow_to_response_error(anyhow::anyhow!(
                        "invalid timestamp '{ts_str}': {e}"
                    )))
                }
            };
            (camera_id, time)
        }
        None => {
            error!("Recordings not found. invalid parameters");
            return Err(anyhow_to_response_error(anyhow::anyhow!(
                "Recordings not found. invalid parameters"
            )));
        }
    };

    bot.answer_callback_query(callback.id).await?; // inmediate ACK
    edit_reply_button_message(
        &bot,
        "⏳ Downloading...".to_string(),
        teloxide_chat_id,
        message_id,
        None,
    )
    .await;

    let bot_clone = bot.clone();
    tokio::spawn(async move {
        match command_processor
            .download_recording(chat_id, message_id.0, camera_id, time)
            .await
        {
            Ok(_) => {
                edit_reply_button_message(
                    &bot_clone,
                    "✅ Downloaded".to_string(),
                    teloxide_chat_id,
                    message_id,
                    callback.data,
                )
                .await
            }
            Err(err) => {
                edit_reply_button_message(
                    &bot_clone,
                    format!("❌ {err}"),
                    teloxide_chat_id,
                    message_id,
                    callback.data,
                )
                .await;
                error!("Error getting recording: {err:#}");
            }
        }
    });

    Ok(())
}

async fn edit_reply_button_message(
    bot: &Bot,
    text: String,
    chat_id: teloxide::types::ChatId,
    message_id: teloxide::types::MessageId,
    callback_string: Option<String>,
) {
    let callback = callback_string.unwrap_or("noop".to_string());
    bot.edit_message_reply_markup(chat_id, message_id)
        .reply_markup(InlineKeyboardMarkup::new(vec![vec![
            InlineKeyboardButton::callback(text, callback),
        ]]))
        .await
        .ok();
}

fn help_text() -> String {
    "\
📋 *HELP*
/help \\- show this command list

📷 *CAMERAS*
/getcameras \\- list all cameras
/addcamera `uri` `username` `password` \\- add a camera manually e\\.g\\. http\\://192\\.168\\.1\\.50\\:8899 admin secret
/deletecamera `camera_id` \\- delete a camera
/setcameraname `camera_id name` \\- rename a camera
/setcredentials `camera_id username password` \\- update credentials
/fixsnapshot `camera_id` \\- fix or resolve snapshot URI

🔔 *EVENTS*
/subscribe `camera_id` \\- subscribe to camera notifications
/unsubscribe `camera_id` \\- unsubscribe from camera notifications
/getsnapshot `[camera_id]` \\- get snapshot of one or all cameras
/setpollingtime `seconds` \\- set detection polling interval
/setbetweentime `seconds` \\- set minimum time between notifications
/dailyreport `true|false` \\- enable or disable daily status report"
        .to_string()
}

async fn unknown_command(
    bot: Bot,
    msg: Message,
    allowed_chat_ids: Vec<String>,
) -> teloxide::requests::ResponseResult<()> {
    let chat_id = msg.chat.id;
    if !allowed_chat_ids.contains(&format!("{}", chat_id.0)) {
        return Ok(());
    }
    let text = msg.text().unwrap_or("");
    let command = text.split_whitespace().next().unwrap_or(text);
    bot.send_message(
        chat_id,
        format!("Unknown command: {command}\nUse /help to see available commands."),
    )
    .await?;
    Ok(())
}

fn anyhow_to_response_error(err: anyhow::Error) -> teloxide::RequestError {
    teloxide::RequestError::Api(teloxide::ApiError::Unknown(err.to_string()))
}

fn string_to_response_error(err: String) -> teloxide::RequestError {
    teloxide::RequestError::Api(teloxide::ApiError::Unknown(err))
}
