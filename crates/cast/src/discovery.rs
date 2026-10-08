//! A combined device list and the fail-closed local-host policy used by checks.

use std::net::{IpAddr, SocketAddr};
use std::time::Duration;

use anyhow::{Context, Result, ensure};

use crate::{Device, dlna, mdns, ssdp};

/// Explicit rather than ambient in tests. The app reads it once at startup.
#[derive(Clone, Copy, Debug, Default)]
pub struct Policy {
    pub local_only: bool,
    /// Maintainer-authorized final check. No other real device is allowed.
    pub shield_only: bool,
}

impl Policy {
    pub fn from_env() -> Self {
        let shield_only = std::env::var("ENCORE_CAST_SHIELD_ONLY").as_deref() == Ok("1");
        Self {
            local_only: !shield_only
                && std::env::var("ENCORE_CAST_LOCAL_ONLY").as_deref() == Ok("1"),
            shield_only,
        }
    }

    /// Check every control endpoint, not just the device's discovery address.
    /// No DNS or connection is made before this check.
    pub fn check(&self, device: &Device) -> Result<()> {
        if self.shield_only {
            ensure!(
                matches!(device, Device::Cast(d) if !d.is_group() && (d.name.to_ascii_uppercase().contains("SHIELD") || d.model.to_ascii_uppercase().contains("SHIELD"))),
                "only the NVIDIA Shield is allowed in this check"
            );
            return Ok(());
        }
        if !self.local_only {
            return Ok(());
        }
        ensure!(
            local_address(device.ip()),
            "only stand-ins on this computer are allowed"
        );
        if let Device::Dlna(r) = device {
            for url in [
                Some(&r.location),
                Some(&r.av_transport),
                r.rendering_control.as_ref(),
                r.connection_manager.as_ref(),
            ]
            .into_iter()
            .flatten()
            {
                check_local_url(url)?;
            }
        }
        Ok(())
    }
}

fn check_local_url(url: &str) -> Result<IpAddr> {
    let url = reqwest::Url::parse(url).context("invalid stand-in URL")?;
    ensure!(url.scheme() == "http", "stand-in must use HTTP");
    let ip: IpAddr = url
        .host_str()
        .context("stand-in has no host")?
        .trim_matches(['[', ']'])
        .parse()
        .context("stand-in must have a literal local address")?;
    ensure!(local_address(ip), "stand-in URL is not on this computer");
    Ok(ip)
}

/// libupnp refuses loopback interfaces. A renderer started on this machine
/// may therefore use its LAN address. Binding a socket verifies that the
/// address belongs to this host without sending a packet or accepting a
/// remote device. Unspecified and multicast addresses are never targets.
fn local_address(ip: IpAddr) -> bool {
    !ip.is_unspecified()
        && !ip.is_multicast()
        && (ip.is_loopback() || std::net::UdpSocket::bind(SocketAddr::new(ip, 0)).is_ok())
}

/// mDNS and SSDP run concurrently. One protocol failing does not hide the other.
pub async fn scan(policy: Policy, wait: Duration) -> Result<Vec<Device>> {
    if policy.shield_only {
        return Ok(mdns::scan(wait)
            .await?
            .into_iter()
            .map(Device::Cast)
            .filter(|d| policy.check(d).is_ok())
            .collect());
    }
    // Explicit local endpoints make desktop checks independent of multicast
    // routing. Overrides are accepted only with the fail-closed policy enabled.
    if policy.local_only {
        let mut devices = Vec::new();
        if let Ok(addr) = std::env::var("ENCORE_CAST_TEST_ADDR") {
            let addr: SocketAddr = addr.parse().context("invalid Cast stand-in address")?;
            let device = Device::Cast(mdns::CastDevice {
                name: "Local Cast receiver".into(),
                model: "Encore test receiver".into(),
                id: "encore-local-cast".into(),
                addr,
                status: String::new(),
            });
            policy.check(&device)?;
            devices.push(device);
        }
        if let Ok(location) = std::env::var("ENCORE_DLNA_TEST_URL") {
            let ip = check_local_url(&location)?;
            let http = reqwest::Client::builder()
                .timeout(Duration::from_secs(5))
                .redirect(reqwest::redirect::Policy::none())
                .build()?;
            if let Some(renderer) = dlna::describe(&http, &location, ip).await? {
                let device = Device::Dlna(renderer);
                policy.check(&device)?;
                devices.push(device);
            }
        }
        if !devices.is_empty() {
            return Ok(devices);
        }
    }
    let (cast, dlna) = tokio::join!(mdns::scan(wait), dlna::scan(ssdp::MULTICAST.parse()?, wait));
    ensure!(
        cast.is_ok() || dlna.is_ok(),
        "couldn't search the network for devices"
    );
    let mut devices: Vec<_> = cast
        .unwrap_or_default()
        .into_iter()
        .map(Device::Cast)
        .chain(dlna.unwrap_or_default().into_iter().map(Device::Dlna))
        .filter(|d| policy.check(d).is_ok())
        .collect();
    devices.sort_by(|a, b| a.name().cmp(b.name()));
    Ok(devices)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, reason = "test fixtures")]
mod tests {
    use super::*;

    #[test]
    fn stand_in_urls_cannot_point_to_the_lan_or_a_hostname() {
        for url in [
            "http://192.168.1.5:8009",
            "http://localhost:8009",
            "http://receiver.test",
            "https://127.0.0.1",
        ] {
            assert!(check_local_url(url).is_err());
        }
        assert!(check_local_url("http://127.0.0.1:1234/device.xml").is_ok());
        assert!(check_local_url("http://[::1]:1234/device.xml").is_ok());
        let d = Device::Cast(mdns::CastDevice {
            name: "Real device".into(),
            model: String::new(),
            id: "real".into(),
            addr: "192.168.1.5:8009".parse().unwrap(),
            status: String::new(),
        });
        assert!(
            Policy {
                local_only: true,
                shield_only: false
            }
            .check(&d)
            .is_err()
        );
    }

    #[test]
    fn shield_check_refuses_nest_minis_and_cast_groups() {
        let policy = Policy {
            local_only: false,
            shield_only: true,
        };
        let mut d = mdns::CastDevice {
            name: "Kitchen speaker".into(),
            model: "Google Nest Mini".into(),
            id: "fake".into(),
            addr: "192.0.2.10:8009".parse().unwrap(),
            status: String::new(),
        };
        assert!(policy.check(&Device::Cast(d.clone())).is_err());
        d.name = "SHIELD".into();
        d.model = "Google Cast Group".into();
        assert!(policy.check(&Device::Cast(d.clone())).is_err());
        d.model = "SHIELD Android TV".into();
        assert!(policy.check(&Device::Cast(d)).is_ok());
    }
}
