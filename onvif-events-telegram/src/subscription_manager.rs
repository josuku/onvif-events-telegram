use onvif::onvif_camera_client::PULL_SUBSCRIPTION_TIMEOUT;
use repository::memory_repository::MemoryRepository;
use std::sync::Arc;
use tracing::info;

const RENEW_INTERVAL_SECS: u64 = 25 * 60; // must be lower than PULL_SUBSCRIPTION_TIMEOUT (30 mins)

pub async fn renew_subscriptions(repository: Arc<MemoryRepository>) {
    loop {
        tokio::time::sleep(tokio::time::Duration::from_secs(RENEW_INTERVAL_SECS)).await;

        let cameras = repository.get_cameras().await;
        for camera in cameras.into_iter().filter(|c| !c.subscriptors.is_empty()) {
            info!("renewing subscription for camera:{}", camera.name);
            camera
                .client
                .renew_subscription(PULL_SUBSCRIPTION_TIMEOUT)
                .await;
        }
    }
}

pub async fn close_subscriptions(repository: Arc<MemoryRepository>) {
    let cameras = repository.get_cameras().await;
    for camera in cameras.into_iter().filter(|c| !c.subscriptors.is_empty()) {
        info!("closing subscription for camera:{}", camera.name);
        camera.client.unsubscribe().await;
    }
}
