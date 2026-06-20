use app_core::domain::{camera::CameraEvent, event_bus::EventBus};
use onvif::onvif_camera_client::create_onvif_camera_client;
use repository::memory_repository::MemoryRepository;
use std::sync::Arc;
use tracing::{error, info};

pub async fn check_for_detections_in_cameras(
    repository: Arc<MemoryRepository>,
    event_bus: Arc<EventBus>,
) {
    let now = chrono::Utc::now();

    for camera in repository.get_cameras().await {
        let onvif_event = match camera.client.get_event_message().await {
            Ok(event) => match event {
                Some(event) => event,
                None => continue,
            },
            Err(err) => {
                error!("error getting pull message. error:{}", err);
                let conn_data = camera.client.get_connection_data();
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
                        continue;
                    }
                    Err(err) => {
                        error!("cannot create onvif camera client. error:{}", err);
                        continue;
                    }
                };
            }
        };

        let snapshot = match camera.client.snapshot().await {
            Ok(snapshot) => snapshot,
            Err(err) => {
                error!(
                    "error getting snapshot from camera:{}. error:{}",
                    camera.name, err
                );
                continue;
            }
        };

        // TODO add yolo object detection over snapshot

        event_bus.publish(CameraEvent {
            r#type: onvif_event.r#type,
            timestamp: onvif_event.timestamp,
            camera: camera.clone(),
            snapshot,
        });

        // TODO DO IN REPOSITORY
        repository
            .update_last_polling_from_camera(camera.id, now)
            .await;

        info!(
            "{} - new detection in camera:{} type:{}",
            onvif_event.timestamp, camera.name, onvif_event.r#type
        );
    }
}
