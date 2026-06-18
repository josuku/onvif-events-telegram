use app_core::traits::discovery_client::DiscoveryClient;
use app_core::{
    domain::discovery_device::DiscoveryDevice, helpers::network::get_primary_ipv4_address,
};
use async_trait::async_trait;
use futures_util::stream::StreamExt;
use onvif::discovery::{self, Device};
use std::net::IpAddr;
use tracing::{info, warn};

pub struct OnvifDiscoveryClient {}

#[async_trait]
impl DiscoveryClient for OnvifDiscoveryClient {
    async fn camera_discovery() -> Vec<DiscoveryDevice> {
        let ipv4 = match get_primary_ipv4_address() {
            Ok(ip) => ip,
            Err(_) => return Vec::new(),
        };

        info!("trying to discover devices using {:?}", ipv4);

        // multicast discovery
        let mut devices = discovery::DiscoveryBuilder::default()
            .listen_address(IpAddr::V4(ipv4.ip))
            .run()
            .await
            .unwrap()
            .collect::<Vec<Device>>()
            .await;

        if devices.is_empty() {
            warn!("no devices discovery using multicast. trying with unicast");

            if let Some(netmask) = ipv4.netmask {
                // try unicast discovery
                devices = discovery::DiscoveryBuilder::default()
                    .listen_address(IpAddr::V4(ipv4.ip))
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

        info!("Discovered {} devices", devices.len());

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
