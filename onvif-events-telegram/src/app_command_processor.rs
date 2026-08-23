use api_camera::dahua_rpc_api_client::DahuaRpcApiCameraClient;
use api_camera::dvrip_xmeye_client::DvrIpXmeyeApiCameraClient;
use app_core::domain::camera::{CameraData, CameraStatus};
use app_core::domain::object::{ObjectClass, object_classes_to_string};
use app_core::helpers::network::is_reachable;
use app_core::traits::api_camera_client::ApiCameraClient;
use app_core::traits::command_processor::DownloadRecordingError;
use app_core::traits::discovery_client::OnvifRsDiscoveryClient;
use app_core::traits::onvif_camera_client::OnvifCameraClient;
use app_core::MessageId;
use app_core::{
    make_caption,
    traits::{command_processor::CommandProcessor, notifier::Notifier},
    CameraId, ChatId,
};
use async_trait::async_trait;
use chrono::{DateTime, Duration, Utc};
use onvif::onvif_rs_camera_client::create_onvif_camera_client;
use onvif::onvif_rs_discovery_client::OnvifDiscoveryClient;
use repository::memory_repository::MemoryRepository;
use std::collections::HashMap;
use std::sync::Arc;
use tracing::{error, info};

pub struct AppCommandProcessor {
    repository: Arc<MemoryRepository>,
    notifier: Arc<dyn Notifier>,
}

impl AppCommandProcessor {
    pub fn new(repository: Arc<MemoryRepository>, notifier: Arc<dyn Notifier>) -> Self {
        Self {
            repository,
            notifier,
        }
    }

    async fn send_error(&self, error: &str, chat_id: ChatId) {
        error!("{}", error);
        let _ = self
            .notifier
            .send_text_message(format!("❌ {}", error), vec![chat_id])
            .await;
    }

    async fn send_success(&self, message: &str, chat_id: ChatId) {
        info!("{}", message);
        let _ = self
            .notifier
            .send_text_message(format!("✅ {}", message), vec![chat_id])
            .await;
    }

    async fn get_snapshot_of_camera_id(
        &self,
        chat_id: ChatId,
        camera_id: CameraId,
    ) -> anyhow::Result<()> {
        let camera = match self.repository.get_camera(camera_id).await {
            Some(camera) => camera,
            None => {
                let error = format!("cannot find camera with id: {}", camera_id);
                self.send_error(&error, chat_id).await;
                anyhow::bail!(error);
            }
        };

        let snapshot = match camera.onvif_client.snapshot().await {
            Ok(snapshot) => snapshot,
            Err(err) => {
                let error = format!(
                    "error getting snapshot from camera:{:?} err:{}",
                    camera.name, err
                );
                self.send_error(&error, chat_id).await;
                anyhow::bail!(error);
            }
        };

        _ = self
            .notifier
            .send_picture_message(
                &make_caption(
                    "Snapshot",
                    &camera.name,
                    &camera.id,
                    &chrono::Utc::now(),
                    None,
                    &Vec::new(),
                ),
                snapshot.clone(),
                chat_id,
                camera_id,
                &chrono::Utc::now(),
            )
            .await;

        Ok(())
    }
}

#[async_trait]
impl CommandProcessor for AppCommandProcessor {
    async fn help_cmd(&self, chat_id: ChatId, help_text: &str) -> anyhow::Result<()> {
        info!("command Help - chat_id:{}", chat_id);
        self.notifier
            .send_text_message(help_text.to_owned(), vec![chat_id])
            .await;
        Ok(())
    }

    async fn get_cameras_cmd(&self, chat_id: ChatId) -> anyhow::Result<()> {
        info!("command GetCameras - chat_id:{}", chat_id);

        let discovered_devices = OnvifDiscoveryClient::camera_discovery().await;

        if let Err(err) = self
            .repository
            .update_repository_cameras(&discovered_devices)
            .await
        {
            error!("cannot update cameras: {}", err);
        }

        let mut cameras = self.repository.get_cameras().await;
        if cameras.is_empty() {
            self.notifier
                .send_text_message("No available cameras".to_string(), vec![chat_id])
                .await;
        } else {
            cameras.sort_by_key(|a| a.id);
            let mut lines = vec!["Available cameras:".to_string()];
            for camera in cameras {
                let reachable = is_reachable(&camera.onvif_client.get_connection_data().uri).await;
                let has_error = camera.status.last_error.is_some();
                let status = if reachable && !has_error {
                    "🟢"
                } else if !reachable {
                    "🔴"
                } else {
                    "🟠"
                };
                lines.push(format!("{status} {camera}"));
            }
            self.notifier
                .send_text_message(lines.join("\n"), vec![chat_id])
                .await;
        }
        Ok(())
    }

