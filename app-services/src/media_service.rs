use anyhow::{Context, Result};
use app_core::{CameraId, domain::object::Object, traits::object_detector::ObjectDetector};
use repository::memory_repository::MemoryRepository;
use std::sync::Arc;
use tokio::sync::Mutex;

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
}
