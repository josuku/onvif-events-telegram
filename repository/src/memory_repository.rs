use anyhow::bail;
use app_core::{
    CameraId, ChatId,
    domain::{
        camera::{CameraConnectionData, CameraData, CameraStatus},
        config::BotConfig,
        discovery_device::DiscoveryDevice,
        object::ObjectClass,
    },
};
use chrono::{DateTime, Utc};
use onvif::onvif_rs_camera_client::create_onvif_camera_client;
use std::{collections::HashMap, sync::Arc, time::Duration};
use tokio::{sync::Mutex, time::timeout};
use tracing::{error, info, warn};
use url::Url;

use super::db_store::DbStore;
use app_core::traits::onvif_camera_client::OnvifCameraClient;

pub struct MemoryRepository {
    cameras: Mutex<HashMap<CameraId, CameraData>>,
    config: Mutex<BotConfig>,
    default_config: Mutex<BotConfig>,
    repo_store: Arc<DbStore>,
    daily_report_subscriptors: Mutex<Vec<ChatId>>,
}

impl MemoryRepository {
    pub fn new(bot_config: BotConfig, repo_store: Arc<DbStore>) -> Self {
        Self {
            cameras: Mutex::new(HashMap::new()),
            config: Mutex::new(bot_config.clone()),
            default_config: Mutex::new(bot_config),
            repo_store,
            daily_report_subscriptors: Mutex::new(Vec::new()),
        }
    }

    pub async fn load_from_store(&self) -> anyhow::Result<()> {
        let cameras = match self.repo_store.get_cameras() {
            Ok(cameras) => cameras,
            Err(err) => {
                error!("cannot load cameras: {}", err);
                Vec::new()
            }
        };

        info!("Creating onvif camera clients for every camera...");

        for camera in cameras {
            let client = Arc::new(
                create_onvif_camera_client(&camera.uri, &camera.username, &camera.password)
                    .await
                    .map_err(|err| anyhow::anyhow!({ err }))?,
            );

            let mut camera_data = CameraData {
                id: camera.id,
                name: camera.name,
                address: camera.address,
                snapshot_uri: camera.snapshot_uri,
                onvif_client: client,
                api_camera_client: None,
                device_info: None,
                subscriptors: Vec::new(),
                status: CameraStatus {
                    last_polling: None,
                    last_error: None,
                    last_error_notified: false,
                    last_notification_by_chat_id: HashMap::new(),
                    today_notifications: Vec::new(),
                },
            };
            camera_data.device_info = camera_data.get_device_info().await;

            self.add_camera(camera_data).await?;

            for subscriptor in camera.subscriptors {
                let _ = self
                    .subscribe_to_camera(camera.id, subscriptor, false)
                    .await;
            }
        }

        match self.repo_store.get_daily_report_subscriptors() {
            Ok(subscriptors) => {
                for chat_id in subscriptors {
                    let _ = self.subscribe_to_daily_report(chat_id, false).await;
                }
            }
            Err(err) => error!("cannot load daily report subscriptors: {}", err),
        };

        match self.repo_store.get_config() {
            Ok(repo_config) => {
                if let Some(repo_config) = repo_config {
                    let mut config = self.config.lock().await;
                    info!("found db config. loading... {:?}", repo_config);
                    *config = repo_config;
                } else {
                    warn!("not found db config. loading config.yaml default values");
                }
            }
            Err(err) => error!("cannot load db config: {}", err),
        }

        Ok(())
    }

    pub async fn get_cameras(&self) -> Vec<CameraData> {
        let cameras = self.cameras.lock().await;
        cameras.values().cloned().collect()
    }

    pub async fn get_sorted_cameras(&self) -> Vec<CameraData> {
        let cameras = self.cameras.lock().await;
        let mut vec: Vec<(i64, CameraData)> = cameras.clone().into_iter().collect();
        vec.sort_by_key(|a| a.0);
        vec.into_iter().map(|(_, camera)| camera).collect()
    }

    pub async fn get_camera(&self, camera_id: CameraId) -> Option<CameraData> {
        let cameras = self.cameras.lock().await;
        cameras.get(&camera_id).cloned()
    }

    pub async fn get_last_notification_time(
        &self,
        camera_id: CameraId,
        chat_id: ChatId,
    ) -> Option<chrono::DateTime<Utc>> {
        let cameras = self.cameras.lock().await;
        if let Some(camera) = cameras.get(&camera_id) {
            camera
                .status
                .last_notification_by_chat_id
                .get(&chat_id)
                .cloned()
        } else {
            error!("camera {} not found", camera_id);
            None
        }
    }

