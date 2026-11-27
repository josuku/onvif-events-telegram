use anyhow::{Context, Result};
use api_camera::{
    dahua_rpc_api_client::DahuaRpcApiCameraClient,
    dvrip_xmeye_client::DvrIpXmeyeApiCameraClient,
    hikvision_isapi_client::HikvisionIsapiApiCameraClient,
};
use app_core::{
    CameraId,
    domain::{
        camera::{CameraData, Recording, RecordingSummary},
        object::Object,
    },
    traits::{api_camera_client::ApiCameraClient, object_detector::ObjectDetector},
};
use chrono::{DateTime, Duration, Utc};
use repository::memory_repository::MemoryRepository;
use std::sync::Arc;
use tokio::sync::Mutex;

async fn get_or_create_api_camera_client(
    camera_data: &mut CameraData,
) -> Option<Arc<dyn ApiCameraClient>> {
    if let Some(ref client) = camera_data.api_camera_client {
        return Some(client.clone());
    }

    let device_info = match camera_data.get_device_info().await {
        Some(info) => info,
        None => {
            tracing::error!("cannot get device info for camera {}", camera_data.name);
            return None;
        }
    };

    camera_data.device_info = Some(device_info.clone());

    let client: Option<Arc<dyn ApiCameraClient>> = match device_info.manufacturer.as_str() {
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
                "recordings API not implemented for manufacturer: {manufacturer}"
            );
            None
        }
    };

    camera_data.api_camera_client = client.clone();
    client
}

pub struct MediaService {
    repository: Arc<MemoryRepository>,
    object_detector: Arc<Mutex<dyn ObjectDetector>>,
}

pub struct DetectionOutcome {
    pub camera_id: CameraId,
    pub camera_name: String,
    pub snapshot: Vec<u8>,
    pub objects: Vec<Object>,
}

impl MediaService {
    pub fn new(
        repository: Arc<MemoryRepository>,
        object_detector: Arc<Mutex<dyn ObjectDetector>>,
    ) -> Self {
        Self {
            repository,
            object_detector,
        }
    }

    pub async fn get_snapshot(&self, camera_id: CameraId) -> Result<Vec<u8>> {
        let camera = self
            .repository
            .get_camera(camera_id)
            .await
            .context(format!("Camera with id {} not found", camera_id))?;

        camera.get_snapshot().await
    }

    pub async fn detect_objects(&self, camera_id: CameraId) -> Result<DetectionOutcome> {
        let camera = self
            .repository
            .get_camera(camera_id)
            .await
            .context(format!("Camera with id {} not found", camera_id))?;

        let snapshot = camera.get_snapshot().await?;
        let config = self.repository.get_config().await;

        let objects = {
            let mut detector = self.object_detector.lock().await;
            match detector.detect(
                &snapshot,
                config.detector.min_confidence,
                &config.detector.types,
            ) {
                Ok(objs) => objs,
                Err(err) => {
                    tracing::error!("error detecting objects in camera {}: {}", camera.name, err);
                    Vec::new()
                }
            }
        };

        Ok(DetectionOutcome {
            camera_id,
            camera_name: camera.name,
            snapshot,
            objects,
        })
    }

    pub async fn get_recording_summary(
        &self,
        camera_id: CameraId,
        days: Option<i64>,
    ) -> Result<RecordingSummary> {
        let mut camera = self
            .repository
            .get_camera(camera_id)
            .await
            .context(format!("Camera with id {} not found", camera_id))?;

        let api_client = get_or_create_api_camera_client(&mut camera).await.context(format!(
            "Recording API not available for camera {}",
            camera_id
        ))?;

        let now = Utc::now();
        let start_time = days.map(|d| now - Duration::days(d));

        api_client
            .get_recording_summary(start_time, Some(now))
            .await
    }

    pub async fn get_recording_list(
        &self,
        camera_id: CameraId,
        date: DateTime<Utc>,
        clip_seconds: Option<u64>,
    ) -> Result<Vec<Recording>> {
        let mut camera = self
            .repository
            .get_camera(camera_id)
            .await
            .context(format!("Camera with id {} not found", camera_id))?;

        let api_client = get_or_create_api_camera_client(&mut camera).await.context(format!(
            "Recording API not available for camera {}",
            camera_id
        ))?;

        let clip = if let Some(s) = clip_seconds {
            Duration::seconds(s as i64)
        } else {
            let config = self.repository.get_config().await;
            Duration::seconds(config.recording_clip as i64)
        };

        api_client.get_recording_list(date, clip).await
    }

    pub async fn download_recording(
        &self,
        camera_id: CameraId,
        date: DateTime<Utc>,
        clip_seconds: Option<u64>,
    ) -> Result<String> {
        let mut camera = self
            .repository
            .get_camera(camera_id)
            .await
            .context(format!("Camera with id {} not found", camera_id))?;

        let api_client = get_or_create_api_camera_client(&mut camera).await.context(format!(
            "Recording API not available for camera {}",
            camera_id
        ))?;

        let clip = if let Some(s) = clip_seconds {
            Duration::seconds(s as i64)
        } else {
            let config = self.repository.get_config().await;
            Duration::seconds(config.recording_clip as i64)
        };

        let recordings = api_client.get_recordings(date, clip).await?;
        if recordings.is_empty() {
            anyhow::bail!("No recordings found at requested date");
        }
        let target_name = format!("camera_{}_{}.mp4", camera_id, date.format("%Y%m%d_%H%M%S"));
        api_client
            .download_recording(&recordings, date, clip, target_name)
            .await
    }
}
