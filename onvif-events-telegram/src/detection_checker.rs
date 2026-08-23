use app_core::{
    domain::{
        camera::{CameraData, CameraEvent},
        error_message::ErrorMessage,
        event_bus::{EventBus, EventBusMessage},
    },
    traits::object_detector::ObjectDetector,
    CameraId,
};
use chrono::{TimeDelta, Utc};
use itertools::Itertools;
use onvif::onvif_rs_camera_client::create_onvif_camera_client;
use repository::memory_repository::MemoryRepository;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{error, info};

pub async fn check_for_detections_in_cameras(
    repository: Arc<MemoryRepository>,
    event_bus: Arc<EventBus>,
    object_detector: Arc<Mutex<dyn ObjectDetector>>,
) {
    let now = chrono::Utc::now();
    let cameras = repository.get_cameras().await;

    let tasks: Vec<_> = cameras
        .into_iter()
        .filter(|camera| !camera.subscriptors.is_empty())
        .map(|camera| {
            let repository = repository.clone();
            let event_bus = event_bus.clone();
            let object_detector = object_detector.clone();
            tokio::spawn(async move {
                check_camera(camera.id, repository, event_bus, now, object_detector).await;
            })
        })
        .collect();

    for task in tasks {
        if let Err(err) = task.await {
            error!("camera polling task panicked with error:{err}")
        }
    }
}

async fn check_camera(
    camera_id: CameraId,
    repository: Arc<MemoryRepository>,
    event_bus: Arc<EventBus>,
    now: chrono::DateTime<chrono::Utc>,
    object_detector: Arc<Mutex<dyn ObjectDetector>>,
) {
    let camera = match repository.get_camera(camera_id).await {
        Some(camera) => camera,
        None => {
            tracing::error!("camera with id {} not found", camera_id);
            return;
        }
    };

    tracing::debug!(
        "check_camera: {:?}",
        camera.onvif_client.get_connection_data().uri
    );
    let onvif_event = match camera.onvif_client.get_event_message().await {
        Ok(Some(event)) => event,
        Ok(None) => {
            clean_sync_error_and_notify(&camera, &repository, &event_bus).await;
            return;
        }
        Err(err) => {
            error!("error getting pull message. error:{}", err);
            camera.onvif_client.unsubscribe().await;

            // updated camera, error can appear after many seconds
            let camera = match repository.get_camera(camera_id).await {
                Some(camera) => camera,
                None => {
                    tracing::error!("camera with id {} not found", camera_id);
                    return;
                }
            };

            let conn_data = camera.onvif_client.get_connection_data();
            match create_onvif_camera_client(
                &conn_data.uri,
                &conn_data.username,
                &conn_data.password,
            )
            .await
            {
                Ok(client) => {
                    if let Err(err) = repository
                        .replace_camera_client(camera.id, Arc::new(client))
                        .await
                    {
                        error!("cannot replace camera client in repository. error:{}", err);
                    }
                }
                Err(err) => error!("cannot create onvif camera client. error:{}", err),
            };

            set_sync_error_and_notify(&camera, &repository, &event_bus).await;

            return;
        }
    };

    clean_sync_error_and_notify(&camera, &repository, &event_bus).await;

    let snapshot = match camera.onvif_client.snapshot().await {
        Ok(snapshot) => snapshot,
        Err(err) => {
            error!(
                "error getting snapshot from camera:{}. error:{}",
                camera.name, err
            );
            return;
        }
    };

    let config = repository.get_config().await;
    let objects = if config.detector.enable {
        let mut object_detector = object_detector.lock().await;
        match object_detector.detect(
            &snapshot,
            config.detector.min_confidence,
            &config.detector.types,
        ) {
            Ok(objects) => objects,
            Err(err) => {
                error!("error detecting objects:{}", err);
                Vec::new()
            }
        }
    } else {
        Vec::new()
    };

    event_bus.publish(EventBusMessage::CameraEvent(CameraEvent {
        r#type: onvif_event.r#type,
        timestamp: onvif_event.timestamp,
        camera: camera.clone(),
        snapshot,
        objects: objects.clone(),
    }));

    // TODO DO IN REPOSITORY
    repository
        .update_last_polling_from_camera(camera.id, now)
        .await;

    info!(
        "{} - new detection in camera:{} type:{} objects:{}",
        onvif_event.timestamp,
        camera.name,
        onvif_event.r#type,
        objects.iter().map(|o| format!("{o}")).join(", ")
    );
}

async fn set_sync_error_and_notify(
    camera: &CameraData,
    repository: &Arc<MemoryRepository>,
    event_bus: &Arc<EventBus>,
) {
    if let Some(last_error) = camera.status.last_error {
        let diff = Utc::now() - last_error;
        if diff > TimeDelta::minutes(5)
            && !camera.status.last_error_notified
            && repository.get_config().await.send_errors
        {
            event_bus.publish(EventBusMessage::Error(ErrorMessage {
                timestamp: last_error,
                camera: camera.clone(),
                message: format!(
                    "❌ Sync camera failed during {} minutes",
                    diff.num_minutes()
                ),
                recovered: false,
            }));
            let _ = repository
                .set_camera_last_error(camera.id, camera.status.last_error, true)
                .await;
        }
    } else {
        let _ = repository
            .set_camera_last_error(camera.id, Some(Utc::now()), false)
            .await;
    }
}

async fn clean_sync_error_and_notify(
    camera: &CameraData,
    repository: &Arc<MemoryRepository>,
    event_bus: &Arc<EventBus>,
) {
    if camera.status.last_error.is_some() {
        if camera.status.last_error_notified && repository.get_config().await.send_errors {
            event_bus.publish(EventBusMessage::Error(ErrorMessage {
                timestamp: Utc::now(),
                camera: camera.clone(),
                message: "✅ Sync recovered".to_string(),
                recovered: true,
            }));
        }

        repository
            .set_camera_last_error(camera.id, None, false)
            .await;
    }
}
