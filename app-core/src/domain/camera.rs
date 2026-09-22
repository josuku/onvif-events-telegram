use crate::{
    CameraId, ChatId,
    domain::object::Object,
    traits::{api_camera_client::ApiCameraClient, onvif_camera_client::OnvifCameraClient},
};
use chrono::{DateTime, Utc};
use std::{collections::HashMap, fmt, sync::Arc};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SnapshotMethod {
    Api,
    Rtsp,
}

impl fmt::Display for SnapshotMethod {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            SnapshotMethod::Api => write!(f, "api"),
            SnapshotMethod::Rtsp => write!(f, "rtsp"),
        }
    }
}

impl std::str::FromStr for SnapshotMethod {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "api" => Ok(SnapshotMethod::Api),
            "rtsp" => Ok(SnapshotMethod::Rtsp),
            other => Err(format!("unknown snapshot method: {other} (expected api|rtsp)")),
        }
    }
}

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
    pub snapshot_method: SnapshotMethod,
    pub rtsp_uri: Option<String>,
    pub device_info: Option<DeviceInfo>,
    pub onvif_client: Arc<dyn OnvifCameraClient>,
    pub api_camera_client: Option<Arc<dyn ApiCameraClient>>,
    pub subscriptors: Vec<ChatId>,
    pub status: CameraStatus,
}

impl CameraData {
    pub fn display_short(&self) -> String {
        let conn_data = self.onvif_client.get_connection_data();
        let credentials = if !conn_data.username.is_empty() && !conn_data.password.is_empty() {
            format!("yes ({})", conn_data.username)
        } else {
            "no".to_string()
        };
        format!(
            r#"Camera id:{}
- Name: {}
- Uri: {:?}
- Credentials: {}
- Subscriptors: {}"#,
            self.id,
            self.name,
            conn_data.uri,
            credentials,
            self.subscriptors.len(),
        )
    }

    pub async fn display_full(&self) -> String {
        let conn_data = self.onvif_client.get_connection_data();
        let credentials = if !conn_data.username.is_empty() && !conn_data.password.is_empty() {
            format!("yes ({})", conn_data.username)
        } else {
            "no".to_string()
        };
        if let Some(device_info) = self.get_device_info().await {
            format!(
                r#"Camera {}
    - Name: {}
    - Uri: {:?}
    - Credentials: {}
    - Subscriptors: {}
    - Manufacturer: {}
    - Model: {}
    - Firmware_version: {}
    - Serial Number: {}
    - Address: {}
    - SnapshotMethod: {}
    - SnapshotUri: {}
    - RtspUri: {}
    "#,
                self.id,
                self.name,
                conn_data.uri,
                credentials,
                self.subscriptors.len(),
                device_info.manufacturer,
                device_info.model,
                device_info.firmware_version,
                device_info.serial_number,
                self.address,
                self.snapshot_method,
                self.snapshot_uri.as_deref().unwrap_or_default(),
                self.rtsp_uri.as_deref().unwrap_or_default(),
            )
        } else {
            format!(
                r#"Camera {}
    - Name: {}
    - Uri: {:?}
    - Credentials: {}
    - Address: {}
    - SnapshotMethod: {}
    - SnapshotUri: {}
    - RtspUri: {}
    - Subscriptors: {}"#,
                self.id,
                self.name,
                conn_data.uri,
                !conn_data.username.is_empty() && !conn_data.password.is_empty(),
                self.address,
                self.snapshot_method,
                self.snapshot_uri.as_deref().unwrap_or_default(),
                self.rtsp_uri.as_deref().unwrap_or_default(),
                self.subscriptors.len(),
            )
        }
    }
}

#[derive(Clone)]
pub struct CameraStatus {
    pub last_polling: Option<DateTime<Utc>>,
    pub last_error: Option<DateTime<Utc>>,
    pub last_error_notified: bool,
    pub last_notification_by_chat_id: HashMap<ChatId, chrono::DateTime<Utc>>,
    pub today_notifications: Vec<DateTime<Utc>>,
}

#[derive(Clone)]
pub struct Recording {
    pub name: String,
    pub size_mb: f64,
    pub begin: String,
    pub end: String,
}

#[derive(Clone)]
pub struct DeviceInfo {
    pub manufacturer: String,
    pub model: String,
    pub firmware_version: String,
    pub serial_number: String,
}

impl CameraData {
    pub fn host(&self) -> String {
        self.onvif_client.get_connection_data().uri
    }

    pub fn username(&self) -> String {
        self.onvif_client.get_connection_data().username
    }

    pub fn password(&self) -> String {
        self.onvif_client.get_connection_data().password
    }

    pub async fn get_snapshot(&self) -> anyhow::Result<Vec<u8>> {
        match self.snapshot_method {
            SnapshotMethod::Rtsp => self.onvif_client.snapshot_via_rtsp().await,
            SnapshotMethod::Api => match self.onvif_client.snapshot().await {
                Ok(bytes) => Ok(bytes),
                Err(err) => {
                    tracing::warn!(
                        "snapshot via api failed for camera {} ({}), falling back to rtsp",
                        self.id,
                        err
                    );
                    self.onvif_client.snapshot_via_rtsp().await
                }
            },
        }
    }

    pub async fn get_device_info(&self) -> Option<DeviceInfo> {
        if let Some(device_info) = &self.device_info {
            return Some(device_info.clone());
        }

        match self.onvif_client.get_device_info().await {
            Ok(info) => Some(info),
            Err(err) => {
                tracing::error!("Error getting device info. {}", err);
                None
            }
        }
    }
}
