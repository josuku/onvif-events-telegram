extern crate onvif;
use futures_util::stream::StreamExt;
use onvif::discovery::{self, Device};

pub async fn camera_discovery() -> Vec<Device> {
    discovery::DiscoveryBuilder::default()
        .run()
        .await
        .unwrap()
        .collect::<Vec<Device>>()
        .await
}
