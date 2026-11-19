use app_core::{
    domain::{event_bus::EventBus, object::string_to_object_classes},
    make_caption, make_error,
    traits::{command_processor::CommandProcessor, notifier::Notifier},
    CameraId,
};
use chrono::{DateTime, Datelike, Duration, TimeZone, Utc};
use repository::memory_repository::MemoryRepository;
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
    GetCameras(String),
    AddCamera(String),
    DeleteCamera(CameraId),
    SetCameraName(String),
    SetCredentials(String),
    FixSnapshot(CameraId),
    SetZoom(String),
    // Events
    Subscribe(CameraId),
    Unsubscribe(CameraId),
    GetSnapshot(String),
    Detect(String),
    ManualDetection(String),
    DailyReport(bool),
    // Recordings
    GetRecordingSummary(String),
    GetRecordingList(String),
    DownloadRecording(String),
    // TODO GetSnapshotEvery(CameraId, String), // get snapshot of camera id every time period. params: camera_id, time (30s, 1m, ...)
    // Config
    GetConfig,
    ResetConfig,
    ConfigPollingTime(u64),
    ConfigBetweenTime(u64),
    ConfigSendErrors(bool),
    ConfigAutoRenewal(bool),
    ConfigRecordingClip(u64),
    ConfigDetectorEnable(bool),
    ConfigDetectorMinConfidence(f32),
    ConfigDetectorTypes(String),
    ConfigSnapshot(String),
    ConfigMultiChannel(String),
}

#[derive(Clone)]
pub struct TelegramBot {
    client: Bot,
    allowed_chat_ids: Vec<String>,
    command_processor: Arc<dyn CommandProcessor>,
    notifier: Arc<dyn Notifier>,
    event_bus: Arc<EventBus>,
    repository: Arc<MemoryRepository>,
}

impl TelegramBot {
    pub fn new(
        bot_token: String,
        chat_ids: Vec<String>,
        command_processor: Arc<dyn CommandProcessor>,
        notifier: Arc<dyn Notifier>,
        event_bus: Arc<EventBus>,
        repository: Arc<MemoryRepository>,
    ) -> Self {
        Self {
            client: Bot::new(bot_token),
            allowed_chat_ids: chat_ids,
            command_processor,
            notifier,
            event_bus,
            repository,
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
                self.repository.clone(),
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
                match event {
                    app_core::domain::event_bus::EventBusMessage::CameraEvent(camera_event) => {
                        let caption = make_caption(
                            "New Detection",
                            &camera_event.camera.name,
                            &camera_event.camera.id,
                            &camera_event.timestamp,
                            Some(camera_event.r#type),
                            &camera_event.objects,
                        );
                        if camera_event.bypass_notification_throttle {
                            for chat_id in camera_event.camera.subscriptors.clone() {
                                let _ = notifier
                                    .send_picture_message(
                                        &caption,
                                        camera_event.snapshot.clone(),
                                        chat_id,
                                        camera_event.camera.id,
                                        &camera_event.timestamp,
                                    )
                                    .await;
                            }
                        } else {
                            notifier
                                .send_text_with_picture_message(
                                    caption,
                                    camera_event.snapshot.clone(),
                                    camera_event.camera.subscriptors.clone(),
                                    camera_event.camera.id,
                                    &camera_event.timestamp,
                                )
                                .await;
                        }
                    }
                    app_core::domain::event_bus::EventBusMessage::Error(error_message) => {
                        let title = if error_message.recovered {
                            "ERROR RECOVERED"
                        } else {
                            "ERROR"
                        };
                        notifier
                            .send_text_message(
                                make_error(
                                    title,
                                    &error_message.message,
                                    &error_message.camera.name,
                                    &error_message.camera.id,
                                    &error_message.timestamp,
                                ),
                                error_message.camera.subscriptors.clone(),
                            )
                            .await;
                    }
                }
            }
        });
    }
}

