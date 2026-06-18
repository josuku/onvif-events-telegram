mod app_command_processor;
mod config;

use crate::app_command_processor::AppCommandProcessor;
use app_core::{
    make_caption,
    traits::{command_processor::CommandProcessor, notifier::Notifier},
};
use config::AppConfig;
use log::{error, info};
use onvif::onvif_camera_client::create_onvif_camera_client;
use repository::db_store::DbStore;
use repository::memory_repository::MemoryRepository;
use std::{process::exit, sync::Arc};
use telegram::{telegram_bot::TelegramBot, telegram_notifier::TelegramNotifier};
use tokio::{select, signal};

const DEFAULT_POLLING_SECONDS: u64 = 1;
const DEFAULT_BETWEEN_SECONDS: u64 = 15;

#[tokio::main]
async fn main() {
    pretty_env_logger::init();

    let config = read_config();

    let repo_store = Arc::new(DbStore::new());
    repo_store.create_tables();
    let repository = Arc::new(MemoryRepository::new(
        DEFAULT_POLLING_SECONDS,
        DEFAULT_BETWEEN_SECONDS,
        repo_store,
    ));
    repository
        .load_from_store()
        .await
        .expect("cannot load from store");

    let notifier: Arc<dyn Notifier> = Arc::new(TelegramNotifier::new(
        config.telegram.bot_token.clone(),
        repository.clone(),
    ));

    let app_command_processor: Arc<dyn CommandProcessor> = Arc::new(AppCommandProcessor::new(
        repository.clone(),
        notifier.clone(),
    ));

    let telegram_bot = Arc::new(TelegramBot::new(
        config.telegram.bot_token.clone(),
        config.telegram.user_ids.clone(),
        app_command_processor,
    ));

    select! {
        _ = start_bot(telegram_bot) => (),
        _ = start_polling(notifier, repository) => (),
        _ = signal::ctrl_c() => info!("Closing app"),
    }
}

fn read_config() -> AppConfig {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 2 {
        error!("onvif events telegram: use CONFIG_FILE");
        exit(1);
    }
    let config_content = std::fs::read_to_string(&args[1]).expect("Could not read config");
    serde_yaml::from_str(&config_content).expect("Config file parsed")
}

async fn start_bot(telegram_bot: Arc<TelegramBot>) {
    telegram_bot.start().await;
}

async fn start_polling(notifier: Arc<dyn Notifier>, repository: Arc<MemoryRepository>) {
    let mut last_polling = chrono::Local::now();
    loop {
        check_for_detections_in_cameras(notifier.clone(), repository.clone()).await;
        manage_daily_report(notifier.clone(), repository.clone(), last_polling).await;

        last_polling = chrono::Local::now();

        tokio::time::sleep(tokio::time::Duration::from_secs(
            repository.get_polling_seconds().await,
        ))
        .await;
    }
}

async fn check_for_detections_in_cameras(
    notifier: Arc<dyn Notifier>,
    repository: Arc<MemoryRepository>,
) {
    let now = chrono::Utc::now();

    for camera in repository.get_cameras().await {
        let msg = match camera.client.get_event_message().await {
            Ok(msg) => match msg {
                Some(msg) => msg,
                None => continue,
            },
            Err(err) => {
                error!("error getting pull message. error:{}", err);
                let conn_data = camera.client.get_connection_data();
                match create_onvif_camera_client(
                    &conn_data.uri,
                    &conn_data.username,
                    &conn_data.password,
                )
                .await
                {
                    Ok(client) => {
                        if let Err(err) = repository
                            .replace_camera_client(camera.id, Arc::new(client))
                            .await
                        {
                            error!("cannot replace camera client in repository. error:{}", err);
                        }
                        continue;
                    }
                    Err(err) => {
                        error!("cannot create onvif camera client. error:{}", err);
                        continue;
                    }
                };
            }
        };

        repository
            .update_last_polling_from_camera(camera.id, now)
            .await;

        let snapshot = match camera.client.snapshot().await {
            Ok(snapshot) => snapshot,
            Err(err) => {
                error!(
                    "error getting snapshot from camera:{}. error:{}",
                    camera.name, err
                );
                continue;
            }
        };

        notifier
            .send_text_with_picture_message(
                make_caption("New Detection", &camera.name, &msg.timestamp),
                snapshot.clone(),
                camera.subscriptors.clone(),
                camera.id,
            )
            .await;

        println!(
            "{} - new detection in camera:{}",
            msg.timestamp, camera.name
        );
    }
}

async fn manage_daily_report(
    notifier: Arc<dyn Notifier>,
    repository: Arc<MemoryRepository>,
    last_polling: chrono::DateTime<chrono::Local>,
) {
    let now = chrono::Local::now();
    let chat_ids = repository.get_daily_report_subscriptors().await;
    if !chat_ids.is_empty() && last_polling.date_naive() != now.date_naive() {
        println!("Sending daily report: {}", now.format("%Y-%m-%d %H:%M:%S"));

        let mut report = String::new();
        report.push_str(&format!("Daily report {}\n", last_polling.date_naive()));

        for camera in repository.get_sorted_cameras().await {
            let notifications = repository.get_today_camera_notifications(camera.id).await;
            let mut status = "";
            if !camera.client.connected() {
                status = "\n (disconnected)";
            }
            let mut last_sync = "".to_string();
            if let Some(last_polling_time) =
                repository.get_last_polling_from_camera(camera.id).await
            {
                last_sync = format!("\n   (sync: {})", time_ago(now.to_utc(), last_polling_time));
            }

            report.push_str(&format!(
                " - Camera {}-{}: {} detections{}{}\n",
                camera.id,
                camera.name,
                notifications.len(),
                status,
                last_sync,
            ));
        }

        notifier.send_text_message(report, chat_ids).await;

        repository.clear_today_notifications().await;
    }
}

fn time_ago(to: chrono::DateTime<chrono::Utc>, from: chrono::DateTime<chrono::Utc>) -> String {
    let duration = to.signed_duration_since(from);

    if duration.num_seconds() < 60 {
        format!("{} secs ago", duration.num_seconds())
    } else if duration.num_minutes() < 60 {
        format!("{} mins ago", duration.num_minutes())
    } else if duration.num_hours() < 24 {
        format!("{} hours ago", duration.num_hours())
    } else {
        format!("{} days ago", duration.num_days())
    }
}