    pub async fn update_last_notification_time(&self, camera_id: CameraId, chat_id: ChatId) {
        let mut cameras = self.cameras.lock().await;
        if let Some(camera) = cameras.get_mut(&camera_id) {
            let now = chrono::Utc::now();
            camera
                .status
                .last_notification_by_chat_id
                .insert(chat_id, now);
            camera.status.today_notifications.push(now);
        } else {
            error!("camera {} not found", camera_id);
        }
    }

    pub async fn clear_today_notifications(&self) {
        let mut cameras = self.cameras.lock().await;
        for (_, camera) in cameras.iter_mut() {
            camera.status.today_notifications.clear();
        }
    }

    pub async fn update_last_polling_from_camera(
        &self,
        camera_id: CameraId,
        now: chrono::DateTime<Utc>,
    ) {
        let mut cameras = self.cameras.lock().await;
        if let Some(camera) = cameras.get_mut(&camera_id) {
            camera.status.last_polling = Some(now);
        } else {
            error!("camera {} not found", camera_id)
        }
    }

    pub async fn add_camera(&self, mut camera: CameraData) -> anyhow::Result<()> {
        let mut cameras = self.cameras.lock().await;
        for (_, cam) in cameras.iter() {
            if camera.id == cam.id || camera.address == cam.address {
                bail!("camera {} already registered", camera.id);
            }
        }

        camera.id = if camera.id == 0 {
            match self.repo_store.insert_camera(
                &camera.name,
                &camera.address,
                &camera.onvif_client.get_connection_data(),
                &camera.snapshot_uri,
            ) {
                Ok(id) => id,
                Err(err) => bail!("{}", err),
            }
        } else {
            camera.id
        };
        cameras.insert(camera.id, camera);
        Ok(())
    }

    pub async fn delete_camera(&self, camera_id: CameraId) -> anyhow::Result<()> {
        let mut cameras = self.cameras.lock().await;
        if cameras.remove(&camera_id).is_none() {
            bail!("camera {} not found", camera_id);
        }
        self.repo_store.delete_camera(camera_id);
        Ok(())
    }

    pub async fn subscribe_to_camera(
        &self,
        camera_id: i64,
        chat_id: ChatId,
        update_store: bool,
    ) -> anyhow::Result<()> {
        let mut cameras = self.cameras.lock().await;
        if let Some(camera) = cameras.get_mut(&camera_id) {
            if camera.subscriptors.contains(&chat_id) {
                bail!("already subscribed to this camera");
            }

            camera.subscriptors.push(chat_id);

            if update_store {
                self.repo_store
                    .insert_camera_subscription(camera_id, chat_id);
            }

            Ok(())
        } else {
            bail!("camera {} not found", camera_id)
        }
    }

    pub async fn unsubscribe_from_camera(
        &self,
        camera_id: CameraId,
        chat_id: ChatId,
        update_store: bool,
    ) -> anyhow::Result<()> {
        let mut cameras = self.cameras.lock().await;
        if let Some(camera) = cameras.get_mut(&camera_id) {
            if !camera.subscriptors.contains(&chat_id) {
                bail!("not subscribed to this camera");
            }

            camera.subscriptors.retain(|cid| *cid != chat_id);

            if update_store {
                self.repo_store
                    .remove_camera_subscription(camera_id, chat_id);
            }

            Ok(())
        } else {
            bail!("camera {} not found", camera_id)
        }
    }

    pub async fn set_camera_name(&self, camera_id: i64, camera_name: &str) -> anyhow::Result<()> {
        let mut cameras = self.cameras.lock().await;
        if let Some(camera) = cameras.get_mut(&camera_id) {
            camera.name = camera_name.to_string();
            self.repo_store
                .update_name_from_camera(camera_id, camera_name);
            Ok(())
        } else {
            bail!("camera {} not found", camera_id)
        }
    }

    pub async fn set_camera_last_error(
        &self,
        camera_id: CameraId,
        last_error: Option<DateTime<Utc>>,
        notified: bool,
    ) {
        let mut cameras = self.cameras.lock().await;
        if let Some(camera) = cameras.get_mut(&camera_id) {
            camera.status.last_error = last_error;
            camera.status.last_error_notified = notified;
        } else {
            error!("camera {} not found", camera_id)
        }
    }

    pub async fn get_config(&self) -> BotConfig {
        let config = self.config.lock().await;
        config.clone()
    }

    pub async fn reset_config(&self) {
        let mut config = self.config.lock().await;
        let default_config = self.default_config.lock().await;
        let _ = self.repo_store.get_config();
        *config = default_config.clone()
    }