async fn process_command(
    bot: Bot,
    msg: teloxide::types::Message,
    allowed_chat_ids: Vec<String>,
    command_processor: Arc<dyn CommandProcessor>,
    _repository: Arc<MemoryRepository>,
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
        BotCommand::GetCameras(value) => {
            let full = value.to_ascii_lowercase().trim() == "extended";
            command_processor
                .get_cameras_cmd(chat_id, full)
                .await
                .map_err(anyhow_to_response_error)?
        }
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
        BotCommand::ConfigPollingTime(seconds) => command_processor
            .config_polling_time_cmd(chat_id, seconds)
            .await
            .map_err(anyhow_to_response_error)?,
        BotCommand::ConfigBetweenTime(seconds) => command_processor
            .config_between_time_cmd(chat_id, seconds)
            .await
            .map_err(anyhow_to_response_error)?,
        BotCommand::ConfigSendErrors(send_errors) => command_processor
            .config_send_errors_cmd(chat_id, send_errors)
            .await
            .map_err(anyhow_to_response_error)?,
        BotCommand::ConfigAutoRenewal(enable) => command_processor
            .config_auto_renewal_cmd(chat_id, enable)
            .await
            .map_err(anyhow_to_response_error)?,
        BotCommand::ConfigRecordingClip(seconds) => command_processor
            .config_recording_clip_cmd(chat_id, seconds)
            .await
            .map_err(anyhow_to_response_error)?,
        BotCommand::ConfigDetectorEnable(enable) => command_processor
            .config_detector_enable_cmd(chat_id, enable)
            .await
            .map_err(anyhow_to_response_error)?,
        BotCommand::ConfigDetectorMinConfidence(min_confidence) => command_processor
            .config_detector_min_confidence_cmd(chat_id, min_confidence)
            .await
            .map_err(anyhow_to_response_error)?,
        BotCommand::ConfigDetectorTypes(types) => command_processor
            .config_detector_types_cmd(chat_id, string_to_object_classes(&types))
            .await
            .map_err(anyhow_to_response_error)?,
        BotCommand::GetConfig => command_processor.get_config(chat_id).await,
        BotCommand::ResetConfig => command_processor.reset_config(chat_id).await,
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
        BotCommand::ConfigSnapshot(params) => {
            let mut parts = params.split_whitespace();
            let (camera_id, method) = match (parts.next(), parts.next()) {
                (Some(id), Some(method)) => {
                    let id: CameraId = match id.parse() {
                        Ok(id) => id,
                        Err(_) => {
                            return Err(string_to_response_error(
                                "usage: /configsnapshot camera_id api|rtsp".to_string(),
                            ))
                        }
                    };
                    (id, method)
                }
                _ => {
                    return Err(string_to_response_error(
                        "usage: /configsnapshot camera_id api|rtsp".to_string(),
                    ))
                }
            };
            command_processor
                .set_snapshot_method_cmd(chat_id, camera_id, method)
                .await
                .map_err(anyhow_to_response_error)?
        }
        BotCommand::ConfigMultiChannel(params) => {
            let mut parts = params.split_whitespace();
            let (camera_id, enable) = match (parts.next(), parts.next()) {
                (Some(id), Some(enable)) => {
                    let id: CameraId = match id.parse() {
                        Ok(id) => id,
                        Err(_) => {
                            return Err(string_to_response_error(
                                "usage: /configmultichannel camera_id true|false".to_string(),
                            ))
                        }
                    };
                    let enable: bool = match enable.parse() {
                        Ok(enable) => enable,
                        Err(_) => {
                            return Err(string_to_response_error(
                                "usage: /configmultichannel camera_id true|false".to_string(),
                            ))
                        }
                    };
                    (id, enable)
                }
                _ => {
                    return Err(string_to_response_error(
                        "usage: /configmultichannel camera_id true|false".to_string(),
                    ))
                }
            };
            command_processor
                .set_multi_channel_detection_cmd(chat_id, camera_id, enable)
                .await
                .map_err(anyhow_to_response_error)?
        }
        BotCommand::SetZoom(params) => {
            let mut parts = params.split_whitespace();
            let (camera_id, zoom_level) = match (parts.next(), parts.next()) {
                (Some(id), Some(zoom_level)) => {
                    let id: CameraId = match id.parse() {
                        Ok(id) => id,
                        Err(_) => {
                            return Err(string_to_response_error(
                                "usage: /setzoom camera_id 1-100".to_string(),
                            ))
                        }
                    };
                    let zoom_level: u8 = match zoom_level.parse() {
                        Ok(zoom_level) => zoom_level,
                        Err(_) => {
                            return Err(string_to_response_error(
                                "usage: /setzoom camera_id 1-100".to_string(),
                            ))
                        }
                    };
                    (id, zoom_level)
                }
                _ => {
                    return Err(string_to_response_error(
                        "usage: /setzoom camera_id 1-100".to_string(),
                    ))
                }
            };
            command_processor
                .set_camera_zoom(chat_id, camera_id, zoom_level)
                .await
                .map_err(anyhow_to_response_error)?
        }
        BotCommand::Detect(camera_id) => {
            let camera_id = camera_id.trim();
            let camera_id = if camera_id.is_empty() || camera_id.eq_ignore_ascii_case("all") {
                None
            } else {
                match camera_id.parse::<CameraId>() {
                    Ok(id) => Some(id),
                    Err(_) => {
                        return Err(string_to_response_error(
                            "usage: /detect [camera_id]".to_string(),
                        ));
                    }
                }
            };

            command_processor
                .manual_detection_cmd(chat_id, camera_id)
                .await
                .map_err(anyhow_to_response_error)?
        }
        BotCommand::ManualDetection(params) => {
            let mut parts = params.split_whitespace();
            match parts.next() {
                Some(subcmd) if subcmd.eq_ignore_ascii_case("enable") => {
                    let mut interval = None;
                    let mut types_vec = Vec::new();
                    for token in parts {
                        if interval.is_none()
                            && (token.ends_with('s')
                                || token.ends_with('m')
                                || token.chars().all(|c| c.is_ascii_digit()))
                        {
                            interval = parse_duration_string(token);
                        } else {
                            types_vec.push(token);
                        }
                    }
                    let types = if types_vec.is_empty() {
                        None
                    } else {
                        Some(string_to_object_classes(&types_vec.join(",")))
                    };
                    command_processor
                        .config_manual_detection_cmd(chat_id, true, interval, types)
                        .await
                        .map_err(anyhow_to_response_error)?
                }
                Some(subcmd) if subcmd.eq_ignore_ascii_case("disable") => command_processor
                    .config_manual_detection_cmd(chat_id, false, None, None)
                    .await
                    .map_err(anyhow_to_response_error)?,
                _ => command_processor
                    .get_manual_detection_status_cmd(chat_id)
                    .await
                    .map_err(anyhow_to_response_error)?,
            }
        }
        BotCommand::GetRecordingSummary(params) => {
            let mut parts = params.split_whitespace();
            let camera_id = match parts.next() {
                Some(id) => match id.parse::<CameraId>() {
                    Ok(id) => id,
                    Err(_) => {
                        return Err(string_to_response_error(
                            "usage: /getrecordingsummary camera_id [days]".to_string(),
                        ));
                    }
                },
                None => {
                    return Err(string_to_response_error(
                        "usage: /getrecordingsummary camera_id [days]".to_string(),
                    ));
                }
            };
            let days = parts.next().and_then(|d| d.parse::<u32>().ok());
            command_processor
                .get_recording_summary_cmd(chat_id, camera_id, days)
                .await
                .map_err(anyhow_to_response_error)?
        }
        BotCommand::GetRecordingList(params) => {
            let usage = "usage: /getrecordinglist camera_id date [clip_seconds]\n(date: today, yesterday, last monday, 30m ago, 2026-11-05, etc.)";
            let (camera_id, date, clip_seconds) = parse_recording_params(&params, usage)?;
            command_processor
                .get_recording_list_cmd(chat_id, camera_id, date, clip_seconds, 1)
                .await
                .map_err(anyhow_to_response_error)?
        }
        BotCommand::DownloadRecording(params) => {
            let usage = "usage: /downloadrecording camera_id date [clip_seconds]\n(date: today, yesterday, last monday, 30m ago, 2026-11-05, etc.)";
            let (camera_id, date, clip_seconds) = parse_recording_params(&params, usage)?;
            command_processor
                .download_recording_cmd(chat_id, camera_id, date, clip_seconds)
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
    repository: Arc<MemoryRepository>,
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

    let Some(raw_data) = &callback.data else {
        error!("Recordings not found");
        return Err(anyhow_to_response_error(anyhow::anyhow!(
            "Recordings not found"
        )));
    };

    if let Some(data) = raw_data.strip_prefix("recpage|") {
        let parts: Vec<&str> = data.split('|').collect();
        if let [cam, date_str, clip_str, page_str] = parts.as_slice() {
            let camera_id: CameraId = cam.parse().unwrap_or_default();
            let page: usize = page_str.parse().unwrap_or(1);
            let date = match DateTime::parse_from_rfc3339(date_str) {
                Ok(t) => t.with_timezone(&Utc),
                Err(e) => {
                    return Err(anyhow_to_response_error(anyhow::anyhow!(
                        "invalid timestamp '{date_str}': {e}"
                    )))
                }
            };
            let clip_seconds = if clip_str.is_empty() {
                None
            } else {
                clip_str.parse::<i64>().ok().map(Duration::seconds)
            };

            bot.answer_callback_query(callback.id).await?;
            let _ = command_processor
                .get_recording_list_cmd(chat_id, camera_id, date, clip_seconds, page)
                .await;
            return Ok(());
        }
    }

    let Some(data) = raw_data.strip_prefix("recording|") else {
        bot.answer_callback_query(callback.id).await?;
        return Ok(());
    };

    let parts: Vec<&str> = data.split('|').collect();
    let (camera_id, time, custom_clip) = match parts.as_slice() {
        [cam, ts_str] => {
            let camera_id: CameraId = cam.parse().unwrap_or_default();
            let time = match DateTime::parse_from_rfc3339(ts_str) {
                Ok(t) => t.with_timezone(&Utc),
                Err(e) => {
                    return Err(anyhow_to_response_error(anyhow::anyhow!(
                        "invalid timestamp '{ts_str}': {e}"
                    )))
                }
            };
            (camera_id, time, None)
        }
        [cam, ts_str, clip_str] => {
            let camera_id: CameraId = cam.parse().unwrap_or_default();
            let time = match DateTime::parse_from_rfc3339(ts_str) {
                Ok(t) => t.with_timezone(&Utc),
                Err(e) => {
                    return Err(anyhow_to_response_error(anyhow::anyhow!(
                        "invalid timestamp '{ts_str}': {e}"
                    )))
                }
            };
            let clip = clip_str.parse::<i64>().ok().map(Duration::seconds);
            (camera_id, time, clip)
        }
        _ => {
            error!("Recordings not found. invalid parameters");
            return Err(anyhow_to_response_error(anyhow::anyhow!(
                "Recordings not found. invalid parameters"
            )));
        }
    };

    let callback_data_str = raw_data.clone();

    bot.answer_callback_query(callback.id).await?; // immediate ACK

    // Extract current inline keyboard from the message
    let current_keyboard: Option<InlineKeyboardMarkup> = match message {
        teloxide::types::MaybeInaccessibleMessage::Regular(reg_msg) => {
            reg_msg.reply_markup().cloned()
        }
        teloxide::types::MaybeInaccessibleMessage::Inaccessible(_) => None,
    };

    // Update only the clicked button in the existing markup
    let initial_markup = update_button_in_markup(
        &current_keyboard,
        &callback_data_str,
        "⏳ Downloading...",
        Some("noop"),
    );
    if let Some(ref markup) = initial_markup {
        bot.edit_message_reply_markup(teloxide_chat_id, message_id)
            .reply_markup(markup.clone())
            .await
            .ok();
    }

    let bot_clone = bot.clone();
    let config = repository.get_config().await;
    let clip_to_download =
        custom_clip.unwrap_or_else(|| Duration::seconds(config.recording_clip as i64));

    tokio::spawn(async move {
        match command_processor
            .download_recording(chat_id, message_id.0, camera_id, time, clip_to_download)
            .await
        {
            Ok(_) => {
                if let Some(updated_markup) = update_button_in_markup(
                    &current_keyboard,
                    &callback_data_str,
                    "✅ Downloaded",
                    Some(&callback_data_str),
                ) {
                    bot_clone
                        .edit_message_reply_markup(teloxide_chat_id, message_id)
                        .reply_markup(updated_markup)
                        .await
                        .ok();
                }
            }
            Err(err) => {
                let err_label = format!("❌ {err}");
                if let Some(updated_markup) = update_button_in_markup(
                    &current_keyboard,
                    &callback_data_str,
                    &err_label,
                    Some(&callback_data_str),
                ) {
                    bot_clone
                        .edit_message_reply_markup(teloxide_chat_id, message_id)
                        .reply_markup(updated_markup)
                        .await
                        .ok();
                }
                error!("Error getting recording: {err:#}");
            }
        }
    });

    Ok(())
}

fn update_button_in_markup(
    existing_markup: &Option<InlineKeyboardMarkup>,
    target_callback: &str,
    new_text: &str,
    new_callback: Option<&str>,
) -> Option<InlineKeyboardMarkup> {
    let markup = existing_markup.as_ref()?;
    let mut new_keyboard: Vec<Vec<InlineKeyboardButton>> = Vec::new();

    for row in &markup.inline_keyboard {
        let mut new_row: Vec<InlineKeyboardButton> = Vec::new();
        for btn in row {
            let mut btn_clone = btn.clone();
            if let teloxide::types::InlineKeyboardButtonKind::CallbackData(ref cb) = btn.kind {
                if cb == target_callback {
                    btn_clone.text = new_text.to_string();
                    if let Some(ncb) = new_callback {
                        btn_clone.kind = teloxide::types::InlineKeyboardButtonKind::CallbackData(
                            ncb.to_string(),
                        );
                    }
                }
            }
            new_row.push(btn_clone);
        }
        new_keyboard.push(new_row);
    }

    Some(InlineKeyboardMarkup::new(new_keyboard))
}

fn help_text() -> String {
    "\
📋 *HELP*
/help \\- show this command list

📷 *CAMERAS*
/getcameras \\- list all cameras
/getcameras extended \\- list extended info of cameras
/addcamera `uri` `username` `password` \\- add a camera manually e\\.g\\. http\\://192\\.168\\.1\\.50\\:8899 admin secret
/deletecamera `camera_id` \\- delete a camera
/setcameraname `camera_id name` \\- rename a camera
/setcredentials `camera_id username password` \\- update credentials
/setzoom `camera_id zoom`, \\- zoom camera \\(range 1\\-100\\)
/fixsnapshot `camera_id` \\- fix or resolve snapshot URI

🔔 *EVENTS*
/subscribe `camera_id` \\- subscribe to camera notifications
/unsubscribe `camera_id` \\- unsubscribe from camera notifications
/getsnapshot `camera_id` \\- get snapshot of one or all cameras
/detect `camera_id` \\- run instant manual object detection
/manualdetection `enable [interval] [types...]|disable` \\- periodic background visual scan
/dailyreport `true|false` \\- enable or disable daily status report

📹 *RECORDINGS*
/getrecordingsummary `camera_id [days]` \\- fast summary of recordings \\(dates & total\\)
/getrecordinglist `camera_id date [seconds]` \\- list recordings \\(e\\.g\\. today, yesterday, last monday, 30m ago\\)
/downloadrecording `camera_id date [seconds]` \\- download recording clip at date \\(e\\.g\\. today, yesterday, last monday\\)

🌣 *CONFIG*
/getconfig \\- shows current config
/resetconfig \\- restore default config yaml
/configpollingtime `seconds` \\- set detection polling interval
/configbetweentime `seconds` \\- set minimum time between notifications
/configsenderrors `true|false` \\- enable or disable sync errors reception
/configautorenewal `true|false` \\- enable or disable onvif sessions renewal
/configrecordingclip `seconds` \\- recording clip seconds to download
/configdetectorenable `true|false` \\- enable or disable detector
/configdetectorminconfidence `0-1` \\- set detector min confidence
/configdetectortypes `person,cat,...` \\- set detector types
/configmultichannel `camera_id true|false` \\- detect on every extra rtsp channel of this camera
/configsnapshot `camera_id api|rtsp` \\- camera api default, rtsp if api fails
"
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

fn parse_recording_params(
    params: &str,
    usage: &str,
) -> Result<(CameraId, DateTime<Utc>, Option<Duration>), teloxide::RequestError> {
    let tokens: Vec<&str> = params.split_whitespace().collect();
    if tokens.is_empty() {
        return Err(string_to_response_error(usage.to_string()));
    }
    let id_str = tokens[0];
    let camera_id: CameraId = id_str
        .parse()
        .map_err(|_| string_to_response_error(format!("invalid camera id '{id_str}'\n{usage}")))?;

    if tokens.len() < 2 {
        return Err(string_to_response_error(usage.to_string()));
    }

    let rest = &tokens[1..];

    // If there are at least 2 tokens in rest, check if the last token is a clip duration
    if rest.len() >= 2 {
        let last_token = rest[rest.len() - 1];
        if let Some(clip) = parse_clip_duration_token(last_token) {
            let date_candidate = rest[..rest.len() - 1].join(" ");
            if let Ok(dt) = parse_datetime_query(&date_candidate) {
                return Ok((camera_id, dt, Some(clip)));
            }
        }
    }

    // Otherwise, parse all tokens in rest as the date query (no clip)
    let date_str = rest.join(" ");
    let dt = parse_datetime_query(&date_str)
        .map_err(|e| string_to_response_error(format!("{e}\n{usage}")))?;

    Ok((camera_id, dt, None))
}

fn parse_clip_duration_token(s: &str) -> Option<Duration> {
    let s = s.trim();
    if let Some(rest) = s.strip_suffix('s').or_else(|| s.strip_suffix('S')) {
        rest.parse::<i64>().ok().map(Duration::seconds)
    } else if let Some(rest) = s.strip_suffix('m').or_else(|| s.strip_suffix('M')) {
        rest.parse::<i64>().ok().map(|m| Duration::seconds(m * 60))
    } else if s.chars().all(|c| c.is_ascii_digit()) && !s.is_empty() {
        s.parse::<i64>().ok().map(Duration::seconds)
    } else {
        None
    }
}

pub fn parse_datetime_query(s: &str) -> anyhow::Result<DateTime<Utc>> {
    if let Ok(dt) = parse_iso8601_datetime(s) {
        return Ok(dt);
    }

    let s_lower = s.trim().to_lowercase();
    if s_lower == "now" {
        return Ok(Utc::now());
    }

    // relative "X ago" (e.g. "30m ago", "2 hours ago")
    if let Some(rest) = s_lower.strip_suffix("ago") {
        let rest = rest.trim();
        if let Some(dur) = parse_relative_duration(rest) {
            return Ok(Utc::now() - dur);
        }
    }

    // rRelative "last X" (e.g. "last 30m", "last 2 hours")
    if let Some(rest) = s_lower.strip_prefix("last ") {
        if let Some(dur) = parse_relative_duration(rest) {
            return Ok(Utc::now() - dur);
        }
    }

    // day keywords (today, yesterday, last monday, etc.) + optional time
    let tokens: Vec<&str> = s_lower.split_whitespace().collect();
    if let Some(dt) = parse_day_and_time(&tokens) {
        return Ok(dt);
    }

    anyhow::bail!(
        "Cannot parse date '{s}'. Expected ISO8601 (e.g. 2026-11-05T15:30:00Z, 2026-11-05) or keyword (e.g. today, yesterday, last monday, 30m ago)"
    )
}

fn parse_iso8601_datetime(s: &str) -> anyhow::Result<DateTime<Utc>> {
    if let Ok(dt) = DateTime::parse_from_rfc3339(s) {
        return Ok(dt.with_timezone(&Utc));
    }
    if let Ok(naive) = chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S") {
        return Ok(DateTime::<Utc>::from_naive_utc_and_offset(naive, Utc));
    }
    if let Ok(naive) = chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%d_%H:%M:%S") {
        return Ok(DateTime::<Utc>::from_naive_utc_and_offset(naive, Utc));
    }
    if let Ok(naive) = chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S") {
        return Ok(DateTime::<Utc>::from_naive_utc_and_offset(naive, Utc));
    }
    if let Ok(naive) = chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M") {
        return Ok(DateTime::<Utc>::from_naive_utc_and_offset(naive, Utc));
    }
    if let Ok(date) = chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d") {
        if let Some(naive) = date.and_hms_opt(0, 0, 0) {
            return Ok(DateTime::<Utc>::from_naive_utc_and_offset(naive, Utc));
        }
    }
    anyhow::bail!("Expected ISO8601 format (e.g. 2026-11-05T15:30:00Z or 2026-11-05)")
}

