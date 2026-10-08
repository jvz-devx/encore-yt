//! DLNA/UPnP media renderers: SSDP discovery, the device description, and
//! AVTransport/RenderingControl/ConnectionManager over SOAP.

mod soap;

use std::net::{IpAddr, SocketAddr};
use std::time::Duration;

use anyhow::{Context, Result};
use quick_xml::escape::escape;

use crate::ssdp;
use crate::xml::Node;

pub const AV_TRANSPORT: &str = "urn:schemas-upnp-org:service:AVTransport:1";
pub const RENDERING_CONTROL: &str = "urn:schemas-upnp-org:service:RenderingControl:1";
pub const CONNECTION_MANAGER: &str = "urn:schemas-upnp-org:service:ConnectionManager:1";

/// A renderer and the control URLs of the services the app needs.
#[derive(Clone, Debug)]
pub struct Renderer {
    pub name: String,
    pub model: String,
    pub manufacturer: String,
    pub udn: String,
    pub ip: IpAddr,
    pub location: String,
    pub av_transport: String,
    pub rendering_control: Option<String>,
    pub connection_manager: Option<String>,
}

/// Searches for MediaRenderers (at `target`: the multicast group, or one
/// address) and reads each one's description. Devices without AVTransport
/// are left out: they can't be told what to play.
pub async fn scan(target: SocketAddr, wait: Duration) -> Result<Vec<Renderer>> {
    let http = client()?;
    let mut renderers = Vec::new();
    for response in ssdp::search(target, ssdp::MEDIA_RENDERER, wait).await? {
        match describe(&http, &response.location, response.from.ip()).await {
            Ok(Some(renderer)) => renderers.push(renderer),
            Ok(None) => log::info!("dlna: {} has no AVTransport", response.location),
            Err(error) => log::info!("dlna: {}: {error:#}", response.location),
        }
    }
    renderers.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(renderers)
}

pub(crate) fn client() -> Result<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .user_agent("Linux/1 UPnP/1.1 encore-yt/0.1")
        .build()?)
}

async fn describe(http: &reqwest::Client, location: &str, ip: IpAddr) -> Result<Option<Renderer>> {
    let xml = http
        .get(location)
        .send()
        .await?
        .error_for_status()?
        .text()
        .await?;
    parse_description(&xml, location, ip)
}

fn parse_description(xml: &str, location: &str, ip: IpAddr) -> Result<Option<Renderer>> {
    let root = Node::parse(xml)?;
    let device = root.child("device").context("no <device>")?;
    let base = match root.text_of("URLBase") {
        "" => location,
        base => base,
    };
    let base = reqwest::Url::parse(base).context("bad location")?;
    let mut services = Vec::new();
    root.find_all("service", &mut services);
    let control = |kind: &str| {
        services
            .iter()
            .find(|s| s.text_of("serviceType") == kind)
            .and_then(|s| base.join(s.text_of("controlURL")).ok())
            .map(String::from)
    };
    let Some(av_transport) = control(AV_TRANSPORT) else {
        return Ok(None);
    };
    Ok(Some(Renderer {
        name: device.text_of("friendlyName").to_owned(),
        model: device.text_of("modelName").to_owned(),
        manufacturer: device.text_of("manufacturer").to_owned(),
        udn: device.text_of("UDN").to_owned(),
        ip,
        location: location.to_owned(),
        av_transport,
        rendering_control: control(RENDERING_CONTROL),
        connection_manager: control(CONNECTION_MANAGER),
    }))
}

/// What to play, for the DIDL-Lite metadata renderers show and some
/// require before they accept a URI.
#[derive(Clone, Debug, Default)]
pub struct Track {
    pub url: String,
    pub mime: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub art: Option<String>,
    pub duration: Option<Duration>,
}

