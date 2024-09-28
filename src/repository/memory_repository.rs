use crate::{onvif::onvif_camera::OnvifCamera, CameraId};
use anyhow::bail;
use log::error;
use onvif::discovery::Device;
use std::{collections::HashMap, fmt, sync::Arc};
use teloxide::types::ChatId;
use tokio::sync::Mutex;

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
    repo_store: Arc<DbStore>,
}
impl MemoryRepository {
    pub fn new(polling_seconds: u64, repo_store: Arc<DbStore>) -> Self {
        Self {
            cameras: Mutex::new(HashMap::new()),
            polling_seconds: Mutex::new(polling_seconds),
            repo_store,
        }
    }

    pub async fn init(&self) -> anyhow::Result<()> {
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
                        bail!("cannot create OnvifCamera:{}", err);
                    }
                };
            client.init().await;

            let _ = self
                .add_camera(Camera {
                    id: camera.id,
                    name: camera.name,
                    address: camera.address,
                    snapshot_uri: camera.snapshot_uri,
                    client,
                    subscriptors: Vec::new(),
                })
                .await;

            for subscriptor in camera.subscriptors {
                let _ = self
                    .subscribe_to_camera(camera.id, subscriptor, false)
                    .await;
            }
        }

        Ok(())
    }

    pub async fn get_cameras(&self) -> Vec<Camera> {
        let cameras = self.cameras.lock().await;
        cameras.values().cloned().collect()
    }

    pub async fn get_camera(&self, camera_id: CameraId) -> Option<Camera> {
        let cameras = self.cameras.lock().await;
        cameras.get(&camera_id).cloned()
    }

    pub async fn get_polling_seconds(&self) -> u64 {
        let polling_seconds = self.polling_seconds.lock().await;
        *polling_seconds
    }

    pub async fn add_camera(&self, camera: Camera) -> anyhow::Result<()> {
        let mut cameras = self.cameras.lock().await;
        for (_, cam) in cameras.iter() {
            if camera.id == cam.id || camera.address == cam.address {
                bail!("camera {} already registered", camera.id);
            }
            // store updated outside to get new camera id
        }
        cameras.insert(camera.id, camera);
        Ok(())
    }

    pub async fn update_camera_snapshot_uri(
        &self,
        camera_id: CameraId,
        snapshot_uri: &str,
    ) -> anyhow::Result<()> {
        let mut cameras = self.cameras.lock().await;
        match cameras.get_mut(&camera_id) {
            Some(camera) => camera.snapshot_uri = Some(snapshot_uri.to_string()),
            None => bail!("cannot find camera {}", camera_id),
        }

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
                self.repo_store.insert_subscription(camera_id, chat_id);
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
                self.repo_store.remove_subscription(camera_id, chat_id);
            }

            Ok(())
        } else {
            bail!("camera {} not found", camera_id)
        }
    }

    pub async fn set_polling_seconds(&self, seconds: u64) {
        let mut polling_seconds = self.polling_seconds.lock().await;
        *polling_seconds = seconds;
    }

    pub async fn update_cameras(&self, new_devices: &[Device]) -> anyhow::Result<()> {
        for new_device in new_devices {
            if !self
                .get_cameras()
                .await
                .iter()
                .any(|camera| camera.address == new_device.address)
            {
                let mut uri = "".to_string();
                if !new_device.urls.is_empty() {
                    if let Some(url) = new_device.urls.first() {
                        uri = format!(
                            "http://{}:{}",
                            url.host_str().unwrap_or_default(),
                            url.port().unwrap_or_default(),
                        );
                    }
                }

                let mut client = match OnvifCamera::new(
                    &uri, "", // TODO
                    "", // TODO
                )
                .await
                {
                    Ok(cli) => cli,
                    Err(err) => {
                        bail!("cannot create OnvifCamera:{}", err);
                    }
                };
                client.init().await;

                let snapshot_uri = match client.get_snapshot_uri().await {
                    Ok(uri) => Some(uri),
                    Err(_) => None,
                };

                // let url = new_device
                //     .urls
                //     .first()
                //     .map(|url| url.as_str().to_string())
                //     .unwrap_or_default();

                let id = self.repo_store.insert_camera(
                    &new_device.name.clone().unwrap_or_default(),
                    &client.uri,
                    &new_device.address,
                    &client.credentials,
                    &snapshot_uri,
                );

                if let Err(err) = self
                    .add_camera(Camera {
                        id,
                        name: new_device.name.clone().unwrap_or_default(),
                        address: new_device.address.clone(),
                        snapshot_uri: None,
                        client,
                        subscriptors: Vec::new(),
                    })
                    .await
                {
                    bail!("{}", err);
                }
            }
        }
        Ok(())
    }
}