fn parse_time_token(s: &str) -> Option<chrono::NaiveTime> {
    chrono::NaiveTime::parse_from_str(s, "%H:%M:%S")
        .or_else(|_| chrono::NaiveTime::parse_from_str(s, "%H:%M"))
        .ok()
}

fn parse_weekday(s: &str) -> Option<chrono::Weekday> {
    match s.trim().to_lowercase().as_str() {
        "monday" | "mon" | "lunes" => Some(chrono::Weekday::Mon),
        "tuesday" | "tue" | "martes" => Some(chrono::Weekday::Tue),
        "wednesday" | "wed" | "miercoles" | "miércoles" => Some(chrono::Weekday::Wed),
        "thursday" | "thu" | "jueves" => Some(chrono::Weekday::Thu),
        "friday" | "fri" | "viernes" => Some(chrono::Weekday::Fri),
        "saturday" | "sat" | "sabado" | "sábado" => Some(chrono::Weekday::Sat),
        "sunday" | "sun" | "domingo" => Some(chrono::Weekday::Sun),
        _ => None,
    }
}

fn parse_relative_duration(s: &str) -> Option<Duration> {
    let s = s.trim().to_lowercase();
    let (num_str, unit) = if let Some(idx) = s.find(|c: char| c.is_alphabetic()) {
        (&s[..idx], s[idx..].trim())
    } else {
        return None;
    };
    let num: i64 = num_str.trim().parse().ok()?;
    if num < 0 {
        return None;
    }

    match unit {
        "s" | "sec" | "secs" | "second" | "seconds" => Some(Duration::seconds(num)),
        "m" | "min" | "mins" | "minute" | "minutes" => Some(Duration::minutes(num)),
        "h" | "hr" | "hrs" | "hour" | "hours" => Some(Duration::hours(num)),
        "d" | "day" | "days" => Some(Duration::days(num)),
        _ => None,
    }
}

