use crate::{
    CameraId, ChatId,
    domain::object::Object,
    traits::{api_camera_client::ApiCameraClient, onvif_camera_client::OnvifCameraClient},
};
use chrono::{DateTime, Utc};
use std::{fmt, sync::Arc};

#[derive(Clone, Debug, Copy)]
pub enum CameraEventType {
    Motion,
    Tamper,
}
impl fmt::Display for CameraEventType {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            CameraEventType::Motion => write!(f, "Motion"),
            CameraEventType::Tamper => write!(f, "Tamper"),
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
    pub objects: Vec<Object>,
}

#[derive(Clone, PartialEq)]
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
    pub onvif_client: Arc<dyn OnvifCameraClient>,
    pub api_camera_client: Option<Arc<dyn ApiCameraClient>>,
    pub subscriptors: Vec<ChatId>,
}
impl fmt::Display for CameraData {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let conn_data = self.onvif_client.get_connection_data();
        write!(
            f,
            r#"Camera {}
- Name: {}
- Uri: {:?}
- Credentials: {}
- Address: {}
- SnapshotUri: {} 
- Subscriptors: {}"#,
            self.id,
            self.name,
            conn_data.uri,
            !conn_data.username.is_empty() && !conn_data.password.is_empty(),
            self.address,
            self.snapshot_uri.clone().unwrap_or_default(),
            self.subscriptors.len(),
        )
    }
}

#[derive(Clone)]
pub struct Recording {
    pub name: String,
    pub size_mb: f64,
    pub begin: String,
    pub end: String,
}