    async fn set_camera_name_cmd(
        &self,
        chat_id: ChatId,
        camera_id: CameraId,
        camera_name: &str,
    ) -> anyhow::Result<()> {
        info!(
            "command SubsSetCameraName - chat id:{} camera_id:{} camera_name:{}",
            chat_id, camera_id, camera_name
        );
        let _ = match self
            .repository
            .set_camera_name(camera_id, camera_name)
            .await
        {
            Ok(_) => {
                self.send_success("Camera name updated successfully", chat_id)
                    .await
            }
            Err(err) => self.send_error(&err.to_string(), chat_id).await,
        };
        Ok(())
    }

    async fn subscribe_cmd(&self, chat_id: ChatId, camera_id: CameraId) -> anyhow::Result<()> {
        info!(
            "command SubscribeToCamera - chat id:{} camera_id:{}",
            chat_id, camera_id
        );
        let _ = match self
            .repository
            .subscribe_to_camera(camera_id, chat_id, true)
            .await
        {
            Ok(_) => self.send_success("Subscribed successfully", chat_id).await,
            Err(err) => self.send_error(&err.to_string(), chat_id).await,
        };
        Ok(())
    }

    async fn unsubscribe_cmd(&self, chat_id: ChatId, camera_id: CameraId) -> anyhow::Result<()> {
        info!(
            "command UnsubscribeFromCamera - chat id:{} camera_id:{}",
            chat_id, camera_id
        );
        let _ = match self
            .repository
            .unsubscribe_from_camera(camera_id, chat_id, true)
            .await
        {
            Ok(_) => {
                self.send_success("Unsubscribed successfully", chat_id)
                    .await
            }
            Err(err) => self.send_error(&err.to_string(), chat_id).await,
        };
        Ok(())
    }

    async fn get_snapshot_cmd(
        &self,
        chat_id: ChatId,
        camera_id: Option<CameraId>,
    ) -> anyhow::Result<()> {
        info!(
            "command GetSnapshotOfCamera - chat id:{} camera_id:{:?}",
            chat_id, camera_id
        );

        if let Some(camera_id) = camera_id {
            let _ = self.get_snapshot_of_camera_id(chat_id, camera_id).await;
        } else {
            for camera in self.repository.get_sorted_cameras().await.iter() {
                let _ = self.get_snapshot_of_camera_id(chat_id, camera.id).await;
            }
        }

        Ok(())
    }

    async fn config_polling_time_cmd(&self, chat_id: ChatId, seconds: u64) -> anyhow::Result<()> {
        info!(
            "command ConfigPollingTime - chat id:{} seconds:{}",
            chat_id, seconds
        );
        self.repository
            .config_polling_seconds(seconds, chat_id)
            .await;
        self.send_success("Polling time config updated successfully", chat_id)
            .await;
        Ok(())
    }

    async fn config_between_time_cmd(&self, chat_id: ChatId, seconds: u64) -> anyhow::Result<()> {
        info!(
            "command ConfigBetweenTime - chat id:{} seconds:{}",
            chat_id, seconds
        );
        self.repository
            .config_between_seconds(seconds, chat_id)
            .await;
        self.send_success("Between time config updated successfully", chat_id)
            .await;
        Ok(())
    }

    async fn config_send_errors_cmd(
        &self,
        chat_id: ChatId,
        send_errors: bool,
    ) -> anyhow::Result<()> {
        info!("command ConfigSendErrors - send_errors:{}", send_errors);
        self.repository
            .config_send_errors(send_errors, chat_id)
            .await;
        self.send_success("Send errors config updated successfully", chat_id)
            .await;
        Ok(())
    }

    async fn config_auto_renewal_cmd(&self, chat_id: ChatId, enable: bool) -> anyhow::Result<()> {
        info!("command ConfigAutoRenewal - enable:{}", enable);
        self.repository.config_auto_renewal(enable, chat_id).await;
        self.send_success("Auto renewal config updated successfully", chat_id)
            .await;
        Ok(())
    }

