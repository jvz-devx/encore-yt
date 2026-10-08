//! A combined device list and the fail-closed loopback policy used by checks.

use std::net::{IpAddr, SocketAddr};
use std::time::Duration;

use anyhow::{Context, Result, ensure};

use crate::{Device, dlna, mdns, ssdp};

/// Explicit rather than ambient in tests. The app reads it once at startup.
#[derive(Clone, Copy, Debug, Default)]
pub struct Policy {
    pub local_only: bool,
}

impl Policy {
    pub fn from_env() -> Self {
        Self {
            local_only: std::env::var("ENCORE_CAST_LOCAL_ONLY").as_deref() == Ok("1"),
        }
    }

    /// Check every control endpoint, not just the device's discovery address.
    /// No DNS or connection is made before this check.
    pub fn check(&self, device: &Device) -> Result<()> {
        if !self.local_only {
            return Ok(());
        }
        ensure!(
            device.ip().is_loopback(),
            "only loopback stand-ins are allowed"
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
                check_loopback_url(url)?;
            }
        }
        Ok(())
    }
}

fn check_loopback_url(url: &str) -> Result<IpAddr> {
    let url = reqwest::Url::parse(url).context("invalid stand-in URL")?;
    ensure!(url.scheme() == "http", "stand-in must use HTTP");
    let ip: IpAddr = url
        .host_str()
        .context("stand-in has no host")?
        .trim_matches(['[', ']'])
        .parse()
        .context("stand-in must have a literal loopback address")?;
    ensure!(ip.is_loopback(), "stand-in URL is not loopback");
    Ok(ip)
}

/// mDNS and SSDP run concurrently. One protocol failing does not hide the other.
pub async fn scan(policy: Policy, wait: Duration) -> Result<Vec<Device>> {
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
            let ip = check_loopback_url(&location)?;
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
            assert!(check_loopback_url(url).is_err());
        }
        assert!(check_loopback_url("http://127.0.0.1:1234/device.xml").is_ok());
        assert!(check_loopback_url("http://[::1]:1234/device.xml").is_ok());
        let d = Device::Cast(mdns::CastDevice {
            name: "Real device".into(),
            model: String::new(),
            id: "real".into(),
            addr: "192.168.1.5:8009".parse().unwrap(),
            status: String::new(),
        });
        assert!(Policy { local_only: true }.check(&d).is_err());
    }
}
