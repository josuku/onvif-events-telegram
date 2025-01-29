mod config;
mod network;
mod onvif;
mod repository;
mod telegram;
mod utils;

use config::AppConfig;
use log::{error, info};
use onvif::onvif_camera::{download_picture, is_new_detection};
use repository::db_store::DbStore;
use repository::memory_repository::MemoryRepository;
use std::{process::exit, sync::Arc};
use telegram::telegram_bot::TelegramBot;
use tokio::{select, signal};
use utils::make_caption;

const DEFAULT_POLLING_SECONDS: u64 = 1;
const DEFAULT_BETWEEN_SECONDS: u64 = 15;

type CameraId = i64;
type SubscriptionId = i64;

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

    let telegram_bot = Arc::new(TelegramBot::new(
        config.telegram.bot_token.clone(),
        config.telegram.user_ids.clone(),
        repository.clone(),
    ));

    select! {
        _ = start_bot(telegram_bot.clone()) => (),
        _ = start_polling(telegram_bot, repository) => (),
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

async fn start_polling(telegram_bot: Arc<TelegramBot>, repository: Arc<MemoryRepository>) {
    let mut last_polling = chrono::Local::now();
    loop {
        check_for_detections(telegram_bot.clone(), repository.clone()).await;
        manage_daily_report(telegram_bot.clone(), repository.clone(), last_polling).await;

        last_polling = chrono::Local::now();

        tokio::time::sleep(tokio::time::Duration::from_secs(
            repository.get_polling_seconds().await,
        ))
        .await;
    }
}

async fn check_for_detections(telegram_bot: Arc<TelegramBot>, repository: Arc<MemoryRepository>) {
    for mut camera in repository.get_cameras().await {
        let msg = match camera.client.get_event_message().await {
            Ok(msg) => msg,
            Err(err) => {
                error!("error getting pull message: {}", err);
                return;
            }
        };

        if is_new_detection(&msg) {
            if let Some(snapshot_uri) = &camera.snapshot_uri {
                let snapshot = match download_picture(snapshot_uri).await {
                    Ok(snapshot) => snapshot,
                    Err(err) => {
                        error!("error getting snapshot: {}", err);
                        return;
                    }
                };
                telegram_bot
                    .send_notification(
                        make_caption(
                            "New Detection",
                            &camera.name,
                            &msg.current_time.value.to_utc(),
                        ),
                        snapshot.clone(),
                        camera.subscriptors.clone(),
                        camera.id,
                    )
                    .await;
            }
            println!(
                "{} - new detection in camera:{}",
                msg.current_time, camera.name
            );
        }
    }
}

async fn manage_daily_report(
    telegram_bot: Arc<TelegramBot>,
    repository: Arc<MemoryRepository>,
    last_polling: chrono::DateTime<chrono::Local>,
) {
    let now = chrono::Local::now();
    let chat_ids = repository.get_daily_report_subscriptors().await;
    if !chat_ids.is_empty() && last_polling.date_naive() != now.date_naive() {
        println!("Sending daily report: {}", now.format("%Y-%m-%d %H:%M:%S"));

        let mut report = String::new();
        report.push_str(&format!("Daily report {}\n", last_polling.date_naive()));

        for camera in repository.get_cameras().await {
            let notifications = repository.get_today_camera_notifications(camera.id).await;
            report.push_str(&format!(
                " - Camera {}-{}: {} detections",
                camera.id,
                camera.name,
                notifications.len()
            ));
        }

        telegram_bot.send_message(report, chat_ids).await;

        repository.clear_today_notifications().await;
    }
}
