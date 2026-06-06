use crate::domain::discovery_device::DiscoveryDevice;
use async_trait::async_trait;

#[async_trait]
pub trait DiscoveryClient: Send + Sync {
    async fn camera_discovery() -> Vec<DiscoveryDevice>;
}
