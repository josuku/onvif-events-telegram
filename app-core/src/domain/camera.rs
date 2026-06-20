use std::{fmt, sync::Arc};

use chrono::{DateTime, Utc};

use crate::{CameraId, ChatId, traits::camera_client::CameraClient};

#[derive(Clone, Debug, Copy)]
pub enum CameraEventType {
    Motion,
}
impl fmt::Display for CameraEventType {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            CameraEventType::Motion => write!(f, "Motion"),
        }
    }
}

pub struct OnvifCameraEvent {
    pub r#type: CameraEventType,
    pub timestamp: DateTime<Utc>,
}

#[derive(Clone)]
pub struct CameraEvent {
    pub r#type: CameraEventType,
    pub timestamp: DateTime<Utc>,
    pub camera: CameraData,
    pub snapshot: Vec<u8>, // TODO try with Arc<Vec<u8>> less memory
                           // pub detections: Vec<Detection>, // TODO when implement yolo object detection
}

#[derive(Clone)]
pub struct CameraConnectionData {
    pub uri: String,
    pub username: String,
    pub password: String,
}

#[derive(Clone)]
pub struct CameraData {
    pub id: CameraId,
    pub name: String,
    pub address: String,
    pub snapshot_uri: Option<String>,
    pub client: Arc<dyn CameraClient>,
    pub subscriptors: Vec<ChatId>,
}
impl fmt::Display for CameraData {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            r#"
Camera {}
- Name: {}
- Uri: {:?}
- Address: {}
- SnapshotUri: {} 
- Subscriptors: {}"#,
            self.id,
            self.name,
            self.client.get_connection_data().uri,
            self.address,
            self.snapshot_uri.clone().unwrap_or_default(),
            self.subscriptors.len(),
        )
    }
}
