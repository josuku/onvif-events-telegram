use api_camera::{
    dahua_rpc_api_client::DahuaRpcApiCameraClient, dvrip_xmeye_client::DvrIpXmeyeApiCameraClient,
    hikvision_isapi_client::HikvisionIsapiApiCameraClient,
};
use app_core::{
    domain::{
        camera::{CameraData, CameraStatus, SnapshotMethod},
        object::{object_classes_to_string, ObjectClass},
    },
    helpers::network::is_reachable,
    make_caption,
    traits::{
        api_camera_client::ApiCameraClient, command_processor::CommandProcessor,
        command_processor::DownloadRecordingError, discovery_client::OnvifRsDiscoveryClient,
        notifier::Notifier, object_detector::ObjectDetector,
        onvif_camera_client::OnvifCameraClient,
    },
    CameraId, ChatId, MessageId,
};
use async_trait::async_trait;
use chrono::{DateTime, Duration, Utc};
use onvif::{
    onvif_rs_camera_client::create_onvif_camera_client,
    onvif_rs_discovery_client::OnvifDiscoveryClient,
    onvif_rs_service_clients::{DEFAULT_PASSWORD, DEFAULT_USERNAME},
};
use repository::memory_repository::MemoryRepository;
use std::{collections::HashMap, sync::Arc};
use tokio::sync::Mutex;
use tracing::{error, info};

pub struct AppCommandProcessor {
    repository: Arc<MemoryRepository>,
    notifier: Arc<dyn Notifier>,
    object_detector: Arc<Mutex<dyn ObjectDetector>>,
}

