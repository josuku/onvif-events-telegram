use crate::domain::object::ObjectClass;
use serde::Deserialize;

#[derive(Deserialize, Debug, Clone)]
pub struct AppConfig {
    pub telegram: TelegramConfig,
    #[serde(rename = "default_config")]
    pub bot_config: BotConfig,
}

#[derive(Deserialize, Debug, Clone)]
pub struct TelegramConfig {
    pub bot_token: String,
    pub user_ids: Vec<String>,
}

#[derive(Deserialize, Debug, Clone)]
pub struct BotConfig {
    pub polling_seconds: u64,
    pub between_seconds: u64,
    pub send_errors: bool,
    pub auto_renewal: bool,
    pub recording_clip: u64,
    pub detector: DetectorConfig,
    pub manual_detection: ManualDetectionConfig,
}

#[derive(Deserialize, Debug, Clone)]
pub struct DetectorConfig {
    pub enable: bool,
    pub min_confidence: f32,
    pub types: Vec<ObjectClass>,
}

#[derive(Deserialize, Debug, Clone)]
pub struct ManualDetectionConfig {
    pub enable: bool,
    pub interval_seconds: u64,
    pub types: Vec<ObjectClass>,
}

impl Default for ManualDetectionConfig {
    fn default() -> Self {
        Self {
            enable: false,
            interval_seconds: 30,
            types: Vec::new(),
        }
    }
}
