mod config;
mod onvif_camera_client;
mod telegram_client;

use config::AppConfig;
use log::{error, info};
use onvif_camera_client::{is_new_detection, OnvifCameraClient};
use telegram_client::TelegramClient;
use tokio::{select, signal};

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

    let mut telegram_clients: Vec<TelegramClient> = Vec::new();
    config.telegram.user_ids.iter().for_each(|user_id| {
        telegram_clients.push(TelegramClient::new(
            config.telegram.bot_token.clone(),
            user_id.clone(),
        ));
    });

    let mut cameras: Vec<OnvifCameraClient> = Vec::new();
    for camera_config in config.cameras.iter() {
        let mut camera = OnvifCameraClient::new(camera_config.clone());
        camera.init().await;
        cameras.push(camera);
    }

    select! {
        _ = start(config, telegram_clients, cameras) => (),
        _ = signal::ctrl_c() => info!("Closing app"),
    }
}

async fn start(
    config: AppConfig,
    telegram_clients: Vec<TelegramClient>,
    cameras: Vec<OnvifCameraClient>,
) {
    // Main Loop
    loop {
        for camera in &cameras {
            let msg = match camera.get_pull_message().await {
                Ok(msg) => msg,
                Err(_) => continue,
            };

            if is_new_detection(&msg) {
                let snapshot = match camera.get_snapshot().await {
                    Ok(snapshot) => snapshot,
                    Err(_) => continue,
                };

                let time = msg.current_time;
                let duration = (msg.termination_time.value.timestamp_millis()
                    - time.value.clone().timestamp_millis())
                    / 1000;

                for telegram_client in &telegram_clients {
                    telegram_client
                        .send_message_with_picture(
                            &time,
                            duration,
                            camera.camera_name.clone(),
                            snapshot.clone(),
                        )
                        .await;
                }

                println!(
                    "{} - new detection in camera:{} duration:{}",
                    time, camera.camera_name, duration
                );
            }
        }

        tokio::time::sleep(tokio::time::Duration::from_secs(config.polling_seconds)).await;
    }
}
