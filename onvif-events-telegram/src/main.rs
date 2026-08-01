mod app_command_processor;
mod config;
mod daily_report;
mod detection_checker;
mod subscription_manager;

use crate::app_command_processor::AppCommandProcessor;
use crate::daily_report::manage_daily_report;
use crate::detection_checker::check_for_detections_in_cameras;
use crate::subscription_manager::{close_subscriptions, renew_subscriptions};
use app_core::domain::event_bus::EventBus;
use app_core::traits::object_detector::ObjectDetector;
use app_core::traits::{command_processor::CommandProcessor, notifier::Notifier};
use config::AppConfig;
use repository::db_store::DbStore;
use repository::memory_repository::MemoryRepository;
use std::fs::OpenOptions;
use std::{process::exit, sync::Arc};
use telegram::{telegram_bot::TelegramBot, telegram_notifier::TelegramNotifier};
use tokio::sync::Mutex;
use tokio::{select, signal};
use tracing::{error, info};
use tracing_appender::non_blocking;
use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};
use ultralitics_detector::UltralyticsDetector;

const DEFAULT_POLLING_SECONDS: u64 = 1;
const DEFAULT_BETWEEN_SECONDS: u64 = 15;

#[tokio::main]
async fn main() {
    let _logging_guard = init_logging().expect("cannot initialize guard");

    let config = read_config();

    info!("START Application with config {:?}", config);

    let event_bus = Arc::new(EventBus::new(100));

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

    let object_detector: Option<Arc<Mutex<dyn ObjectDetector>>> = if config.detector.enable {
        Some(Arc::new(Mutex::new(
            UltralyticsDetector::new("./models/yolo11n.onnx", config.detector.min_confidence)
                .expect("cannot create object detector"),
        )))
    } else {
        None
    };

    let telegram_bot = Arc::new(TelegramBot::new(
        config.telegram.bot_token.clone(),
        config.telegram.user_ids.clone(),
        app_command_processor,
        notifier.clone(),
        event_bus.clone(),
    ));

    select! {
        _ = start_bot(telegram_bot) => (),
        _ = start_polling(notifier, repository.clone(), event_bus.clone(), object_detector) => (),
        _ = renew_subscriptions(repository.clone()) => (),
        _ = signal::ctrl_c() => {
            close_subscriptions(repository).await;
            info!("Closing app")
        },
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

async fn start_polling(
    notifier: Arc<dyn Notifier>,
    repository: Arc<MemoryRepository>,
    event_bus: Arc<EventBus>,
    object_detector: Option<Arc<Mutex<dyn ObjectDetector>>>,
) {
    let mut last_polling = chrono::Local::now();
    loop {
        check_for_detections_in_cameras(
            repository.clone(),
            event_bus.clone(),
            object_detector.clone(),
        )
        .await;
        manage_daily_report(notifier.clone(), repository.clone(), last_polling).await;

        last_polling = chrono::Local::now();

        tokio::time::sleep(tokio::time::Duration::from_secs(
            repository.get_polling_seconds().await,
        ))
        .await;
    }
}

pub fn init_logging() -> anyhow::Result<tracing_appender::non_blocking::WorkerGuard> {
    std::fs::create_dir_all("logs")?;

    // Daily rotation version:
    // let file_appender = tracing_appender::rolling::daily("logs", "onvif-events.log");

    let file = OpenOptions::new()
        .create(true)
        .append(true)
        .open("logs/onvif-events.log")?;

    let (writer, guard) = non_blocking(file);

    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

    let console_layer = fmt::layer()
        .pretty()
        .with_writer(std::io::stdout)
        .with_file(true)
        .with_line_number(true)
        .with_thread_ids(false)
        .with_ansi(true);

    let file_layer = fmt::layer()
        .with_writer(writer)
        .with_ansi(false)
        .with_target(false)
        .with_file(true)
        .with_line_number(true);

    tracing_subscriber::registry()
        .with(filter)
        .with(console_layer)
        .with(file_layer)
        .init();

    tracing::info!("logging initialized");

    Ok(guard)
}
