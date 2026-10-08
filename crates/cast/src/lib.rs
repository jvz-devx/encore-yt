//! Casting spike (M35, docs/gpui/CASTING.md): finds Google Cast devices
//! (mDNS) and DLNA/UPnP renderers (SSDP) on the local network, serves a
//! stream to them from a small HTTP relay in the app, and tells them to play
//! it: Cast v2 with the Default Media Receiver, or UPnP AVTransport.
//!
//! The device always fetches from the relay, never from googlevideo: stream
//! URLs are signed for the address that resolved them, expire, and a
//! receiver that can't play a format needs the app in between anyway.

pub mod castv2;
pub mod device;
pub mod dlna;
mod http;
pub mod mdns;
pub mod relay;
pub mod ssdp;
mod sync;
mod xml;

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "test assertions report fixture failures"
)]
mod tests;

pub use device::{Device, Kind};

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, UdpSocket};

/// The address of this machine that `peer` would see: the interface the
/// route to it leaves by. No packet is sent (a UDP "connect" only picks the
/// route), so this is the address to put in URLs the device fetches.
pub fn local_ip_for(peer: IpAddr) -> std::io::Result<IpAddr> {
    let bind: SocketAddr = match peer {
        IpAddr::V4(_) => (Ipv4Addr::UNSPECIFIED, 0).into(),
        IpAddr::V6(_) => (Ipv6Addr::UNSPECIFIED, 0).into(),
    };
    let socket = UdpSocket::bind(bind)?;
    socket.connect(SocketAddr::new(peer, 9))?;
    Ok(socket.local_addr()?.ip())
}