    async fn config_recording_clip_cmd(&self, chat_id: ChatId, seconds: u64) -> anyhow::Result<()> {
        info!("command ConfiRecordingClip - seconds:{}", seconds);
        self.repository
            .config_recording_clip(seconds, chat_id)
            .await;
        self.send_success("Recording clip config updated successfully", chat_id)
            .await;
        Ok(())
    }

    async fn config_detector_enable_cmd(
        &self,
        chat_id: ChatId,
        enable: bool,
    ) -> anyhow::Result<()> {
        info!("command ConfigDetectorEnable - enable:{}", enable);
        self.repository
            .config_detector_enable(enable, chat_id)
            .await;
        self.send_success("Detector enable config updated successfully", chat_id)
            .await;
        Ok(())
    }

    async fn config_detector_min_confidence_cmd(
        &self,
        chat_id: ChatId,
        confidence: f32,
    ) -> anyhow::Result<()> {
        info!(
            "command ConfigDetectorMinConfidence - min_confidence:{}",
            confidence
        );
        self.repository
            .config_detector_min_confidence(confidence, chat_id)
            .await;
        self.send_success(
            "Detector min confidence config updated successfully",
            chat_id,
        )
        .await;
        Ok(())
    }

    async fn config_detector_types_cmd(
        &self,
        chat_id: ChatId,
        types: Vec<ObjectClass>,
    ) -> anyhow::Result<()> {
        info!(
            "command ConfigDetectorTypes - types:{}",
            object_classes_to_string(&types),
        );
        self.repository
            .config_detector_types(types, chat_id)
            .await;
        self.send_success(
            "Detector types config updated successfully",
            chat_id,
        )
        .await;
        Ok(())
    }

    async fn get_config(&self, chat_id: ChatId) {
        info!("command GetConfig");
        let config = self.repository.get_config().await;
        let available_types = self.repository.get_detector_types().await;
        let message = format!(
            r#"
CURRENT CONFIG
🔄 Polling seconds: {}
📢 Between seconds: {}
🚨 Send sync errors: {}
🔄 Auto renewal: {}
🎞️ Recording clip seconds: {}
🕵🏽 Detector enabled: {}
🎯 Detector min confidence: {}
🔎 Detection types: {}
* available types: {}
"#,
            config.polling_seconds,
            config.between_seconds,
            config.send_errors,
            config.auto_renewal,
            config.recording_clip,
            config.detector.enable,
            config.detector.min_confidence,
            object_classes_to_string(&config.detector.types),
            object_classes_to_string(&available_types),
        );
        self.notifier
            .send_text_message(message, vec![chat_id])
            .await;
    }

    async fn reset_config(&self, chat_id: ChatId) {
        info!("command ResetConfig");
        self.repository.reset_config().await;
        self.get_config(chat_id).await;
    }

    async fn fix_snapshot_uri_cmd(
        &self,
        chat_id: ChatId,
        camera_id: CameraId,
    ) -> anyhow::Result<()> {
        info!(
            "command FixSnapshotCmd - chat id:{} camera_id:{}",
            chat_id, camera_id
        );
        let camera = match self.repository.get_camera(camera_id).await {
            Some(camera) => camera,
            None => {
                let error = format!("cannot find camera with id: {}", camera_id);
                self.send_error(&error, chat_id).await;
                anyhow::bail!(error);
            }
        };

        if let Some(snapshot_uri) = camera.snapshot_uri {
            match camera
                .onvif_client
                .create_user_and_fix_snapshot_uri(
                    &camera.onvif_client.get_connection_data().uri,
                    &snapshot_uri,
                )
                .await
            {
                Ok(fixed_uri) => {
                    match self
                        .repository
                        .update_snapshot_uri_from_camera(camera_id, &fixed_uri)
                        .await
                    {
                        Ok(_) => {
                            let message = format!("Camera snapshot uri fixed:{}", fixed_uri);
                            self.send_success(&message, chat_id).await;
                        }
                        Err(err) => {
                            self.send_error(&err.to_string(), chat_id).await;
                            anyhow::bail!(err.to_string());
                        }
                    }
                }
                Err(err) => {
                    self.send_error(&err.to_string(), chat_id).await;
                    anyhow::bail!(err.to_string());
                }
            }
        } else {
            match camera.onvif_client.get_snapshot_uri().await {
                Ok(uri) => {
                    match self
                        .repository
                        .update_snapshot_uri_from_camera(camera_id, &uri)
                        .await
                    {
                        Ok(_) => {
                            let message = format!("Camera snapshot uri resolved:{}", uri);
                            self.send_success(&message, chat_id).await;
                        }
                        Err(err) => {
                            self.send_error(&err.to_string(), chat_id).await;
                            anyhow::bail!(err.to_string());
                        }
                    }
                }
                Err(err) => {
                    let error = format!("cannot resolve snapshot uri: {}", err);
                    self.send_error(&error, chat_id).await;
                    anyhow::bail!(error);
                }
            }
        }
        Ok(())
    }

