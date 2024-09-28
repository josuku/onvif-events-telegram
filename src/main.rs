mod config;
mod onvif;
mod repository;
mod telegram;

use config::AppConfig;
use log::{error, info};
use onvif::onvif_camera::{download_picture, is_new_detection};
use repository::db_store::DbStore;
use repository::memory_repository::MemoryRepository;
use std::sync::Arc;
use telegram::telegram_client::{make_caption, TelegramBot};
use tokio::{select, signal};

const DEFAULT_POLLING_SECONDS: u64 = 2;

type CameraId = i64;
type SubscriptionId = i64;

#[tokio::main]
async fn main() {
    pretty_env_logger::init();

    let args: Vec<String> = std::env::args().collect();
    if args.len() != 2 {
        error!("onvif events telegram: use CONFIG_FILE");
        return;
    }

    let config_content = std::fs::read_to_string(&args[1]).expect("Could not read config");
    let config: AppConfig = serde_yaml::from_str(&config_content).expect("Config file parsed");

    let repo_store = Arc::new(DbStore::new());
    repo_store.load();
    let repository = Arc::new(MemoryRepository::new(DEFAULT_POLLING_SECONDS, repo_store));
    let _ = repository.init().await;

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

async fn start_bot(telegram_bot: Arc<TelegramBot>) {
    telegram_bot.start().await;
}

async fn start_polling(telegram_bot: Arc<TelegramBot>, repository: Arc<MemoryRepository>) {
    loop {
        for camera in &repository.get_cameras().await {
            let msg = match camera.client.get_event_message().await {
                Ok(msg) => msg,
                Err(err) => {
                    error!("error getting pull message: {}", err);
                    continue;
                }
            };

            if is_new_detection(&msg) {
                if let Some(snapshot_uri) = &camera.snapshot_uri {
                    let snapshot = match download_picture(snapshot_uri).await {
                        Ok(snapshot) => snapshot,
                        Err(err) => {
                            error!("error getting snapshot: {}", err);
                            continue;
                        }
                    };
                    telegram_bot
                        .send_message_with_picture(
                            make_caption(
                                "New Detection",
                                &camera.name,
                                &msg.current_time.value.to_utc(),
                            ),
                            snapshot.clone(),
                            camera.subscriptors.clone(),
                        )
                        .await;
                }
                println!(
                    "{} - new detection in camera:{}",
                    msg.current_time, camera.name
                );
            }
        }

        tokio::time::sleep(tokio::time::Duration::from_secs(
            repository.get_polling_seconds().await,
        ))
        .await;
    }
}
