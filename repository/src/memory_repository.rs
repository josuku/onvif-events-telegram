use anyhow::bail;
use app_core::{CameraId, ChatId, domain::discovery_device::DiscoveryDevice};
use chrono::Utc;
use onvif::onvif_camera_client::create_onvif_camera_client;
use std::{collections::HashMap, fmt, sync::Arc};
use tokio::sync::Mutex;
use tracing::{error, info, warn};
use url::Url;

use super::db_store::DbStore;
use app_core::traits::camera_client::CameraClient;

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

pub struct MemoryRepository {
    cameras: Mutex<HashMap<CameraId, CameraData>>,
    polling_seconds: Mutex<u64>,
    between_seconds: Mutex<u64>,
    repo_store: Arc<DbStore>,
    #[allow(clippy::type_complexity)]
    last_notifications: Mutex<HashMap<(CameraId, ChatId), Option<chrono::DateTime<Utc>>>>,
    today_notifications: Mutex<HashMap<CameraId, Vec<chrono::DateTime<Utc>>>>,
    daily_report_subscriptors: Mutex<Vec<ChatId>>,
    last_polling: Mutex<HashMap<CameraId, chrono::DateTime<Utc>>>,
}

impl MemoryRepository {
    pub fn new(polling_seconds: u64, between_seconds: u64, repo_store: Arc<DbStore>) -> Self {
        Self {
            cameras: Mutex::new(HashMap::new()),
            polling_seconds: Mutex::new(polling_seconds),
            between_seconds: Mutex::new(between_seconds),
            repo_store,
            last_notifications: Mutex::new(HashMap::new()),
            today_notifications: Mutex::new(HashMap::new()),
            daily_report_subscriptors: Mutex::new(Vec::new()),
            last_polling: Mutex::new(HashMap::new()),
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

        for camera in cameras {
            let client =
                create_onvif_camera_client(&camera.uri, &camera.username, &camera.password)
                    .await
                    .map_err(|err| anyhow::anyhow!({ err }))?;

            self.add_camera(CameraData {
                id: camera.id,
                name: camera.name,
                address: camera.address,
                snapshot_uri: camera.snapshot_uri,
                client: Arc::new(client),
                subscriptors: Vec::new(),
            })
            .await?;

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
            Err(err) => {
                error!("cannot load daily report subscriptors: {}", err);
            }
        };

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

    pub async fn get_polling_seconds(&self) -> u64 {
        let polling_seconds = self.polling_seconds.lock().await;
        *polling_seconds
    }

    pub async fn get_between_seconds(&self) -> u64 {
        let between_seconds = self.between_seconds.lock().await;
        *between_seconds
    }

    pub async fn get_last_notification_time(
        &self,
        camera_id: CameraId,
        chat_id: ChatId,
    ) -> Option<chrono::DateTime<Utc>> {
        let last_notifications = self.last_notifications.lock().await;
        match last_notifications.get(&(camera_id, chat_id)) {
            Some(time) => *time,
            None => None,
        }
    }

    pub async fn update_last_notification_time(&self, camera_id: CameraId, chat_id: ChatId) {
        let mut last_notifications = self.last_notifications.lock().await;
        let now = chrono::Utc::now();
        last_notifications.insert((camera_id, chat_id), Some(now));
        self.update_today_notification(camera_id, now).await;
    }

    pub async fn get_today_camera_notifications(
        &self,
        camera_id: CameraId,
    ) -> Vec<chrono::DateTime<Utc>> {
        let today_notifications = self.today_notifications.lock().await;
        if let Some(camera_notifications) = today_notifications.get(&camera_id) {
            return camera_notifications.to_vec();
        }
        Vec::new()
    }

    async fn update_today_notification(&self, camera_id: CameraId, now: chrono::DateTime<Utc>) {
        let mut today_notifications = self.today_notifications.lock().await;
        match today_notifications.get_mut(&camera_id) {
            Some(camera_notifications) => camera_notifications.push(now),
            None => {
                today_notifications.insert(camera_id, vec![now]);
            }
        }
    }

    pub async fn clear_today_notifications(&self) {
        let mut today_notifications = self.today_notifications.lock().await;
        today_notifications.clear();
    }

    pub async fn get_last_polling_from_camera(
        &self,
        camera_id: CameraId,
    ) -> Option<chrono::DateTime<Utc>> {
        let last_polling = self.last_polling.lock().await;
        last_polling.get(&camera_id).copied()
    }

    pub async fn update_last_polling_from_camera(
        &self,
        camera_id: CameraId,
        now: chrono::DateTime<Utc>,
    ) {
        let mut last_polling = self.last_polling.lock().await;
        last_polling.insert(camera_id, now);
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
                &camera.client.get_connection_data(),
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

    pub async fn set_polling_seconds(&self, seconds: u64) {
        let mut polling_seconds = self.polling_seconds.lock().await;
        *polling_seconds = seconds;
    }

    pub async fn set_between_seconds(&self, seconds: u64) {
        let mut between_seconds = self.between_seconds.lock().await;
        *between_seconds = seconds;
    }

    pub async fn update_uri_from_camera(
        &self,
        camera_id: CameraId,
        uri: &str,
    ) -> anyhow::Result<()> {
        let mut cameras = self.cameras.lock().await;
        match cameras.get_mut(&camera_id) {
            Some(camera) => {
                let client = create_onvif_camera_client(
                    uri,
                    &camera.client.get_connection_data().username,
                    &camera.client.get_connection_data().password,
                )
                .await
                .map_err(|err| anyhow::anyhow!({ err }))?;
                camera.client = Arc::new(client);
            }
            None => bail!("cannot find camera {} to replace uri", camera_id),
        }
        Ok(())
    }

    pub async fn replace_camera_client(
        &self,
        camera_id: CameraId,
        camera_client: Arc<dyn CameraClient>,
    ) -> anyhow::Result<()> {
        let mut cameras = self.cameras.lock().await;
        match cameras.get_mut(&camera_id) {
            Some(camera) => {
                camera.client = camera_client;
            }
            None => bail!("cannot find camera {} to replace camera client", camera_id),
        }
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
            if !current_cameras
                .iter()
                .any(|camera| camera.address == new_device.address)
            {
                let mut uri = "".to_string();
                if !new_device.urls.is_empty()
                    && let Some(url) = new_device.urls.first()
                {
                    uri = make_uri(url);
                }

                // TODO user-pass empty by default
                let mut client = create_onvif_camera_client(&uri, "", "")
                    .await
                    .map_err(|err| anyhow::anyhow!({ err }))?;

                let snapshot_uri = client.get_snapshot_uri().await.ok();

                if let Some((user, password)) = extract_credentials(snapshot_uri.clone()) {
                    info!(
                        "Using user and password extracted from snapshot uri:{:?}",
                        snapshot_uri
                    );

                    client = create_onvif_camera_client(&uri, &user, &password)
                        .await
                        .map_err(|err| anyhow::anyhow!({ err }))?;
                }

                if let Err(err) = self
                    .add_camera(CameraData {
                        id: 0, // new camera, insert into store
                        name: new_device.name.clone().unwrap_or_default(),
                        address: new_device.address.clone(),
                        snapshot_uri,
                        client: Arc::new(client),
                        subscriptors: Vec::new(),
                    })
                    .await
                {
                    bail!("{}", err);
                }
            } else if let Some(camera) = current_cameras
                .iter()
                .find(|cam| cam.address == new_device.address)
                && let Some(new_url) = new_device.urls.first()
            {
                let new_uri = make_uri(new_url);
                let prev_conn_data = camera.client.get_connection_data();
                if prev_conn_data.uri != new_uri {
                    warn!(
                        "Updating host of camera:{} -> prev:{} new:{}",
                        camera.id, prev_conn_data.uri, new_url
                    );
                    let _ = self.update_uri_from_camera(camera.id, &new_uri).await;

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
    format!(
        "http://{}:{}",
        url.host_str().unwrap_or_default(),
        url.port().unwrap_or_default(),
    )
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