    pub async fn config_polling_seconds(&self, seconds: u64, chat_id: ChatId) {
        let mut config = self.config.lock().await;
        config.polling_seconds = seconds;
        self.repo_store.update_config(chat_id, &config);
    }

    pub async fn config_between_seconds(&self, seconds: u64, chat_id: ChatId) {
        let mut config = self.config.lock().await;
        config.between_seconds = seconds;
        self.repo_store.update_config(chat_id, &config);
    }

    pub async fn config_send_errors(&self, value: bool, chat_id: ChatId) {
        let mut config = self.config.lock().await;
        config.send_errors = value;
        self.repo_store.update_config(chat_id, &config);
    }

    pub async fn config_auto_renewal(&self, value: bool, chat_id: ChatId) {
        let mut config = self.config.lock().await;
        config.auto_renewal = value;
        self.repo_store.update_config(chat_id, &config);
    }

    pub async fn config_recording_clip(&self, seconds: u64, chat_id: ChatId) {
        let mut config = self.config.lock().await;
        config.recording_clip = seconds;
        self.repo_store.update_config(chat_id, &config);
    }

    pub async fn config_detector_enable(&self, value: bool, chat_id: ChatId) {
        let mut config = self.config.lock().await;
        config.detector.enable = value;
        self.repo_store.update_config(chat_id, &config);
    }

    pub async fn config_detector_min_confidence(&self, value: f32, chat_id: ChatId) {
        let mut config = self.config.lock().await;
        config.detector.min_confidence = value;
        self.repo_store.update_config(chat_id, &config);
    }

    pub async fn config_detector_types(&self, types: Vec<ObjectClass>, chat_id: ChatId) {
        let mut config = self.config.lock().await;
        config.detector.types = types;
        self.repo_store.update_config(chat_id, &config);
    }

    pub async fn get_detector_types(&self) -> Vec<ObjectClass> {
        let default_config = self.default_config.lock().await;
        default_config.detector.types.clone()
    }

    pub async fn update_uri_from_camera(
        &self,
        camera_id: CameraId,
        new_uri: &str,
        prev_conn_data: CameraConnectionData,
    ) -> anyhow::Result<()> {
        let new_client =
            create_onvif_camera_client(new_uri, &prev_conn_data.username, &prev_conn_data.password)
                .await
                .map_err(|err| anyhow::anyhow!({ err }))?;

        self.replace_camera_client(camera_id, Arc::new(new_client))
            .await?;

        Ok(())
    }

    pub async fn update_camera_credentials(
        &self,
        camera_id: CameraId,
        client: Arc<dyn OnvifCameraClient>,
        username: &str,
        password: &str,
    ) -> anyhow::Result<()> {
        let mut cameras = self.cameras.lock().await;
        match cameras.get_mut(&camera_id) {
            Some(camera) => {
                camera.onvif_client = client;
                self.repo_store
                    .update_credentials_from_camera(camera_id, username, password);
            }
            None => bail!("cannot find camera {} to update credentials", camera_id),
        }
        Ok(())
    }

    pub async fn replace_camera_client(
        &self,
        camera_id: CameraId,
        new_client: Arc<dyn OnvifCameraClient>,
    ) -> anyhow::Result<()> {
        let old_client = {
            let mut cameras: tokio::sync::MutexGuard<'_, HashMap<i64, CameraData>> =
                self.cameras.lock().await;
            match cameras.get_mut(&camera_id) {
                Some(camera) => {
                    tracing::info!(
                        "UPDATING camera:{} from ip:{} to ip:{}",
                        camera_id,
                        camera.onvif_client.get_connection_data().uri,
                        new_client.get_connection_data().uri
                    );
                    if camera.onvif_client.get_connection_data() == new_client.get_connection_data()
                        && camera.status.last_error.is_none()
                    {
                        tracing::warn!(
                            "trying to replace camera client with same data for camera {}",
                            camera_id
                        );
                        return Ok(());
                    }
                    let old_client = camera.onvif_client.clone();
                    camera.onvif_client = new_client.clone();
                    self.repo_store
                        .update_uri_from_camera(camera_id, &new_client.get_connection_data().uri);
                    old_client
                }
                None => bail!("cannot find camera {} to replace camera client", camera_id),
            }
        };
        let _ = timeout(Duration::from_secs(5), old_client.unsubscribe()).await;
        Ok(())
    }

    pub async fn update_snapshot_uri_from_camera(
        &self,
        camera_id: CameraId,
        snapshot_uri: &str,
    ) -> anyhow::Result<()> {
        let mut cameras = self.cameras.lock().await;
        match cameras.get_mut(&camera_id) {
            Some(camera) => {
                camera.snapshot_uri = Some(snapshot_uri.to_string());
                self.repo_store
                    .update_snapshot_uri_from_camera(camera_id, snapshot_uri);
            }
            None => bail!("cannot find camera {}", camera_id),
        }
        Ok(())
    }

