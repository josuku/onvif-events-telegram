use app_core::{
    domain::discovery_device::DiscoveryDevice, helpers::network::get_primary_ipv4_address,
};
use async_trait::async_trait;
use futures_util::stream::StreamExt;
use log::warn;
use onvif::discovery::{self, Device};

use app_core::traits::discovery_client::DiscoveryClient;

pub struct OnvifDiscoveryClient {}

#[async_trait]
impl DiscoveryClient for OnvifDiscoveryClient {
    async fn camera_discovery() -> Vec<DiscoveryDevice> {
        // multicast discovery
        let mut devices = discovery::DiscoveryBuilder::default()
            .run()
            .await
            .unwrap()
            .collect::<Vec<Device>>()
            .await;

        if devices.is_empty() {
            warn!("no devices discovery using multicast. trying with unicast");

            let ipv4 = match get_primary_ipv4_address() {
                Ok(ip) => ip,
                Err(_) => return Vec::new(),
            };

            if let Some(netmask) = ipv4.netmask {
                // try unicast discovery
                devices = discovery::DiscoveryBuilder::default()
                    .discovery_mode(onvif::discovery::DiscoveryMode::Unicast {
                        network: ipv4.ip,
                        network_mask: netmask,
                    })
                    .run()
                    .await
                    .unwrap()
                    .collect::<Vec<Device>>()
                    .await;
            }
            if devices.is_empty() {
                warn!("no devices discovery using unicast");
            }
        }

        devices
            .iter()
            .map(|device| DiscoveryDevice {
                address: device.address.clone(),
                name: device.name.clone(),
                urls: device.urls.clone(),
            })
            .collect()
    }
}
