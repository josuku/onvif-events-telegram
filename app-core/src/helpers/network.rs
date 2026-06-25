use anyhow::bail;
use network_interface::{Addr, NetworkInterface, NetworkInterfaceConfig, V4IfAddr};

pub fn get_primary_ipv4_address() -> anyhow::Result<V4IfAddr> {
    let network_interfaces = NetworkInterface::show().unwrap();
    for interface in network_interfaces.iter() {
        if is_virtual_interface(&interface.name) {
            continue;
        }
        for addr in &interface.addr {
            if let Addr::V4(ipv4) = addr {
                return Ok(*ipv4);
            }
        }
    }
    bail!("No IPv4 address found for a physical network interface")
}

fn is_virtual_interface(name: &str) -> bool {
    name.starts_with("veth")
        || name.starts_with("tun")
        || name.starts_with("docker")
        || name.starts_with("vEthernet")
        || name.to_lowercase().contains("tailscale")
}

pub async fn is_reachable(uri: &str) -> bool {
    let connection_timeout_seconds = 2;
    let Ok(url) = url::Url::parse(uri) else {
        return false;
    };
    let host = url.host_str().unwrap_or("");
    let port = url.port_or_known_default().unwrap_or(80);
    let addr = format!("{host}:{port}");
    tokio::time::timeout(
        std::time::Duration::from_secs(connection_timeout_seconds),
        tokio::net::TcpStream::connect(&addr),
    )
    .await
    .is_ok_and(|r| r.is_ok())
}
