use serde::Deserialize;

#[derive(Deserialize, Debug, Clone)]
pub struct AppConfig {
    pub telegram: TelegramConfig,
    pub detector: DetectorConfig,
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
