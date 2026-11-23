use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct CameraDto {
    pub id: i64,
    pub name: String,
    pub address: String,
    pub uri: String,
    pub has_credentials: bool,
    pub username: String,
    pub snapshot_method: String,
    pub snapshot_uri: Option<String>,
    pub rtsp_uri: Option<String>,
    pub multi_channel_detection: bool,
    pub subscriptors_count: usize,
    pub manufacturer: Option<String>,
    pub model: Option<String>,
    pub firmware_version: Option<String>,
    pub serial_number: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct AddCameraRequest {
    pub uri: String,
    pub username: String,
    pub password: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct SetCameraNameRequest {
    pub name: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct SetCredentialsRequest {
    pub username: String,
    pub password: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct SetSnapshotMethodRequest {
    pub method: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct SetMultiChannelDetectionRequest {
    pub enable: bool,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct LoginRequest {
    pub username: String,
    pub password: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct LoginResponse {
    pub token: String,
    pub token_type: String,
    pub expires_in: u64,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct AuthClaims {
    pub sub: String,
    pub exp: usize,
    pub iat: usize,
}
