use anyhow::{Context, Result};
use app_core::{
    CameraId,
    domain::camera::{CameraData, CameraStatus, SnapshotMethod},
    traits::onvif_camera_client::OnvifCameraClient,
};
use onvif::{
    onvif_rs_camera_client::create_onvif_camera_client,
    onvif_rs_service_clients::{DEFAULT_PASSWORD, DEFAULT_USERNAME},
};
use repository::memory_repository::MemoryRepository;
use std::{collections::HashMap, sync::Arc};
use tracing::info;

pub struct CameraService {
    repository: Arc<MemoryRepository>,
}

impl CameraService {
    pub fn new(repository: Arc<MemoryRepository>) -> Self {
        Self { repository }
    }

    pub async fn get_cameras(&self) -> Vec<CameraData> {
        self.repository.get_sorted_cameras().await
    }

    pub async fn get_cameras_dto(&self) -> Vec<crate::dtos::CameraDto> {
        let cameras = self.get_cameras().await;
        let mut dtos = Vec::with_capacity(cameras.len());
        for camera in cameras {
            let conn_data = camera.onvif_client.get_connection_data();
            let device_info = camera.get_device_info().await;
            dtos.push(crate::dtos::CameraDto {
                id: camera.id,
                name: camera.name.clone(),
                address: camera.address.clone(),
                uri: conn_data.uri.clone(),
                has_credentials: !conn_data.username.is_empty() && !conn_data.password.is_empty(),
                username: conn_data.username.clone(),
                snapshot_method: camera.snapshot_method.to_string(),
                snapshot_uri: camera.snapshot_uri.clone(),
                rtsp_uri: camera.rtsp_uri.clone(),
                multi_channel_detection: camera.multi_channel_detection,
                subscriptors_count: camera.subscriptors.len(),
                manufacturer: device_info.as_ref().map(|d| d.manufacturer.clone()),
                model: device_info.as_ref().map(|d| d.model.clone()),
                firmware_version: device_info.as_ref().map(|d| d.firmware_version.clone()),
                serial_number: device_info.as_ref().map(|d| d.serial_number.clone()),
            });
        }
        dtos
    }

    pub async fn get_camera(&self, camera_id: CameraId) -> Result<CameraData> {
        self.repository
            .get_camera(camera_id)
            .await
            .context(format!("Camera with id {} not found", camera_id))
    }

    pub async fn set_camera_name(&self, camera_id: CameraId, name: &str) -> Result<()> {
        info!("Setting camera name for id {}: {}", camera_id, name);
        self.repository.set_camera_name(camera_id, name).await
    }

    pub async fn add_camera(&self, uri: &str, username: &str, password: &str) -> Result<CameraId> {
        info!("Adding camera with URI: {}", uri);
        let client = create_onvif_camera_client(uri, username, password).await?;
        let snapshot_uri = client.get_snapshot_uri().await.ok();
        let rtsp_uri = client.get_rtsp_uri().await.ok();

        let mut camera_data = CameraData {
            id: 0,
            name: uri.to_string(),
            address: uri.to_string(),
            snapshot_uri,
            snapshot_method: SnapshotMethod::Api,
            rtsp_uri,
            multi_channel_detection: false,
            onvif_client: Arc::new(client),
            api_camera_client: None,
            device_info: None,
            subscriptors: Vec::new(),
            status: CameraStatus {
                last_polling: None,
                last_error: None,
                last_error_notified: false,
                last_notification_by_chat_id: HashMap::new(),
                today_notifications: Vec::new(),
                event_subscription_failures: 0,
            },
        };
        camera_data.device_info = camera_data.get_device_info().await;

        self.repository.add_camera(camera_data).await?;
        let cameras = self.repository.get_sorted_cameras().await;
        let added = cameras
            .into_iter()
            .find(|c| c.onvif_client.get_connection_data().uri == uri)
            .map(|c| c.id)
            .unwrap_or(0);
        Ok(added)
    }

    pub async fn delete_camera(&self, camera_id: CameraId) -> Result<()> {
        info!("Deleting camera with id: {}", camera_id);
        self.repository.delete_camera(camera_id).await
    }

    pub async fn set_credentials(
        &self,
        camera_id: CameraId,
        username: &str,
        password: &str,
    ) -> Result<()> {
        info!("Setting credentials for camera id: {}", camera_id);
        let camera = self.get_camera(camera_id).await?;
        let uri = camera.onvif_client.get_connection_data().uri;
        let client = create_onvif_camera_client(&uri, username, password).await?;

        self.repository
            .update_camera_credentials(camera_id, Arc::new(client), username, password)
            .await
    }

    pub async fn set_snapshot_method(
        &self,
        camera_id: CameraId,
        method: SnapshotMethod,
    ) -> Result<()> {
        info!(
            "Setting snapshot method for camera id {}: {:?}",
            camera_id, method
        );
        self.repository
            .update_snapshot_method(camera_id, method)
            .await
    }

    pub async fn set_multi_channel_detection(
        &self,
        camera_id: CameraId,
        enable: bool,
    ) -> Result<()> {
        info!(
            "Setting multi-channel detection for camera id {}: {}",
            camera_id, enable
        );
        self.repository
            .update_multi_channel_detection(camera_id, enable)
            .await
    }

    pub async fn fix_snapshot_uri(&self, camera_id: CameraId) -> Result<String> {
        info!("Fixing snapshot URI for camera id: {}", camera_id);
        let camera = self.get_camera(camera_id).await?;

        if let Some(snapshot_uri) = camera.snapshot_uri {
            let fixed_uri = camera
                .onvif_client
                .create_user_and_fix_snapshot_uri(
                    &camera.onvif_client.get_connection_data().uri,
                    &snapshot_uri,
                )
                .await?;

            self.repository
                .update_snapshot_uri_from_camera(camera_id, &snapshot_uri)
                .await?;

            let client = create_onvif_camera_client(
                &camera.onvif_client.get_connection_data().uri,
                DEFAULT_USERNAME,
                DEFAULT_PASSWORD,
            )
            .await?;

            self.repository
                .update_camera_credentials(
                    camera_id,
                    Arc::new(client),
                    DEFAULT_USERNAME,
                    DEFAULT_PASSWORD,
                )
                .await?;

            Ok(fixed_uri)
        } else {
            let uri = camera.onvif_client.get_snapshot_uri().await?;
            self.repository
                .update_snapshot_uri_from_camera(camera_id, &uri)
                .await?;
            Ok(uri)
        }
    }
}
