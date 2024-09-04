use serde::Deserialize;

#[derive(Deserialize, Debug, Clone)]
pub struct AppConfig {
    pub telegram: TelegramConfig,
    pub cameras: Vec<CameraConfig>,
    pub polling_seconds: u64,
}

#[derive(Deserialize, Debug, Clone)]
pub struct TelegramConfig {
    pub bot_token: String,
    pub user_ids: Vec<String>,
}

#[derive(Deserialize, Debug, Clone)]
pub struct CameraConfig {
    pub name: String,
    pub ip: String,
    pub username: String,
    pub password: String,
    pub snapshot_uri: String,
}
