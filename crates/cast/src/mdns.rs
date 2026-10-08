//! Google Cast discovery: devices announce `_googlecast._tcp` over mDNS
//! with TXT keys `fn` (friendly name), `md` (model), `id` (a stable id) and
//! `rs` (what the device is showing, empty when idle). Groups of speakers
//! announce themselves the same way, with model "Google Cast Group" and
//! their own port.

use std::collections::BTreeMap;
use std::net::{IpAddr, SocketAddr};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use mdns_sd::{ServiceDaemon, ServiceEvent};

const SERVICE: &str = "_googlecast._tcp.local.";

#[derive(Clone, Debug)]
pub struct CastDevice {
    pub name: String,
    pub model: String,
    pub id: String,
    pub addr: SocketAddr,
    /// The `rs` key: the app the device shows, empty when idle.
    pub status: String,
}

impl CastDevice {
    pub fn is_group(&self) -> bool {
        self.model == "Google Cast Group"
    }
}

/// Browses for Cast devices for `wait` and returns them by name. Runs the
/// mDNS daemon (its own thread, port 5353 shared with avahi or Bonjour) on a
/// blocking task.
pub async fn scan(wait: Duration) -> Result<Vec<CastDevice>> {
    tokio::task::spawn_blocking(move || scan_blocking(wait))
        .await
        .context("mDNS task")?
}

fn scan_blocking(wait: Duration) -> Result<Vec<CastDevice>> {
    let daemon = ServiceDaemon::new().context("start mDNS")?;
    let events = daemon.browse(SERVICE).context("browse")?;
    let deadline = Instant::now() + wait;
    let mut found = BTreeMap::new();
    while let Some(left) = deadline.checked_duration_since(Instant::now()) {
        let Ok(event) = events.recv_timeout(left) else {
            break;
        };
        if let ServiceEvent::ServiceResolved(service) = event
            && let Some(device) = device(
                service.port,
                service.addresses.iter().map(|ip| ip.to_ip_addr()),
                |key| service.get_property_val_str(key).map(str::to_owned),
            )
        {
            found.insert(device.id.clone(), device);
        }
    }
    daemon.shutdown().context("stop mDNS discovery")?;
    let mut devices: Vec<_> = found.into_values().collect();
    devices.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(devices)
}

/// A device from one resolved service, preferring an IPv4 address (Cast
/// devices answer on both; a link-local IPv6 one needs a scope id).
fn device(
    port: u16,
    addresses: impl Iterator<Item = IpAddr>,
    txt: impl Fn(&str) -> Option<String>,
) -> Option<CastDevice> {
    let ip = addresses.min_by_key(|ip| !ip.is_ipv4())?;
    let id = txt("id")?;
    Some(CastDevice {
        name: txt("fn").unwrap_or_else(|| id.clone()),
        model: txt("md").unwrap_or_default(),
        id,
        addr: SocketAddr::new(ip, port),
        status: txt("rs").unwrap_or_default(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_device_prefers_ipv4_and_falls_back_to_the_id_for_a_name() {
        let txt = |key: &str| match key {
            "id" => Some("abc".to_owned()),
            "md" => Some("Google Nest Mini".to_owned()),
            _ => None,
        };
        let addresses = ["fe80::1".parse().unwrap(), "192.168.1.5".parse().unwrap()];
        let d = device(8009, addresses.into_iter(), txt).unwrap();
        assert_eq!(d.addr, "192.168.1.5:8009".parse().unwrap());
        assert_eq!(d.name, "abc");
        assert!(!d.is_group());
        assert!(device(8009, std::iter::empty(), txt).is_none());
    }
}