    async fn enable_daily_report_cmd(&self, chat_id: ChatId, enable: bool) -> anyhow::Result<()> {
        if enable {
            self.send_success("Subscribed to daily report", chat_id)
                .await;
            self.repository
                .subscribe_to_daily_report(chat_id, true)
                .await;
        } else {
            self.send_success("Unsubscribed from daily report", chat_id)
                .await;
            self.repository
                .unsubscribe_from_daily_report(chat_id, true)
                .await;
        }
        Ok(())
    }

    async fn add_camera_cmd(
        &self,
        chat_id: ChatId,
        uri: &str,
        username: &str,
        password: &str,
    ) -> anyhow::Result<()> {
        info!(
            "command AddCamera - chat_id:{} uri:{} username:{}",
            chat_id, uri, username
        );

        let client = match create_onvif_camera_client(uri, username, password).await {
            Ok(client) => client,
            Err(err) => {
                let error = format!("cannot connect to camera at {}: {}", uri, err);
                self.send_error(&error, chat_id).await;
                anyhow::bail!(error);
            }
        };

        let snapshot_uri = client.get_snapshot_uri().await.ok();

        match self
            .repository
            .add_camera(CameraData {
                id: 0, // 0 -> nueva cámara, el repositorio le asigna id
                name: uri.to_string(),
                address: uri.to_string(),
                snapshot_uri,
                onvif_client: Arc::new(client),
                api_camera_client: None, // TODO
                device_info: None,
                subscriptors: Vec::new(),
                status: CameraStatus {
                    last_polling: None,
                    last_error: None,
                    last_error_notified: false,
                    last_notification_by_chat_id: HashMap::new(),
                    today_notifications: Vec::new(),
                },
            })
            .await
        {
            Ok(_) => {
                let message = "Camera added successfully. Use /getcameras to see its id and /setcameraname to rename it.";
                self.send_success(message, chat_id).await;
            }
            Err(err) => {
                self.send_error(&err.to_string(), chat_id).await;
                anyhow::bail!(err.to_string());
            }
        }

        Ok(())
    }

    async fn delete_camera_cmd(&self, chat_id: ChatId, camera_id: CameraId) -> anyhow::Result<()> {
        info!(
            "command DeleteCamera - chat_id:{} camera_id:{}",
            chat_id, camera_id
        );

        match self.repository.delete_camera(camera_id).await {
            Ok(_) => {
                self.send_success("Camera deleted successfully", chat_id)
                    .await
            }
            Err(err) => {
                self.send_error(&err.to_string(), chat_id).await;
                anyhow::bail!(err.to_string());
            }
        }
        Ok(())
    }

    async fn set_credentials_cmd(
        &self,
        chat_id: ChatId,
        camera_id: CameraId,
        username: &str,
        password: &str,
    ) -> anyhow::Result<()> {
        info!(
            "command SetCredentials - chat_id:{} camera_id:{} username:{}",
            chat_id, camera_id, username
        );

        let uri = match self.repository.get_camera(camera_id).await {
            Some(camera) => camera.onvif_client.get_connection_data().uri,
            None => {
                let error = format!("camera {} not found", camera_id);
                self.send_error(&error, chat_id).await;
                anyhow::bail!(error);
            }
        };

        let client = match create_onvif_camera_client(&uri, username, password).await {
            Ok(client) => client,
            Err(err) => {
                let error = format!(
                    "cannot connect to camera {} with new credentials: {}",
                    camera_id, err
                );
                self.send_error(&error, chat_id).await;
                anyhow::bail!(error);
            }
        };

        match self
            .repository
            .update_camera_credentials(camera_id, Arc::new(client), username, password)
            .await
        {
            Ok(_) => {
                self.send_success("Credentials updated successfully", chat_id)
                    .await
            }
            Err(err) => {
                self.send_error(&err.to_string(), chat_id).await;
                anyhow::bail!(err.to_string());
            }
        }
        Ok(())
    }

