use chrono::{DateTime, Utc};

pub enum CameraEventType {
    Motion,
}

pub struct CameraEvent {
    pub r#type: CameraEventType,
    pub timestamp: DateTime<Utc>,
}

#[derive(Clone)]
pub struct CameraConnectionData {
    pub uri: String,
    pub username: String,
    pub password: String,
}
