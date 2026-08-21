use crate::domain::camera::CameraData;
use chrono::{DateTime, Utc};

#[derive(Clone)]
pub struct ErrorMessage {
    pub timestamp: DateTime<Utc>,
    pub camera: CameraData,
    pub message: String,
    pub recovered: bool,
}
