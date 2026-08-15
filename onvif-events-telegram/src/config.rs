use serde::Deserialize;

#[derive(Deserialize, Debug, Clone)]
pub struct AppConfig {
    pub telegram: TelegramConfig,
    pub detector: DetectorConfig,
    pub default_polling_seconds: u64,
    pub default_between_seconds: u64,
    pub auto_renewal: bool,
}

#[derive(Deserialize, Debug, Clone)]
pub struct TelegramConfig {
    pub bot_token: String,
    pub user_ids: Vec<String>,
}

#[derive(Deserialize, Debug, Clone)]
pub struct DetectorConfig {
    pub enable: bool,
    pub min_confidence: f32,
}