/// DIDL-Lite for one music track. The `res` protocolInfo repeats the relay's
/// `contentFeatures.dlna.org` (byte seeks supported).
pub fn didl(track: &Track) -> String {
    let duration = track
        .duration
        .map(|d| format!(" duration=\"{}.000\"", hms(d)))
        .unwrap_or_default();
    let art = track
        .art
        .as_ref()
        .map(|a| {
            format!(
                "<upnp:albumArtURI>{}</upnp:albumArtURI>",
                escape(a.as_str())
            )
        })
        .unwrap_or_default();
    format!(
        concat!(
            r#"<DIDL-Lite xmlns="urn:schemas-upnp-org:metadata-1-0/DIDL-Lite/" "#,
            r#"xmlns:dc="http://purl.org/dc/elements/1.1/" "#,
            r#"xmlns:upnp="urn:schemas-upnp-org:metadata-1-0/upnp/">"#,
            r#"<item id="0" parentID="-1" restricted="1">"#,
            "<dc:title>{title}</dc:title><dc:creator>{artist}</dc:creator>",
            "<upnp:artist>{artist}</upnp:artist><upnp:album>{album}</upnp:album>{art}",
            "<upnp:class>object.item.audioItem.musicTrack</upnp:class>",
            r#"<res protocolInfo="http-get:*:{mime}:DLNA.ORG_OP=01;DLNA.ORG_CI=0"{duration}>{url}</res>"#,
            "</item></DIDL-Lite>"
        ),
        title = escape(track.title.as_str()),
        artist = escape(track.artist.as_str()),
        album = escape(track.album.as_str()),
        art = art,
        mime = escape(track.mime.as_str()),
        duration = duration,
        url = escape(track.url.as_str()),
    )
}

/// `H:MM:SS`, the UPnP time format.
pub fn hms(d: Duration) -> String {
    let s = d.as_secs();
    format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60)
}

/// Seconds from `H:MM:SS[.fff]`; None for `NOT_IMPLEMENTED` and the like.
pub fn seconds(hms: &str) -> Option<f64> {
    let mut parts = hms.split(':').rev();
    let s: f64 = parts.next()?.parse().ok()?;
    let m: f64 = parts.next().unwrap_or("0").parse().ok()?;
    let h: f64 = parts.next().unwrap_or("0").parse().ok()?;
    Some(h * 3600.0 + m * 60.0 + s)
}

impl Renderer {
    /// SetAVTransportURI with the track's DIDL-Lite.
    pub async fn set_uri(&self, track: &Track) -> Result<()> {
        let meta = didl(track);
        soap::call(
            &self.av_transport,
            AV_TRANSPORT,
            "SetAVTransportURI",
            &[
                ("InstanceID", "0"),
                ("CurrentURI", &track.url),
                ("CurrentURIMetaData", &meta),
            ],
        )
        .await?;
        Ok(())
    }

    pub async fn play(&self) -> Result<()> {
        self.transport("Play", &[("Speed", "1")]).await
    }

    pub async fn pause(&self) -> Result<()> {
        self.transport("Pause", &[]).await
    }

    pub async fn stop(&self) -> Result<()> {
        self.transport("Stop", &[]).await
    }

    pub async fn seek(&self, to: Duration) -> Result<()> {
        self.transport("Seek", &[("Unit", "REL_TIME"), ("Target", &hms(to))])
            .await
    }

    /// PLAYING, PAUSED_PLAYBACK, STOPPED, TRANSITIONING or NO_MEDIA_PRESENT.
    pub async fn state(&self) -> Result<String> {
        let reply = soap::call(
            &self.av_transport,
            AV_TRANSPORT,
            "GetTransportInfo",
            &[("InstanceID", "0")],
        )
        .await?;
        Ok(reply.text_of("CurrentTransportState").to_owned())
    }

    /// The position and the track's length, in seconds, as far as known.
    pub async fn position(&self) -> Result<(Option<f64>, Option<f64>)> {
        let reply = soap::call(
            &self.av_transport,
            AV_TRANSPORT,
            "GetPositionInfo",
            &[("InstanceID", "0")],
        )
        .await?;
        Ok((
            seconds(reply.text_of("RelTime")),
            seconds(reply.text_of("TrackDuration")),
        ))
    }

