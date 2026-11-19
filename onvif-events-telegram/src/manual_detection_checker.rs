use app_core::{
    domain::{
        camera::{CameraData, CameraEvent, CameraEventType},
        event_bus::{EventBus, EventBusMessage},
        object::ObjectClass,
    },
    traits::object_detector::ObjectDetector,
};
use repository::memory_repository::MemoryRepository;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{error, info, warn};

pub async fn start_manual_detection_loop(
    repository: Arc<MemoryRepository>,
    event_bus: Arc<EventBus>,
    object_detector: Arc<Mutex<dyn ObjectDetector>>,
) {
    loop {
        let (enabled, interval, types) = {
            let config = repository.get_config().await;
            (
                config.manual_detection.enable,
                config.manual_detection.interval_seconds,
                if config.manual_detection.types.is_empty() {
                    config.detector.types.clone()
                } else {
                    config.manual_detection.types.clone()
                },
            )
        };

        if enabled {
            check_manual_detection_in_cameras(
                repository.clone(),
                event_bus.clone(),
                object_detector.clone(),
                &types,
            )
            .await;
        }

        tokio::time::sleep(tokio::time::Duration::from_secs(interval.max(5))).await;
    }
}

async fn check_manual_detection_in_cameras(
    repository: Arc<MemoryRepository>,
    event_bus: Arc<EventBus>,
    object_detector: Arc<Mutex<dyn ObjectDetector>>,
    types: &[ObjectClass],
) {
    let cameras = repository.get_cameras().await;
    let subscribed_cameras: Vec<CameraData> = cameras
        .into_iter()
        .filter(|cam| !cam.subscriptors.is_empty())
        .collect();

    let config = repository.get_config().await;

    // Strict sequential execution: one camera at a time to prevent CPU/GPU overload
    for camera in subscribed_cameras {
        let snapshot = match camera.get_snapshot().await {
            Ok(bytes) => bytes,
            Err(err) => {
                warn!(
                    "manual detection: could not get snapshot for camera {}: {}",
                    camera.name, err
                );
                continue;
            }
        };

        let objects = {
            let mut detector = object_detector.lock().await;
            match detector.detect(&snapshot, config.detector.min_confidence, types) {
                Ok(objs) => objs,
                Err(err) => {
                    error!(
                        "manual detection: error detecting objects in camera {}: {}",
                        camera.name, err
                    );
                    Vec::new()
                }
            }
        };

        if !objects.is_empty() {
            let now = chrono::Utc::now();
            let mut should_notify = false;

            for chat_id in &camera.subscriptors {
                let last_notification_time = repository
                    .get_last_notification_time(camera.id, *chat_id)
                    .await;

                if last_notification_time.is_none()
                    || now.timestamp() - last_notification_time.unwrap().timestamp()
                        > config.between_seconds.try_into().unwrap_or(60)
                {
                    should_notify = true;
                    break;
                }
            }

            if should_notify {
                info!(
                    "manual detection: new visual detection in camera:{} objects:{}",
                    camera.name,
                    objects
                        .iter()
                        .map(|o| format!("{o}"))
                        .collect::<Vec<_>>()
                        .join(", ")
                );

                for chat_id in &camera.subscriptors {
                    repository
                        .update_last_notification_time(camera.id, *chat_id)
                        .await;
                }

                event_bus.publish(EventBusMessage::CameraEvent(CameraEvent {
                    r#type: CameraEventType::ManualDetection,
                    timestamp: now,
                    camera: camera.clone(),
                    snapshot,
                    objects,
                    bypass_notification_throttle: false,
                }));
            }
        }

        // 250ms cool-down pause between cameras to ease hardware load
        tokio::time::sleep(tokio::time::Duration::from_millis(250)).await;
    }
}
