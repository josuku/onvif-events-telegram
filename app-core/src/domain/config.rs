use crate::domain::object::ObjectClass;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct AppConfig {
    pub telegram: TelegramConfig,
    #[serde(default)]
    pub http: HttpConfig,
    #[serde(rename = "default_config")]
    pub bot_config: BotConfig,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct HttpConfig {
    pub enabled: bool,
    pub host: String,
    pub port: u16,
    #[serde(default)]
    pub auth: AuthConfig,
}

impl Default for HttpConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            host: "0.0.0.0".to_string(),
            port: 8080,
            auth: AuthConfig::default(),
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
pub struct AuthConfig {
    pub enabled: bool,
    pub username: String,
    pub password: String,
    #[serde(default = "default_jwt_secret")]
    pub jwt_secret: String,
    #[serde(default = "default_token_expiration_hours")]
    pub token_expiration_hours: u64,
}

fn default_jwt_secret() -> String {
    "onvif-events-telegram-jwt-secret-key-change-me".to_string()
}

fn default_token_expiration_hours() -> u64 {
    24
}

impl Default for AuthConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            username: "admin".to_string(),
            password: "admin".to_string(),
            jwt_secret: default_jwt_secret(),
            token_expiration_hours: default_token_expiration_hours(),
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct TelegramConfig {
    pub bot_token: String,
    pub user_ids: Vec<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
pub struct BotConfig {
    pub polling_seconds: u64,
    pub between_seconds: u64,
    pub send_errors: bool,
    pub auto_renewal: bool,
    pub recording_clip: u64,
    pub detector: DetectorConfig,
    pub manual_detection: ManualDetectionConfig,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
pub struct DetectorConfig {
    pub enable: bool,
    pub min_confidence: f32,
    pub types: Vec<ObjectClass>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
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
