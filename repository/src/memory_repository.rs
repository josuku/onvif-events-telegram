use anyhow::bail;
use app_core::{CameraId, ChatId};
use chrono::Utc;
use log::error;
use onvif::{onvif_camera::OnvifCamera, onvif_clients::DiscoveryDevice};
use std::{collections::HashMap, fmt, sync::Arc};
// use teloxide::types::ChatId;
use tokio::sync::Mutex;
use url::Url;

use super::db_store::DbStore;

#[derive(Clone)]
pub struct Camera {
    pub id: CameraId,
    pub name: String,
    pub address: String,
    pub snapshot_uri: Option<String>,
    pub client: OnvifCamera,
    pub subscriptors: Vec<ChatId>,
}
impl fmt::Display for Camera {
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
            self.client.uri,
            self.address,
            self.snapshot_uri.clone().unwrap_or_default(),
            self.subscriptors.len(),
        )
    }
}

pub struct MemoryRepository {
    cameras: Mutex<HashMap<CameraId, Camera>>,
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
            let mut client =
                match OnvifCamera::new(&camera.uri, &camera.username, &camera.password).await {
                    Ok(cli) => cli,
                    Err(err) => {
                        anyhow::bail!("cannot create OnvifCamera:{}", err);
                    }
                };
            client.init().await;

            self.add_camera(Camera {
                id: camera.id,
                name: camera.name,
                address: camera.address,
                snapshot_uri: camera.snapshot_uri,
                client,
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

    pub async fn get_cameras(&self) -> Vec<Camera> {
        let cameras = self.cameras.lock().await;
        cameras.values().cloned().collect()
    }

    pub async fn get_sorted_cameras(&self) -> Vec<Camera> {
        let cameras = self.cameras.lock().await;
        let mut vec: Vec<(i64, Camera)> = cameras.clone().into_iter().collect();
        vec.sort_by_key(|a| a.0);
        vec.into_iter().map(|(_, camera)| camera).collect()
    }

    pub async fn get_camera(&self, camera_id: CameraId) -> Option<Camera> {
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

    pub async fn add_camera(&self, mut camera: Camera) -> anyhow::Result<()> {
        let mut cameras = self.cameras.lock().await;
        for (_, cam) in cameras.iter() {
            if camera.id == cam.id || camera.address == cam.address {
                bail!("camera {} already registered", camera.id);
            }
        }

        camera.id = if camera.id == 0 {
            match self.repo_store.insert_camera(
                &camera.name,
                &camera.client.uri,
                &camera.address,
                &camera.client.credentials.username,
                &camera.client.credentials.password,
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
                camera.client.uri = uri.to_string();
                self.repo_store.update_uri_from_camera(camera_id, uri);
            }
            None => bail!("cannot find camera {}", camera_id),
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

                let mut client = match OnvifCamera::new(
                    &uri, "", "", // TODO user-pass empty by default
                )
                .await
                {
                    Ok(cli) => cli,
                    Err(err) => {
                        bail!("cannot create OnvifCamera:{}", err);
                    }
                };
                client.init().await;

                let snapshot_uri = client.get_snapshot_uri().await.ok();

                if let Err(err) = self
                    .add_camera(Camera {
                        id: 0, // new camera, insert into store
                        name: new_device.name.clone().unwrap_or_default(),
                        address: new_device.address.clone(),
                        snapshot_uri,
                        client,
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
                if camera.client.uri != new_uri {
                    println!(
                        "Updating host of camera:{} -> prev:{} new:{}",
                        camera.id, camera.client.uri, new_url
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
