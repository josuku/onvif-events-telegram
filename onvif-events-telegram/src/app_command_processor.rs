use app_core::domain::camera::CameraData;
use app_core::helpers::network::is_reachable;
use app_core::traits::discovery_client::OnvifRsDiscoveryClient;
use app_core::traits::onvif_camera_client::OnvifCameraClient;
use app_core::{
    make_caption,
    traits::{command_processor::CommandProcessor, notifier::Notifier},
    CameraId, ChatId,
};
use async_trait::async_trait;
use onvif::onvif_rs_camera_client::create_onvif_camera_client;
use onvif::onvif_rs_discovery_client::OnvifDiscoveryClient;
use repository::memory_repository::MemoryRepository;
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

    async fn print_and_send_error(&self, error: &str, chat_id: ChatId) {
        error!("{}", error);
        let _ = self
            .notifier
            .send_text_message(error.to_owned(), vec![chat_id])
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
                self.print_and_send_error(&error, chat_id).await;
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
                self.print_and_send_error(&error, chat_id).await;
                anyhow::bail!(error);
            }
        };

        _ = self
            .notifier
            .send_picture_message(
                &make_caption(
                    "Snapshot",
                    &camera.name,
                    &chrono::Utc::now(),
                    None,
                    &Vec::new(),
                ),
                snapshot.clone(),
                chat_id,
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
                let status = if reachable { "🟢" } else { "🔴" };
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
                self.notifier
                    .send_text_message(
                        "Camera name updated successfully".to_string(),
                        vec![chat_id],
                    )
                    .await;
            }
            Err(err) => {
                self.notifier
                    .send_text_message(format!("{}", err), vec![chat_id])
                    .await
            }
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
            Ok(_) => {
                self.notifier
                    .send_text_message("Subscribed successfully".to_string(), vec![chat_id])
                    .await
            }
            Err(err) => {
                self.notifier
                    .send_text_message(format!("{}", err), vec![chat_id])
                    .await
            }
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
                self.notifier
                    .send_text_message("Unsubscribed successfully".to_string(), vec![chat_id])
                    .await
            }
            Err(err) => {
                self.notifier
                    .send_text_message(format!("{}", err), vec![chat_id])
                    .await
            }
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

    async fn set_polling_time_cmd(&self, chat_id: ChatId, seconds: u64) -> anyhow::Result<()> {
        info!(
            "command SetPollingTime - chat id:{} seconds:{}",
            chat_id, seconds
        );
        self.repository.set_polling_seconds(seconds).await;
        let _ = self
            .notifier
            .send_text_message(
                "Polling time updated successfully".to_string(),
                vec![chat_id],
            )
            .await;
        Ok(())
    }

    async fn set_between_time_cmd(&self, chat_id: ChatId, seconds: u64) -> anyhow::Result<()> {
        info!(
            "command SetBetweenTime - chat id:{} seconds:{}",
            chat_id, seconds
        );
        self.repository.set_between_seconds(seconds).await;
        let _ = self
            .notifier
            .send_text_message(
                "Between time updated successfully".to_string(),
                vec![chat_id],
            )
            .await;
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
                self.print_and_send_error(&error, chat_id).await;
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
                            let message = format!("camera snapshot uri fixed:{}", fixed_uri);
                            info!("{}", message);
                            let _ = self
                                .notifier
                                .send_text_message(message, vec![chat_id])
                                .await;
                        }
                        Err(err) => {
                            let error = format!("{}", err);
                            self.print_and_send_error(&error, chat_id).await;
                            anyhow::bail!(error);
                        }
                    }
                }
                Err(err) => {
                    let error = format!("{}", err);
                    self.print_and_send_error(&error, chat_id).await;
                    anyhow::bail!(error);
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
                            let message = format!("camera snapshot uri resolved:{}", uri);
                            info!("{}", message);
                            let _ = self
                                .notifier
                                .send_text_message(message, vec![chat_id])
                                .await;
                        }
                        Err(err) => {
                            let error = format!("{}", err);
                            self.print_and_send_error(&error, chat_id).await;
                            anyhow::bail!(error);
                        }
                    }
                }
                Err(err) => {
                    let error = format!("cannot resolve snapshot uri: {}", err);
                    self.print_and_send_error(&error, chat_id).await;
                    anyhow::bail!(error);
                }
            }
        }
        Ok(())
    }

    async fn enable_daily_report_cmd(&self, chat_id: ChatId, enable: bool) -> anyhow::Result<()> {
        if enable {
            let _ = self
                .notifier
                .send_text_message("Subscribed to daily report".to_string(), vec![chat_id])
                .await;
            self.repository
                .subscribe_to_daily_report(chat_id, true)
                .await;
        } else {
            let _ = self
                .notifier
                .send_text_message("Unsubscribed from daily report".to_string(), vec![chat_id])
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
                self.print_and_send_error(&error, chat_id).await;
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
                subscriptors: Vec::new(),
            })
            .await
        {
            Ok(_) => {
                let _ = self
                    .notifier
                    .send_text_message(
                        "Camera added successfully. Use /getcameras to see its id and /setcameraname to rename it.".to_string(),
                        vec![chat_id],
                    )
                    .await;
            }
            Err(err) => {
                let error = format!("{}", err);
                self.print_and_send_error(&error, chat_id).await;
                anyhow::bail!(error);
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
                let _ = self
                    .notifier
                    .send_text_message("Camera deleted successfully".to_string(), vec![chat_id])
                    .await;
            }
            Err(err) => {
                let error = format!("{}", err);
                self.print_and_send_error(&error, chat_id).await;
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
                self.print_and_send_error(&error, chat_id).await;
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
                self.print_and_send_error(&error, chat_id).await;
                anyhow::bail!(error);
            }
        };

        match self
            .repository
            .update_camera_credentials(camera_id, Arc::new(client), username, password)
            .await
        {
            Ok(_) => {
                let _ = self
                    .notifier
                    .send_text_message(
                        "Credentials updated successfully".to_string(),
                        vec![chat_id],
                    )
                    .await;
            }
            Err(err) => {
                let error = format!("{}", err);
                self.print_and_send_error(&error, chat_id).await;
                anyhow::bail!(error);
            }
        }
        Ok(())
    }
}