    async fn download_recording(
        &self,
        chat_id: ChatId,
        message_id: MessageId,
        camera_id: CameraId,
        event_time: DateTime<Utc>,
        clip_seconds: Duration,
    ) -> Result<(), DownloadRecordingError> {
        info!(
            "command Download - chat_id:{} camera_id:{} time:{}",
            chat_id, camera_id, event_time
        );

        let mut camera_data = match self.repository.get_camera(camera_id).await {
            Some(camera) => camera,
            None => {
                let error = format!("camera {} not found", camera_id);
                self.send_error(&error, chat_id).await;
                return Err(DownloadRecordingError::CameraNotFound);
            }
        };

        if camera_data.api_camera_client.is_none() {
            camera_data.api_camera_client = make_api_camera_client(&mut camera_data).await;
        }

        if let Some(api_camera_client) = camera_data.api_camera_client {
            let recordings = match api_camera_client
                .get_recordings(event_time, clip_seconds)
                .await
            {
                Ok(recordings) => recordings,
                Err(err) => {
                    tracing::error!("error getting recordings. {}", err);
                    if event_time + clip_seconds + chrono::Duration::minutes(5) > Utc::now() {
                        return Err(DownloadRecordingError::NoRecordingsYet);
                    }
                    return Err(DownloadRecordingError::NoRecordings);
                }
            };

            if recordings.is_empty() {
                tracing::error!("no recordings");
                return Err(DownloadRecordingError::NoRecordings);
            }

            tracing::info!("Found {} recording files:", recordings.len());
            recordings.iter().for_each(|rec| {
                tracing::info!(
                    " - {} ({:.2} Mb) from {} to {}",
                    rec.name,
                    rec.size_mb,
                    rec.begin,
                    rec.end
                )
            });

            let target_name = format!(
                "{}_{}_{}",
                camera_id,
                event_time.timestamp_millis(),
                rand::random::<u32>()
            );
            match api_camera_client
                .download_recording(
                    recordings.first().unwrap(),
                    event_time,
                    clip_seconds,
                    target_name,
                )
                .await
            {
                Ok(file_path) => {
                    match self
                        .notifier
                        .send_video_message(&file_path, chat_id, Some(message_id))
                        .await
                    {
                        Ok(_) => {
                            tracing::info!("deleting file {file_path:?} after send");
                            tokio::fs::remove_file(&file_path).await.ok();
                        }
                        Err(err) => {
                            tracing::error!("cannot send video message. {}", err);
                            return Err(DownloadRecordingError::ErrorSendingVideo);
                        }
                    }
                    return Ok(());
                }
                Err(err) => {
                    tracing::error!("cannot download recording. {}", err);
                    return Err(DownloadRecordingError::Other);
                }
            }
        }

        tracing::error!("no api camera client available");
        Err(DownloadRecordingError::RecordingDownloadNotAvailable)
    }
}

async fn make_api_camera_client(camera_data: &mut CameraData) -> Option<Arc<dyn ApiCameraClient>> {
    if let Ok(device_info) = camera_data.device_info().await {
        match device_info.manufacturer.as_str() {
            "Dahua" => {
                return Some(Arc::new(DahuaRpcApiCameraClient::new(
                    camera_data.host(),
                    camera_data.username(),
                    camera_data.password(),
                )))
            }
            "H264" => {
                return Some(Arc::new(DvrIpXmeyeApiCameraClient::new(
                    camera_data.host(),
                    camera_data.username(),
                    camera_data.password(),
                )))
            }
            manufacturer => {
                tracing::error!("download not implemented for this manufacturer: {manufacturer}")
            }
        }
    } else {
        tracing::error!("cannot get device info to check which camera API to use");
    }
    None
}
