//! SSDP search (UPnP Device Architecture 1.1, section 1.3): an `M-SEARCH`
//! to the multicast group, and unicast HTTP-over-UDP answers carrying the
//! `LOCATION` of each device's description.

use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::time::Duration;

use anyhow::{Context, Result};
use tokio::net::UdpSocket;
use tokio::time::{Instant, timeout_at};

/// The SSDP multicast group and port.
pub const MULTICAST: &str = "239.255.255.250:1900";
/// What a DLNA renderer (TVs, speakers, Sonos) answers to.
pub const MEDIA_RENDERER: &str = "urn:schemas-upnp-org:device:MediaRenderer:1";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Response {
    pub from: SocketAddr,
    pub location: String,
    pub st: String,
    pub usn: String,
    pub server: String,
}

/// Sends an `M-SEARCH` for `st` to `target` (the multicast group, or one
/// address) twice, as UDP may drop it, and collects the answers for `wait`,
/// one per `LOCATION`. Answers come back to this socket's own port, which a
/// host firewall must let in.
pub async fn search(target: SocketAddr, st: &str, wait: Duration) -> Result<Vec<Response>> {
    let socket = UdpSocket::bind("0.0.0.0:0").await.context("bind UDP")?;
    let mx = wait.as_secs().clamp(1, 5);
    let request = format!(
        "M-SEARCH * HTTP/1.1\r\nHOST: 239.255.255.250:1900\r\nMAN: \"ssdp:discover\"\r\n\
         MX: {mx}\r\nST: {st}\r\nUSER-AGENT: Linux/1 UPnP/1.1 encore-yt/0.1\r\n\r\n"
    );
    for _ in 0..2 {
        socket
            .send_to(request.as_bytes(), target)
            .await
            .context("send M-SEARCH")?;
    }
    let deadline = Instant::now() + wait;
    let mut found = BTreeMap::new();
    let mut buf = vec![0u8; 8192];
    while let Ok(received) = timeout_at(deadline, socket.recv_from(&mut buf)).await {
        let (len, from) = received.context("receive")?;
        if let Some(response) = parse(&buf[..len], from)
            && (st == "ssdp:all" || response.st == st)
        {
            found.insert(response.location.clone(), response);
        }
    }
    Ok(found.into_values().collect())
}

/// One answer, or None for anything that isn't a `200 OK` with a location
/// (another host's `M-SEARCH`, a `NOTIFY`).
fn parse(datagram: &[u8], from: SocketAddr) -> Option<Response> {
    let text = std::str::from_utf8(datagram).ok()?;
    let mut lines = text.split("\r\n");
    let status = lines.next()?;
    if !status.starts_with("HTTP/1.1 200") && !status.starts_with("HTTP/1.0 200") {
        return None;
    }
    let header = |name: &str| {
        text.split("\r\n").skip(1).find_map(|line| {
            let (key, value) = line.split_once(':')?;
            key.trim()
                .eq_ignore_ascii_case(name)
                .then(|| value.trim().to_owned())
        })
    };
    Some(Response {
        from,
        location: header("location")?,
        st: header("st").unwrap_or_default(),
        usn: header("usn").unwrap_or_default(),
        server: header("server").unwrap_or_default(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn answers_are_parsed_and_searches_ignored() {
        let from: SocketAddr = "192.168.1.9:1900".parse().unwrap();
        let ok = b"HTTP/1.1 200 OK\r\nCACHE-CONTROL: max-age=1800\r\nLocation: http://192.168.1.9:49152/desc.xml\r\nST: urn:schemas-upnp-org:device:MediaRenderer:1\r\nUSN: uuid:1::urn:x\r\nServer: Linux UPnP/1.0 Sonos/80.1\r\n\r\n";
        let r = parse(ok, from).unwrap();
        assert_eq!(r.location, "http://192.168.1.9:49152/desc.xml");
        assert_eq!(r.st, MEDIA_RENDERER);
        assert_eq!(r.server, "Linux UPnP/1.0 Sonos/80.1");
        let search = b"M-SEARCH * HTTP/1.1\r\nST: ssdp:all\r\n\r\n";
        assert!(parse(search, from).is_none());
        assert!(parse(b"HTTP/1.1 200 OK\r\nST: x\r\n\r\n", from).is_none());
    }
}
