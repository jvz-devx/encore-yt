//! A device found on the network, of either protocol.

use std::net::IpAddr;

use crate::dlna::Renderer;
use crate::mdns::CastDevice;

/// The protocol a device is controlled with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Cast,
    Dlna,
}

#[derive(Clone, Debug)]
pub enum Device {
    Cast(CastDevice),
    Dlna(Renderer),
}

impl Device {
    pub fn kind(&self) -> Kind {
        match self {
            Device::Cast(_) => Kind::Cast,
            Device::Dlna(_) => Kind::Dlna,
        }
    }

    /// The name people gave the device ("Kitchen speaker").
    pub fn name(&self) -> &str {
        match self {
            Device::Cast(d) => &d.name,
            Device::Dlna(r) => &r.name,
        }
    }

    pub fn model(&self) -> &str {
        match self {
            Device::Cast(d) => &d.model,
            Device::Dlna(r) => &r.model,
        }
    }

    pub fn ip(&self) -> IpAddr {
        match self {
            Device::Cast(d) => d.addr.ip(),
            Device::Dlna(r) => r.ip,
        }
    }

    /// A stable id: the Cast `id` TXT key or the UPnP UDN.
    pub fn id(&self) -> &str {
        match self {
            Device::Cast(d) => &d.id,
            Device::Dlna(r) => &r.udn,
        }
    }
}
