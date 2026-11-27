use app_core::domain::object::ObjectClass;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Serialize, Deserialize, ToSchema, Debug, Clone)]
pub struct CameraResponse {
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

impl From<app_services::CameraDto> for CameraResponse {
    fn from(dto: app_services::CameraDto) -> Self {
        Self {
            id: dto.id,
            name: dto.name,
            address: dto.address,
            uri: dto.uri,
            has_credentials: dto.has_credentials,
            username: dto.username,
            snapshot_method: dto.snapshot_method,
            snapshot_uri: dto.snapshot_uri,
            rtsp_uri: dto.rtsp_uri,
            multi_channel_detection: dto.multi_channel_detection,
            subscriptors_count: dto.subscriptors_count,
            manufacturer: dto.manufacturer,
            model: dto.model,
            firmware_version: dto.firmware_version,
            serial_number: dto.serial_number,
        }
    }
}

#[derive(Serialize, Deserialize, ToSchema, Debug, Clone)]
pub struct AddCameraRequest {
    pub uri: String,
    pub username: String,
    pub password: String,
}

#[derive(Serialize, Deserialize, ToSchema, Debug, Clone)]
pub struct SetCameraNameRequest {
    pub name: String,
}

#[derive(Serialize, Deserialize, ToSchema, Debug, Clone)]
pub struct SetCredentialsRequest {
    pub username: String,
    pub password: String,
}

#[derive(Serialize, Deserialize, ToSchema, Debug, Clone)]
pub struct SetSnapshotMethodRequest {
    pub method: String,
}

#[derive(Serialize, Deserialize, ToSchema, Debug, Clone)]
pub struct SetMultiChannelDetectionRequest {
    pub enable: bool,
}

#[derive(Serialize, Deserialize, ToSchema, Debug, Clone)]
pub struct DetectedObjectResponse {
    pub class: String,
    pub confidence: f32,
    pub x1: f32,
    pub y1: f32,
    pub x2: f32,
    pub y2: f32,
}

#[derive(Serialize, Deserialize, ToSchema, Debug, Clone)]
pub struct DetectionResponse {
    pub camera_id: i64,
    pub camera_name: String,
    pub objects: Vec<DetectedObjectResponse>,
}

#[derive(Serialize, Deserialize, ToSchema, Debug, Clone)]
pub struct UpdateDetectorConfigRequest {
    pub enable: Option<bool>,
    pub min_confidence: Option<f32>,
    pub types: Option<Vec<ObjectClass>>,
}

#[derive(Serialize, Deserialize, ToSchema, Debug, Clone)]
pub struct UpdatePollingConfigRequest {
    pub polling_seconds: Option<u64>,
    pub between_seconds: Option<u64>,
    pub send_errors: Option<bool>,
    pub auto_renewal: Option<bool>,
    pub recording_clip: Option<u64>,
}

#[derive(Serialize, Deserialize, ToSchema, Debug, Clone)]
pub struct GenericMessageResponse {
    pub success: bool,
    pub message: String,
}

#[derive(Serialize, Deserialize, ToSchema, Debug, Clone)]
pub struct UpdateManualDetectionConfigRequest {
    pub enable: bool,
    pub interval_seconds: Option<u64>,
    pub types: Option<Vec<ObjectClass>>,
}

#[derive(Deserialize, Debug, Clone)]
pub struct RecordingSummaryQuery {
    pub days: Option<i64>,
}

#[derive(Deserialize, Debug, Clone)]
pub struct RecordingListQuery {
    pub date: Option<String>,
    pub clip_seconds: Option<u64>,
}

#[derive(Serialize, Deserialize, ToSchema, Debug, Clone)]
pub struct LoginRequest {
    #[schema(example = "admin")]
    pub username: String,
    #[schema(example = "admin")]
    pub password: String,
}

#[derive(Serialize, Deserialize, ToSchema, Debug, Clone)]
pub struct LoginResponse {
    #[schema(example = "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9...")]
    pub token: String,
    #[schema(example = "Bearer")]
    pub token_type: String,
    #[schema(example = 86400)]
    pub expires_in: u64,
}

#[derive(Serialize, Deserialize, ToSchema, Debug, Clone)]
pub struct CurrentUserResponse {
    pub username: String,
    pub expires_at: usize,
}

#[derive(Serialize, Deserialize, ToSchema, Debug, Clone)]
pub struct AuthErrorResponse {
    pub error: String,
    pub message: String,
}