fn resolve_target_date(
    today: chrono::NaiveDate,
    weekday: chrono::Weekday,
    explicit_last: bool,
) -> chrono::NaiveDate {
    let cur_weekday = today.weekday();
    let cur_idx = cur_weekday.num_days_from_monday();
    let target_idx = weekday.num_days_from_monday();

    let days_ago = if explicit_last {
        if cur_idx > target_idx {
            cur_idx - target_idx
        } else {
            cur_idx + 7 - target_idx
        }
    } else {
        if cur_idx >= target_idx {
            cur_idx - target_idx
        } else {
            cur_idx + 7 - target_idx
        }
    };

    today - chrono::Duration::days(days_ago as i64)
}

fn combine_local_date_and_time(date: chrono::NaiveDate, time: chrono::NaiveTime) -> DateTime<Utc> {
    let naive_dt = date.and_time(time);
    match chrono::Local.from_local_datetime(&naive_dt) {
        chrono::LocalResult::Single(dt) => dt.with_timezone(&Utc),
        chrono::LocalResult::Ambiguous(dt1, _) => dt1.with_timezone(&Utc),
        chrono::LocalResult::None => DateTime::<Utc>::from_naive_utc_and_offset(naive_dt, Utc),
    }
}

fn parse_day_and_time(tokens: &[&str]) -> Option<DateTime<Utc>> {
    if tokens.is_empty() {
        return None;
    }

    let today = chrono::Local::now().date_naive();
    let default_time = chrono::NaiveTime::from_hms_opt(0, 0, 0)?;

    // 1 token: today, yesterday, monday, last_monday
    if tokens.len() == 1 {
        let t = tokens[0];
        if t == "today" || t == "hoy" {
            return Some(combine_local_date_and_time(today, default_time));
        }
        if t == "yesterday" || t == "ayer" {
            let yest = today - chrono::Duration::days(1);
            return Some(combine_local_date_and_time(yest, default_time));
        }
        if let Some(rest) = t.strip_prefix("last_") {
            if let Some(wd) = parse_weekday(rest) {
                let date = resolve_target_date(today, wd, true);
                return Some(combine_local_date_and_time(date, default_time));
            }
        }
        if let Some(wd) = parse_weekday(t) {
            let date = resolve_target_date(today, wd, false);
            return Some(combine_local_date_and_time(date, default_time));
        }
        return None;
    }

    // 2 tokens: "last monday", "today 14:30", "yesterday 20:00", "monday 10:00", etc.
    if tokens.len() == 2 {
        let t0 = tokens[0];
        let t1 = tokens[1];

        if t0 == "last" {
            if let Some(wd) = parse_weekday(t1) {
                let date = resolve_target_date(today, wd, true);
                return Some(combine_local_date_and_time(date, default_time));
            }
        }

        if let Some(time) = parse_time_token(t1) {
            if t0 == "today" || t0 == "hoy" {
                return Some(combine_local_date_and_time(today, time));
            }
            if t0 == "yesterday" || t0 == "ayer" {
                let yest = today - chrono::Duration::days(1);
                return Some(combine_local_date_and_time(yest, time));
            }
            if let Some(rest) = t0.strip_prefix("last_") {
                if let Some(wd) = parse_weekday(rest) {
                    let date = resolve_target_date(today, wd, true);
                    return Some(combine_local_date_and_time(date, time));
                }
            }
            if let Some(wd) = parse_weekday(t0) {
                let date = resolve_target_date(today, wd, false);
                return Some(combine_local_date_and_time(date, time));
            }
            if let Ok(date) = chrono::NaiveDate::parse_from_str(t0, "%Y-%m-%d") {
                return Some(combine_local_date_and_time(date, time));
            }
        }
        return None;
    }

    // 3 tokens: "last monday 14:30"
    if tokens.len() == 3 {
        let t0 = tokens[0];
        let t1 = tokens[1];
        let t2 = tokens[2];

        if t0 == "last" {
            if let Some(wd) = parse_weekday(t1) {
                if let Some(time) = parse_time_token(t2) {
                    let date = resolve_target_date(today, wd, true);
                    return Some(combine_local_date_and_time(date, time));
                }
            }
        }
        return None;
    }

    None
}

