use app_core::{traits::command_processor::CommandProcessor, CameraId};
use log::error;
use std::sync::Arc;
use teloxide::{
    dispatching::{Dispatcher, HandlerExt, UpdateFilterExt},
    dptree,
    types::Update,
    utils::command::BotCommands,
    Bot,
};

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
    #[command(description = "set camera name. params camera_id camera_name")]
    SetCameraName(String),
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
    #[command(description = "set seconds between notifications.")]
    SetBetweenTime(u64),
    #[command(description = "fix snapshot uri camera id. params: camera_id")]
    FixSnapshot(CameraId),
    #[command(description = "enable/disable daily report. params: true/false")]
    DailyReport(bool),
}

#[derive(Clone)]
pub struct TelegramBot {
    client: Bot,
    allowed_chat_ids: Vec<String>,
    command_processor: Arc<dyn CommandProcessor>,
}

impl TelegramBot {
    pub fn new(
        bot_token: String,
        chat_ids: Vec<String>,
        command_processor: Arc<dyn CommandProcessor>,
    ) -> Self {
        Self {
            client: Bot::new(bot_token),
            allowed_chat_ids: chat_ids,
            command_processor,
        }
    }

    pub async fn start(&self) {
        let handler = Update::filter_message().branch(
            dptree::entry()
                .filter_command::<BotCommand>()
                .endpoint(process_command),
        );

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
}

async fn process_command(
    _bot: Bot,
    msg: teloxide::types::Message,
    allowed_chat_ids: Vec<String>,
    command_processor: Arc<dyn CommandProcessor>,
    cmd: BotCommand,
) -> teloxide::requests::ResponseResult<()> {
    if !allowed_chat_ids.contains(&format!("{}", msg.chat.id)) {
        error!(
            "not allowed chat id:{:?} allowed:{:?}",
            msg.chat.id, allowed_chat_ids
        );
    }
    let chat_id = msg.chat.id.0;
    match cmd {
        BotCommand::Help => command_processor
            .help_cmd(chat_id, &BotCommand::descriptions().to_string())
            .await
            .map_err(anyhow_to_response_error)?,
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
        BotCommand::GetSnapshot(camera_id) => command_processor
            .get_snapshot_cmd(chat_id, camera_id)
            .await
            .map_err(anyhow_to_response_error)?,
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
    };
    Ok(())
}

fn anyhow_to_response_error(err: anyhow::Error) -> teloxide::RequestError {
    teloxide::RequestError::Api(teloxide::ApiError::Unknown(err.to_string()))
}

fn string_to_response_error(err: String) -> teloxide::RequestError {
    teloxide::RequestError::Api(teloxide::ApiError::Unknown(err))
}