    pub async fn update_repository_cameras(
        &self,
        new_devices: &[DiscoveryDevice],
    ) -> anyhow::Result<()> {
        let current_cameras = self.get_cameras().await;
        for new_device in new_devices {
            let mut new_uri = "".to_string();
            if !new_device.urls.is_empty()
                && let Some(url) = new_device.urls.first()
            {
                new_uri = make_uri(url);
            }

            if !current_cameras.iter().any(|camera| {
                camera.address == new_device.address
                    || camera.onvif_client.get_connection_data().uri == new_uri
            }) {
                // TODO user-pass empty by default
                let mut client = create_onvif_camera_client(&new_uri, "", "")
                    .await
                    .map_err(|err| anyhow::anyhow!({ err }))?;

                let snapshot_uri = client.get_snapshot_uri().await.ok();

                if let Some((user, password)) = extract_credentials(snapshot_uri.clone()) {
                    info!(
                        "Using user and password extracted from snapshot uri:{:?}",
                        snapshot_uri
                    );

                    client = create_onvif_camera_client(&new_uri, &user, &password)
                        .await
                        .map_err(|err| anyhow::anyhow!({ err }))?;
                }

                let mut camera_data = CameraData {
                    id: 0, // new camera, insert into store
                    name: new_device.name.clone().unwrap_or_default(),
                    address: new_device.address.clone(),
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
                };
                camera_data.device_info = camera_data.get_device_info().await;

                if let Err(err) = self.add_camera(camera_data).await {
                    bail!("{}", err);
                }
            } else if let Some(camera) = current_cameras.iter().find(|cam| {
                cam.address == new_device.address
                    || cam.onvif_client.get_connection_data().uri == new_uri
            }) && let Some(new_url) = new_device.urls.first()
            {
                warn!(
                    "Camera with address '{}' or uri '{}' already exists",
                    new_device.address, new_uri
                );

                let prev_conn_data = camera.onvif_client.get_connection_data();
                if prev_conn_data.uri != new_uri {
                    warn!(
                        "Updating host of camera:{} -> prev:{} new:{}",
                        camera.id, prev_conn_data.uri, new_url
                    );
                    let _ = self
                        .update_uri_from_camera(camera.id, &new_uri, prev_conn_data)
                        .await;

                    if let Some(snapshot_uri) = &camera.snapshot_uri
                        && let Ok(prev_url) = Url::parse(snapshot_uri)
                    {
                        let prev_host = prev_url.host_str().unwrap_or_default();
                        let new_host = new_url.host_str().unwrap_or_default();
                        let new_snapshot_uri = snapshot_uri.replace(prev_host, new_host);
                        let _ = self
                            .update_snapshot_uri_from_camera(camera.id, &new_snapshot_uri)
                            .await;
                    }
                }
            }
        }
        Ok(())
    }

    pub async fn subscribe_to_daily_report(&self, chat_id: ChatId, update_store: bool) {
        let mut subscriptors = self.daily_report_subscriptors.lock().await;
        subscriptors.push(chat_id);

        if update_store {
            self.repo_store.insert_daily_report_subscription(chat_id);
        }
    }

    pub async fn unsubscribe_from_daily_report(&self, chat_id: ChatId, update_store: bool) {
        let mut subscriptors = self.daily_report_subscriptors.lock().await;
        subscriptors.retain(|&s| s != chat_id);

        if update_store {
            self.repo_store.remove_daily_report_subscription(chat_id);
        }
    }

    pub async fn get_daily_report_subscriptors(&self) -> Vec<ChatId> {
        let subscriptors = self.daily_report_subscriptors.lock().await;
        subscriptors.to_vec()
    }
}

fn make_uri(url: &Url) -> String {
    if let Some(port) = url.port() {
        format!("http://{}:{}", url.host_str().unwrap_or_default(), port,)
    } else {
        format!("http://{}", url.host_str().unwrap_or_default(),)
    }
}

fn extract_credentials(input: Option<String>) -> Option<(String, String)> {
    if let Some(input) = input {
        let url = Url::parse(&input).ok()?;

        let user = url
            .query_pairs()
            .find(|(k, _)| k == "user")
            .map(|(_, v)| v.to_string())?;

        let password = url
            .query_pairs()
            .find(|(k, _)| k == "password")
            .map(|(_, v)| v.to_string())?;

        return Some((user, password));
    }
    None
}