fn parse_duration_string(s: &str) -> Option<u64> {
    let s = s.trim().to_lowercase();
    if let Some(rest) = s.strip_suffix('s') {
        rest.parse().ok()
    } else if let Some(rest) = s.strip_suffix('m') {
        rest.parse::<u64>().ok().map(|m| m * 60)
    } else {
        s.parse().ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_iso8601_datetime_rfc3339() {
        let dt = parse_iso8601_datetime("2026-11-05T15:30:00Z").unwrap();
        assert_eq!(dt.to_rfc3339(), "2026-11-05T15:30:00+00:00");

        let dt_offset = parse_iso8601_datetime("2026-11-05T16:30:00+01:00").unwrap();
        assert_eq!(dt_offset.to_rfc3339(), "2026-11-05T15:30:00+00:00");
    }

    #[test]
    fn test_parse_iso8601_datetime_naive() {
        let dt = parse_iso8601_datetime("2026-11-05T15:30:00").unwrap();
        assert_eq!(dt.to_rfc3339(), "2026-11-05T15:30:00+00:00");

        let dt_space = parse_iso8601_datetime("2026-11-05 15:30:00").unwrap();
        assert_eq!(dt_space.to_rfc3339(), "2026-11-05T15:30:00+00:00");
    }

    #[test]
    fn test_parse_iso8601_datetime_date_only() {
        let dt = parse_iso8601_datetime("2026-11-05").unwrap();
        assert_eq!(dt.to_rfc3339(), "2026-11-05T00:00:00+00:00");
    }

    #[test]
    fn test_parse_datetime_keywords_today_and_yesterday() {
        let today_dt = parse_datetime_query("today").unwrap();
        let expected_today = chrono::Local::now().date_naive();
        assert_eq!(
            today_dt.with_timezone(&chrono::Local).date_naive(),
            expected_today
        );

        let yest_dt = parse_datetime_query("yesterday").unwrap();
        let expected_yest = expected_today - chrono::Duration::days(1);
        assert_eq!(
            yest_dt.with_timezone(&chrono::Local).date_naive(),
            expected_yest
        );
    }

    #[test]
    fn test_parse_datetime_keywords_with_time() {
        let dt = parse_datetime_query("today 14:30").unwrap();
        let local_dt = dt.with_timezone(&chrono::Local);
        assert_eq!(local_dt.format("%H:%M:%S").to_string(), "14:30:00");
    }

    #[test]
    fn test_parse_datetime_keywords_last_weekday() {
        let dt = parse_datetime_query("last monday").unwrap();
        let local_date = dt.with_timezone(&chrono::Local).date_naive();
        assert_eq!(local_date.weekday(), chrono::Weekday::Mon);

        let dt_time = parse_datetime_query("last monday 18:00").unwrap();
        let local_dt_time = dt_time.with_timezone(&chrono::Local);
        assert_eq!(local_dt_time.date_naive().weekday(), chrono::Weekday::Mon);
        assert_eq!(local_dt_time.format("%H:%M:%S").to_string(), "18:00:00");
    }

    #[test]
    fn test_parse_datetime_keywords_relative_ago() {
        let _before = Utc::now();
        let dt = parse_datetime_query("30m ago").unwrap();
        let after = Utc::now();
        let diff = after - dt;
        assert!(diff.num_minutes() >= 29 && diff.num_minutes() <= 31);
    }

    #[test]
    fn test_parse_recording_params_with_keywords_and_clip() {
        let usage = "usage: /cmd id date [clip]";
        let (cam_id, dt, clip) = parse_recording_params("2 today 45", usage).unwrap();
        assert_eq!(cam_id, 2);
        assert_eq!(clip, Some(Duration::seconds(45)));
        assert_eq!(
            dt.with_timezone(&chrono::Local).date_naive(),
            chrono::Local::now().date_naive()
        );

        let (cam_id, dt, clip) = parse_recording_params("3 last monday 60", usage).unwrap();
        assert_eq!(cam_id, 3);
        assert_eq!(clip, Some(Duration::seconds(60)));
        assert_eq!(
            dt.with_timezone(&chrono::Local).date_naive().weekday(),
            chrono::Weekday::Mon
        );

        let (cam_id, _, clip) = parse_recording_params("5 last monday", usage).unwrap();
        assert_eq!(cam_id, 5);
        assert_eq!(clip, None);
    }

    #[test]
    fn test_parse_recording_params_with_clip() {
        let usage = "usage: /cmd id date [clip]";
        let (cam_id, dt, clip) =
            parse_recording_params("2 2026-11-05T15:30:00Z 45", usage).unwrap();
        assert_eq!(cam_id, 2);
        assert_eq!(dt.to_rfc3339(), "2026-11-05T15:30:00+00:00");
        assert_eq!(clip, Some(Duration::seconds(45)));
    }

    #[test]
    fn test_parse_recording_params_without_clip() {
        let usage = "usage: /cmd id date [clip]";
        let (cam_id, dt, clip) = parse_recording_params("5 2026-11-05T10:00:00Z", usage).unwrap();
        assert_eq!(cam_id, 5);
        assert_eq!(dt.to_rfc3339(), "2026-11-05T10:00:00+00:00");
        assert_eq!(clip, None);
    }

    #[test]
    fn test_parse_duration_string() {
        assert_eq!(parse_duration_string("30s"), Some(30));
        assert_eq!(parse_duration_string("2m"), Some(120));
        assert_eq!(parse_duration_string("45"), Some(45));
        assert_eq!(parse_duration_string("abc"), None);
    }
}