    /// Master volume, 0–100.
    pub async fn set_volume(&self, volume: u8) -> Result<()> {
        let url = self
            .rendering_control
            .as_deref()
            .context("no RenderingControl")?;
        let volume = volume.min(100).to_string();
        soap::call(
            url,
            RENDERING_CONTROL,
            "SetVolume",
            &[
                ("InstanceID", "0"),
                ("Channel", "Master"),
                ("DesiredVolume", &volume),
            ],
        )
        .await?;
        Ok(())
    }

    /// The formats the renderer says it plays (`http-get:*:audio/mp4:*`
    /// entries of the Sink protocol info).
    pub async fn sink_formats(&self) -> Result<Vec<String>> {
        let url = self
            .connection_manager
            .as_deref()
            .context("no ConnectionManager")?;
        let reply = soap::call(url, CONNECTION_MANAGER, "GetProtocolInfo", &[]).await?;
        Ok(reply
            .text_of("Sink")
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
            .collect())
    }

    async fn transport(&self, action: &str, args: &[(&str, &str)]) -> Result<()> {
        let mut all = vec![("InstanceID", "0")];
        all.extend_from_slice(args);
        soap::call(&self.av_transport, AV_TRANSPORT, action, &all).await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DESCRIPTION: &str = r#"<?xml version="1.0"?>
<root xmlns="urn:schemas-upnp-org:device-1-0">
  <specVersion><major>1</major><minor>0</minor></specVersion>
  <device>
    <deviceType>urn:schemas-upnp-org:device:MediaRenderer:1</deviceType>
    <friendlyName>Living room &amp; kitchen</friendlyName>
    <manufacturer>Sonos, Inc.</manufacturer>
    <modelName>Sonos One</modelName>
    <UDN>uuid:RINCON_1</UDN>
    <serviceList>
      <service><serviceType>urn:schemas-upnp-org:service:RenderingControl:1</serviceType>
        <controlURL>/MediaRenderer/RenderingControl/Control</controlURL></service>
      <service><serviceType>urn:schemas-upnp-org:service:AVTransport:1</serviceType>
        <controlURL>/MediaRenderer/AVTransport/Control</controlURL></service>
    </serviceList>
  </device>
</root>"#;

    #[test]
    fn a_description_gives_name_and_absolute_control_urls() {
        let ip = "192.168.1.20".parse().unwrap();
        let r = parse_description(
            DESCRIPTION,
            "http://192.168.1.20:1400/xml/device_description.xml",
            ip,
        )
        .unwrap()
        .unwrap();
        assert_eq!(r.name, "Living room & kitchen");
        assert_eq!(r.model, "Sonos One");
        assert_eq!(
            r.av_transport,
            "http://192.168.1.20:1400/MediaRenderer/AVTransport/Control"
        );
        assert!(r.rendering_control.is_some());
        assert!(r.connection_manager.is_none());
        let no_transport = DESCRIPTION.replace("AVTransport:1", "Other:1");
        assert!(
            parse_description(&no_transport, &r.location, ip)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn didl_is_escaped_and_parses() {
        let track = Track {
            url: "http://h:1/s/a.m4a?x=1&y=2".into(),
            mime: "audio/mp4".into(),
            title: "Rock & <Roll>".into(),
            duration: Some(Duration::from_secs(3725)),
            ..Track::default()
        };
        let meta = didl(&track);
        let root = Node::parse(&meta).unwrap();
        let res = root.find("res").unwrap();
        assert_eq!(res.text, track.url);
        assert_eq!(root.find("title").unwrap().text, "Rock & <Roll>");
        assert!(meta.contains(r#"duration="1:02:05.000""#));
        assert_eq!(seconds("1:02:05.500"), Some(3725.5));
        assert_eq!(seconds("NOT_IMPLEMENTED"), None);
    }
}