impl AppCommandProcessor {
    pub fn new(
        repository: Arc<MemoryRepository>,
        notifier: Arc<dyn Notifier>,
        object_detector: Arc<Mutex<dyn ObjectDetector>>,
    ) -> Self {
        Self {
            repository,
            notifier,
            object_detector,
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

    async fn get_snapshot_of_camera_id(&self, chat_id: ChatId, camera_id: CameraId) {
        let repository = self.repository.clone();
        let notifier = self.notifier.clone();

        tokio::spawn(async move {
            let camera = match repository.get_camera(camera_id).await {
                Some(camera) => camera,
                None => {
                    let error = format!("❌ cannot find camera with id: {}", camera_id);
                    notifier
                        .send_text_message(error.clone(), vec![chat_id])
                        .await;
                    tracing::error!(error);
                    return;
                }
            };

            let snapshot = match camera.get_snapshot().await {
                Ok(snapshot) => snapshot,
                Err(err) => {
                    let error = format!(
                        "❌ error getting snapshot from camera:{:?} err:{}",
                        camera.name, err
                    );
                    notifier
                        .send_text_message(error.clone(), vec![chat_id])
                        .await;
                    tracing::error!(error);
                    return;
                }
            };

            _ = notifier
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
        });
    }

    async fn get_api_camera_client(
        &self,
        camera_id: CameraId,
        chat_id: ChatId,
    ) -> Option<(CameraData, Arc<dyn ApiCameraClient>)> {
        let mut camera_data = match self.repository.get_camera(camera_id).await {
            Some(camera) => camera,
            None => {
                let error = format!("camera {} not found", camera_id);
                self.send_error(&error, chat_id).await;
                return None;
            }
        };

        if camera_data.api_camera_client.is_none() {
            camera_data.api_camera_client = make_api_camera_client(&mut camera_data).await;
        }

        match camera_data.api_camera_client.clone() {
            Some(client) => Some((camera_data, client)),
            None => {
                let error = format!("recording API not available for camera {}", camera_id);
                self.send_error(&error, chat_id).await;
                None
            }
        }
    }

    async fn resolve_clip_duration(&self, clip_seconds: Option<Duration>) -> Duration {
        match clip_seconds {
            Some(d) => d,
            None => {
                let config = self.repository.get_config().await;
                Duration::seconds(config.recording_clip as i64)
            }
        }
    }

    async fn detect_of_camera_id(&self, chat_id: ChatId, camera_id: CameraId) {
        let repository = self.repository.clone();
        let notifier = self.notifier.clone();
        let object_detector = self.object_detector.clone();

        tokio::spawn(async move {
            let camera = match repository.get_camera(camera_id).await {
                Some(camera) => camera,
                None => {
                    let error = format!("❌ cannot find camera with id: {camera_id}");
                    notifier
                        .send_text_message(error.clone(), vec![chat_id])
                        .await;
                    tracing::error!(error);
                    return;
                }
            };

            let snapshot = match camera.get_snapshot().await {
                Ok(snapshot) => snapshot,
                Err(err) => {
                    let error = format!(
                        "❌ error getting snapshot from camera:{:?} err:{}",
                        camera.name, err
                    );
                    notifier
                        .send_text_message(error.clone(), vec![chat_id])
                        .await;
                    tracing::error!(error);
                    return;
                }
            };

            let config = repository.get_config().await;
            let objects = {
                let mut detector = object_detector.lock().await;
                match detector.detect(
                    &snapshot,
                    config.detector.min_confidence,
                    &config.detector.types,
                ) {
                    Ok(objs) => objs,
                    Err(err) => {
                        tracing::error!(
                            "error detecting objects in camera {}: {}",
                            camera.name,
                            err
                        );
                        Vec::new()
                    }
                }
            };

            let caption = if objects.is_empty() {
                format!(
                    "MANUAL DETECTION\nCamera: {} (id:{})\nTime: {}\nNo objects detected",
                    camera.name,
                    camera.id,
                    chrono::Local::now().format("%Y-%m-%d %H:%M:%S %:z")
                )
            } else {
                make_caption(
                    "Manual Detection",
                    &camera.name,
                    &camera.id,
                    &chrono::Utc::now(),
                    None,
                    &objects,
                )
            };

            let _ = notifier
                .send_picture_message(
                    &caption,
                    snapshot,
                    chat_id,
                    camera_id,
                    &chrono::Utc::now(),
                )
                .await;
        });
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

    async fn get_cameras_cmd(&self, chat_id: ChatId, extended: bool) -> anyhow::Result<()> {
        info!("command GetCameras - chat_id:{}", chat_id);

        self.notifier
            .send_text_message("⏳ Start devices discovery".to_string(), vec![chat_id])
            .await;

        let discovered_devices = OnvifDiscoveryClient::camera_discovery().await;

        self.notifier
            .send_text_message(
                format!("💡 Discovered {} devices", discovered_devices.len()),
                vec![chat_id],
            )
            .await;

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
                let camera_info = if extended {
                    camera.display_full().await
                } else {
                    camera.display_short()
                };
                lines.push(format!("{status} {camera_info}"));
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
        self.repository.config_detector_types(types, chat_id).await;
        self.send_success("Detector types config updated successfully", chat_id)
            .await;
        Ok(())
    }

    async fn set_multi_channel_detection_cmd(
        &self,
        chat_id: ChatId,
        camera_id: CameraId,
        enable: bool,
    ) -> anyhow::Result<()> {
        info!(
            "command SetMultiChannelDetection - chat_id:{} camera_id:{} enable:{}",
            chat_id, camera_id, enable
        );

        match self
            .repository
            .update_multi_channel_detection(camera_id, enable)
            .await
        {
            Ok(_) => {
                self.send_success(
                    &format!(
                        "Multi-channel detection set to '{}' for camera {}",
                        enable, camera_id
                    ),
                    chat_id,
                )
                .await
            }
            Err(err) => self.send_error(&err.to_string(), chat_id).await,
        };
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

    async fn set_snapshot_method_cmd(
        &self,
        chat_id: ChatId,
        camera_id: CameraId,
        method: &str,
    ) -> anyhow::Result<()> {
        info!(
            "command SetSnapshotMethod - chat id:{} camera_id:{} method:{}",
            chat_id, camera_id, method
        );

        let snapshot_method: SnapshotMethod = match method.parse() {
            Ok(m) => m,
            Err(err) => {
                self.send_error(&err, chat_id).await;
                anyhow::bail!(err);
            }
        };

        match self
            .repository
            .update_snapshot_method(camera_id, snapshot_method)
            .await
        {
            Ok(_) => {
                self.send_success(
                    &format!(
                        "Snapshot method set to '{}' for camera {}",
                        snapshot_method, camera_id
                    ),
                    chat_id,
                )
                .await
            }
            Err(err) => self.send_error(&err.to_string(), chat_id).await,
        };
        Ok(())
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
                    if let Err(err) = self
                        .repository
                        .update_snapshot_uri_from_camera(camera_id, &snapshot_uri)
                        .await
                    {
                        self.send_error(&err.to_string(), chat_id).await;
                        anyhow::bail!(err.to_string());
                    }

                    let message = format!("Camera snapshot uri fixed:{}", fixed_uri);
                    self.send_success(&message, chat_id).await;

                    let client = match create_onvif_camera_client(
                        &camera.onvif_client.get_connection_data().uri,
                        DEFAULT_USERNAME,
                        DEFAULT_PASSWORD,
                    )
                    .await
                    {
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
                        .update_camera_credentials(
                            camera_id,
                            Arc::new(client),
                            DEFAULT_USERNAME,
                            DEFAULT_PASSWORD,
                        )
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
        let rtsp_uri = client.get_rtsp_uri().await.ok();

        let mut camera_data = CameraData {
            id: 0, // 0 -> nueva cámara, el repositorio le asigna id
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

        match self.repository.add_camera(camera_data).await {
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
                .download_recording(&recordings, event_time, clip_seconds, target_name)
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

    async fn set_camera_zoom(
        &self,
        chat_id: ChatId,
        camera_id: CameraId,
        zoom_level: u8,
    ) -> anyhow::Result<()> {
        info!(
            "command Zoom - chat_id:{} camera_id:{} zoom:{}",
            chat_id, camera_id, zoom_level
        );

        if !(1..=100).contains(&zoom_level) {
            let error = "wrong zoom level value. must be between 1-100".to_string();
            self.send_error(&error, chat_id).await;
            anyhow::bail!("{error}")
        }

        let mut camera_data = match self.repository.get_camera(camera_id).await {
            Some(camera) => camera,
            None => {
                let error = format!("camera {} not found", camera_id);
                self.send_error(&error, chat_id).await;
                anyhow::bail!("camera not found");
            }
        };

        if camera_data.api_camera_client.is_none() {
            camera_data.api_camera_client = make_api_camera_client(&mut camera_data).await;
        }

        if let Some(api_camera_client) = camera_data.api_camera_client {
            match api_camera_client.set_optical_zoom(zoom_level).await {
                Ok(_) => {
                    self.send_success(
                        &format!("camera {} zoomed successfully", camera_id),
                        chat_id,
                    )
                    .await;
                    return Ok(());
                }
                Err(err) => {
                    let error = format!("cannot zoom on camera {}", camera_id);
                    self.send_error(&error, chat_id).await;
                    anyhow::bail!("{err}")
                }
            }
        }

        anyhow::bail!("no api camera client available")
    }

    async fn get_recording_summary_cmd(
        &self,
        chat_id: ChatId,
        camera_id: CameraId,
        days: Option<u32>,
    ) -> anyhow::Result<()> {
        let days_count = days.unwrap_or(30) as i64;
        info!(
            "command GetRecordingSummary - chat_id:{chat_id} camera_id:{camera_id} days:{days_count}"
        );

        let (camera_data, api_client) = match self.get_api_camera_client(camera_id, chat_id).await {
            Some(res) => res,
            None => return Ok(()),
        };

        let now = chrono::Utc::now();
        let start_time = now - chrono::Duration::days(days_count);

        let summary = match api_client
            .get_recording_summary(Some(start_time), Some(now))
            .await
        {
            Ok(sum) => sum,
            Err(err) => {
                let error = format!("cannot get recording summary for camera {camera_id}: {err}");
                self.send_error(&error, chat_id).await;
                return Ok(());
            }
        };

        if summary.total_files == 0 {
            self.notifier
                .send_text_message(
                    format!(
                        "📹 RECORDINGS SUMMARY\nCamera: {} (id:{})\nPeriod queried: last {} days\nResult: No recordings found",
                        camera_data.name, camera_id, days_count,
                    ),
                    vec![chat_id],
                )
                .await;
            return Ok(());
        }

        let first_begin = summary.first_begin.as_deref().unwrap_or("-");
        let last_end = summary.last_end.as_deref().unwrap_or("-");

        let size_display = if summary.total_size_mb >= 1024.0 {
            format!("{:.2} GB", summary.total_size_mb / 1024.0)
        } else {
            format!("{:.2} MB", summary.total_size_mb)
        };

        let msg = format!(
            "📹 RECORDINGS SUMMARY\nCamera: {} (id:{})\nFrom: {}\nTo: {}\nTotal files: {}\nStorage used: {}",
            camera_data.name,
            camera_id,
            first_begin,
            last_end,
            summary.total_files,
            size_display,
        );

        self.notifier.send_text_message(msg, vec![chat_id]).await;
        Ok(())
    }

    async fn get_recording_list_cmd(
        &self,
        chat_id: ChatId,
        camera_id: CameraId,
        date: DateTime<Utc>,
        clip_seconds: Option<Duration>,
        page: usize,
    ) -> anyhow::Result<()> {
        info!("command GetRecordingList - chat_id:{chat_id} camera_id:{camera_id} date:{date} page:{page}");

        let (camera_data, api_client) = match self.get_api_camera_client(camera_id, chat_id).await {
            Some(res) => res,
            None => return Ok(()),
        };

        let clip = self.resolve_clip_duration(clip_seconds).await;
        let recordings = match api_client.get_recording_list(date, clip).await {
            Ok(rec) => rec,
            Err(err) => {
                let error = format!("cannot get recording list for camera {camera_id}: {err}");
                self.send_error(&error, chat_id).await;
                return Ok(());
            }
        };

        let local_date: DateTime<chrono::Local> = DateTime::from(date);
        if recordings.is_empty() {
            self.notifier
                .send_text_message(
                    format!(
                        "📹 RECORDING FILES\nCamera: {} (id:{})\nDate: {}\nResult: No recordings found in this interval",
                        camera_data.name,
                        camera_id,
                        local_date.format("%Y-%m-%d %H:%M:%S"),
                    ),
                    vec![chat_id],
                )
                .await;
            return Ok(());
        }

        let total_items = recordings.len();
        let page_size = 10;
        let total_pages = (total_items + page_size - 1) / page_size;
        let current_page = page.max(1).min(total_pages);
        let start_index = (current_page - 1) * page_size;
        let end_index = (start_index + page_size).min(total_items);

        let page_items = &recordings[start_index..end_index];

        let mut lines = vec![format!(
            "📹 RECORDING FILES\nCamera: {} (id:{})\nShowing: {}-{} of {} file(s) (Page {}/{})",
            camera_data.name,
            camera_id,
            start_index + 1,
            end_index,
            total_items,
            current_page,
            total_pages
        )];

        let mut buttons: Vec<Vec<(String, String)>> = Vec::new();

        for (idx_offset, r) in page_items.iter().enumerate() {
            let item_num = start_index + idx_offset + 1;
            let size_text = if r.size_mb >= 1024.0 {
                format!("{:.2} GB", r.size_mb / 1024.0)
            } else {
                format!("{:.2} MB", r.size_mb)
            };

            lines.push(format!(
                "{}. `{}`\n- From: {}\n- To: {}\n- Size: {}\n",
                item_num,
                r.name,
                r.begin,
                r.end,
                size_text,
            ));

            // Try to get rfc3339 timestamp from r.begin.
            // Dahua and most camera APIs return begin/end formatted as local camera time ("%Y-%m-%d %H:%M:%S").
            // We must convert it from Local timezone to Utc so that subsequent queries look for the proper time window.
            let ts_rfc3339 = if let Ok(parsed) = DateTime::parse_from_rfc3339(&r.begin) {
                parsed.with_timezone(&Utc).to_rfc3339()
            } else if let Ok(naive) =
                chrono::NaiveDateTime::parse_from_str(&r.begin, "%Y-%m-%d %H:%M:%S")
            {
                use chrono::TimeZone;
                match chrono::Local.from_local_datetime(&naive).single() {
                    Some(local_dt) => local_dt.with_timezone(&Utc).to_rfc3339(),
                    None => DateTime::<Utc>::from_naive_utc_and_offset(naive, Utc).to_rfc3339(),
                }
            } else {
                date.to_rfc3339()
            };

            // Calculate duration of the file if begin and end can be parsed
            let file_duration_secs = if let (Ok(b_naive), Ok(e_naive)) = (
                chrono::NaiveDateTime::parse_from_str(&r.begin, "%Y-%m-%d %H:%M:%S"),
                chrono::NaiveDateTime::parse_from_str(&r.end, "%Y-%m-%d %H:%M:%S"),
            ) {
                Some((e_naive - b_naive).num_seconds().max(1))
            } else if let (Ok(b_dt), Ok(e_dt)) = (
                DateTime::parse_from_rfc3339(&r.begin),
                DateTime::parse_from_rfc3339(&r.end),
            ) {
                Some((e_dt - b_dt).num_seconds().max(1))
            } else {
                None
            };

            let label = format!("⬇️ #{} ({})", item_num, size_text);
            let callback_data = match file_duration_secs {
                Some(dur) => format!("recording|{}|{}|{}", camera_id, ts_rfc3339, dur),
                None => format!("recording|{}|{}", camera_id, ts_rfc3339),
            };
            buttons.push(vec![(label, callback_data)]);
        }

        // Add pagination navigation row if there's more than 1 page
        if total_pages > 1 {
            let mut nav_row: Vec<(String, String)> = Vec::new();
            let date_rfc = date.to_rfc3339();
            let clip_str = clip_seconds
                .map(|c| c.num_seconds().to_string())
                .unwrap_or_default();

            // First page button
            if current_page > 1 {
                nav_row.push((
                    "⏮️ 1".to_string(),
                    format!("recpage|{}|{}|{}|1", camera_id, date_rfc, clip_str),
                ));
                nav_row.push((
                    format!("◀️ Prev ({})", current_page - 1),
                    format!(
                        "recpage|{}|{}|{}|{}",
                        camera_id,
                        date_rfc,
                        clip_str,
                        current_page - 1
                    ),
                ));
            }

            // Next & Last buttons
            if current_page < total_pages {
                nav_row.push((
                    format!("Next ({}) ▶️", current_page + 1),
                    format!(
                        "recpage|{}|{}|{}|{}",
                        camera_id,
                        date_rfc,
                        clip_str,
                        current_page + 1
                    ),
                ));
                nav_row.push((
                    format!("⏭️ {}", total_pages),
                    format!(
                        "recpage|{}|{}|{}|{}",
                        camera_id, date_rfc, clip_str, total_pages
                    ),
                ));
            }

            if !nav_row.is_empty() {
                buttons.push(nav_row);
            }
        }

        let text = lines.join("\n");
        if !buttons.is_empty() {
            if let Err(err) = self
                .notifier
                .send_text_message_with_buttons(text.clone(), buttons, chat_id)
                .await
            {
                tracing::warn!("could not send with buttons: {err}, falling back to plain text");
                self.notifier.send_text_message(text, vec![chat_id]).await;
            }
        } else {
            self.notifier.send_text_message(text, vec![chat_id]).await;
        }

        Ok(())
    }

    async fn download_recording_cmd(
        &self,
        chat_id: ChatId,
        camera_id: CameraId,
        date: DateTime<Utc>,
        clip_seconds: Option<Duration>,
    ) -> anyhow::Result<()> {
        info!("command DownloadRecording - chat_id:{chat_id} camera_id:{camera_id} date:{date}");

        let (_, api_client) = match self.get_api_camera_client(camera_id, chat_id).await {
            Some(res) => res,
            None => return Ok(()),
        };

        let clip = self.resolve_clip_duration(clip_seconds).await;

        self.notifier
            .send_text_message(
                format!("⏳ Searching and downloading recording for camera {camera_id}..."),
                vec![chat_id],
            )
            .await;

        let notifier = self.notifier.clone();
        tokio::spawn(async move {
            let recordings = match api_client.get_recordings(date, clip).await {
                Ok(rec) => rec,
                Err(err) => {
                    let err_msg = format!("❌ Error getting recordings for camera {camera_id}: {err}");
                    notifier.send_text_message(err_msg, vec![chat_id]).await;
                    return;
                }
            };

            if recordings.is_empty() {
                let msg = format!("❌ No recordings found for camera {camera_id} at requested date");
                notifier.send_text_message(msg, vec![chat_id]).await;
                return;
            }

            let target_name = format!(
                "{}_{}_{}",
                camera_id,
                date.timestamp_millis(),
                rand::random::<u32>()
            );

            match api_client
                .download_recording(&recordings, date, clip, target_name)
                .await
            {
                Ok(file_path) => {
                    match notifier.send_video_message(&file_path, chat_id, None).await {
                        Ok(_) => {
                            tracing::info!("deleting file {file_path:?} after send");
                            tokio::fs::remove_file(&file_path).await.ok();
                        }
                        Err(err) => {
                            tracing::error!("cannot send video message: {err}");
                            notifier
                                .send_text_message(
                                    format!("❌ Error sending video: {err}"),
                                    vec![chat_id],
                                )
                                .await;
                        }
                    }
                }
                Err(err) => {
                    tracing::error!("cannot download recording: {err}");
                    notifier
                        .send_text_message(
                            format!("❌ Error downloading recording: {err}"),
                            vec![chat_id],
                        )
                        .await;
                }
            }
        });

        Ok(())
    }

    async fn manual_detection_cmd(
        &self,
        chat_id: ChatId,
        camera_id: Option<CameraId>,
    ) -> anyhow::Result<()> {
        info!("command Detect - chat_id:{chat_id} camera_id:{camera_id:?}");

        if let Some(camera_id) = camera_id {
            self.detect_of_camera_id(chat_id, camera_id).await;
        } else {
            let cameras = self.repository.get_sorted_cameras().await;
            if cameras.is_empty() {
                self.notifier
                    .send_text_message("No available cameras".to_string(), vec![chat_id])
                    .await;
                return Ok(());
            }
            for camera in cameras {
                self.detect_of_camera_id(chat_id, camera.id).await;
            }
        }
        Ok(())
    }

    async fn config_manual_detection_cmd(
        &self,
        chat_id: ChatId,
        enable: bool,
        interval_seconds: Option<u64>,
        types: Option<Vec<ObjectClass>>,
    ) -> anyhow::Result<()> {
        info!(
            "command ConfigManualDetection - chat_id:{chat_id} enable:{enable} interval:{interval_seconds:?} types:{types:?}"
        );

        self.repository
            .config_manual_detection(enable, interval_seconds, types, chat_id)
            .await;

        let config = self.repository.get_config().await;
        if enable {
            let types_str = if config.manual_detection.types.is_empty() {
                if config.detector.types.is_empty() {
                    "all configured objects".to_string()
                } else {
                    object_classes_to_string(&config.detector.types)
                }
            } else {
                object_classes_to_string(&config.manual_detection.types)
            };

            self.send_success(
                &format!(
                    "Manual background detection ENABLED (every {}s, watching: {})",
                    config.manual_detection.interval_seconds, types_str
                ),
                chat_id,
            )
            .await;
        } else {
            self.send_success("Manual background detection DISABLED", chat_id)
                .await;
        }
        Ok(())
    }

    async fn get_manual_detection_status_cmd(&self, chat_id: ChatId) -> anyhow::Result<()> {
        let config = self.repository.get_config().await;
        let status_str = if config.manual_detection.enable {
            "ENABLED ✅"
        } else {
            "DISABLED ❌"
        };
        let types_str = if config.manual_detection.types.is_empty() {
            if config.detector.types.is_empty() {
                "(defaults to all/detector types)".to_string()
            } else {
                format!(
                    "{} (inherited from detector)",
                    object_classes_to_string(&config.detector.types)
                )
            }
        } else {
            object_classes_to_string(&config.manual_detection.types)
        };

        let msg = format!(
            "🔍 MANUAL BACKGROUND DETECTION STATUS\n• Status: {}\n• Interval: {} seconds\n• Target objects: {}\n\nUsage:\n/manualdetection enable 30s person car dog\n/manualdetection disable",
            status_str, config.manual_detection.interval_seconds, types_str
        );
        self.notifier.send_text_message(msg, vec![chat_id]).await;
        Ok(())
    }
}

async fn make_api_camera_client(camera_data: &mut CameraData) -> Option<Arc<dyn ApiCameraClient>> {
    match camera_data.get_device_info().await {
        Some(device_info) => {
            camera_data.device_info = Some(device_info.clone());
            match device_info.manufacturer.as_str() {
                "Dahua" => Some(Arc::new(DahuaRpcApiCameraClient::new(
                    camera_data.host(),
                    camera_data.username(),
                    camera_data.password(),
                ))),
                "H264" => Some(Arc::new(DvrIpXmeyeApiCameraClient::new(
                    camera_data.host(),
                    camera_data.username(),
                    camera_data.password(),
                ))),
                "Interlogix" | "TVT" | "Hikvision" => {
                    let fake_utc = device_info.model.contains("TVF-1103");
                    Some(Arc::new(HikvisionIsapiApiCameraClient::new(
                        camera_data.host(),
                        camera_data.username(),
                        camera_data.password(),
                        fake_utc,
                    )))
                }
                manufacturer => {
                    tracing::error!(
                        "download not implemented for this manufacturer: {manufacturer}"
                    );
                    None
                }
            }
        }
        None => {
            tracing::error!("cannot get device info to check which camera API to use");
            None
        }
    }
}
