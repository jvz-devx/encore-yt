//! A fake DLNA renderer: answers an SSDP M-SEARCH (sent to its own address
//! instead of the multicast group), serves its description, and handles
//! AVTransport, RenderingControl and ConnectionManager actions. On Play it
//! fetches the URI it was given, with a Range, from the relay.

use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::io::{AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, UdpSocket};

use super::{TestFile, accesses};
use crate::dlna::{self, Track};
use crate::http;
use crate::relay::{Relay, Source};
use crate::xml::Node;

#[derive(Default, Debug)]
struct State {
    uri: String,
    meta_title: String,
    playing: bool,
    volume: String,
    fetched: usize,
    actions: Vec<String>,
    controls: bool,
    paused: bool,
    position: Option<String>,
    seek_ignored: bool,
}

const DESCRIPTION: &str = r#"<?xml version="1.0"?>
<root xmlns="urn:schemas-upnp-org:device-1-0"><device>
<deviceType>urn:schemas-upnp-org:device:MediaRenderer:1</deviceType>
<friendlyName>Fake renderer</friendlyName><manufacturer>Encore tests</manufacturer>
<modelName>Fake 1</modelName><UDN>uuid:fake-1</UDN><serviceList>
<service><serviceType>urn:schemas-upnp-org:service:AVTransport:1</serviceType><controlURL>avt</controlURL></service>
<service><serviceType>urn:schemas-upnp-org:service:RenderingControl:1</serviceType><controlURL>/rc</controlURL></service>
<service><serviceType>urn:schemas-upnp-org:service:ConnectionManager:1</serviceType><controlURL>/cm</controlURL></service>
</serviceList></device></root>"#;

/// Starts the renderer; returns its SSDP address.
async fn fake_renderer(state: Arc<Mutex<State>>) -> SocketAddr {
    let http_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let http_addr = http_listener.local_addr().unwrap();
    let udp = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let ssdp_addr = udp.local_addr().unwrap();
    tokio::spawn(async move {
        let mut buf = [0u8; 2048];
        while let Ok((len, from)) = udp.recv_from(&mut buf).await {
            let text = String::from_utf8_lossy(&buf[..len]);
            if text.starts_with("M-SEARCH") && text.contains(dlna_st()) {
                let reply = format!(
                    "HTTP/1.1 200 OK\r\nCACHE-CONTROL: max-age=1800\r\nEXT:\r\n\
                     LOCATION: http://{http_addr}/dev/desc.xml\r\nSERVER: Fake UPnP/1.0\r\n\
                     ST: {}\r\nUSN: uuid:fake-1::{}\r\n\r\n",
                    dlna_st(),
                    dlna_st()
                );
                udp.send_to(reply.as_bytes(), from).await.unwrap();
            }
        }
    });
    tokio::spawn(async move {
        loop {
            let (stream, _) = http_listener.accept().await.unwrap();
            let state = state.clone();
            tokio::spawn(async move {
                let mut reader = BufReader::new(stream);
                let request = http::read_request(&mut reader).await.unwrap().unwrap();
                let mut stream = reader.into_inner();
                let (status, body) = handle(&request, &state).await;
                let head = http::head(
                    status,
                    "X",
                    &[
                        ("Content-Type", "text/xml".into()),
                        ("Content-Length", body.len().to_string()),
                    ],
                );
                stream.write_all(head.as_bytes()).await.unwrap();
                stream.write_all(body.as_bytes()).await.unwrap();
            });
        }
    });
    ssdp_addr
}

fn dlna_st() -> &'static str {
    crate::ssdp::MEDIA_RENDERER
}

async fn handle(request: &http::Request, state: &Mutex<State>) -> (u16, String) {
    if request.method == "GET" && request.path == "/dev/desc.xml" {
        return (200, DESCRIPTION.to_owned());
    }
    let action = request
        .header("soapaction")
        .and_then(|a| a.trim_matches('"').split('#').nth(1))
        .unwrap_or_default()
        .to_owned();
    let envelope = Node::parse(std::str::from_utf8(&request.body).unwrap()).unwrap();
    let args = envelope.find(&action).unwrap().clone();
    state
        .lock()
        .unwrap()
        .actions
        .push(format!("{} {action}", request.path));
    let out = match (request.path.as_str(), action.as_str()) {
        ("/dev/avt", "SetAVTransportURI") => {
            let meta = Node::parse(args.text_of("CurrentURIMetaData")).unwrap();
            let mut s = state.lock().unwrap();
            s.uri = args.text_of("CurrentURI").to_owned();
            s.meta_title = meta.find("title").unwrap().text.clone();
            String::new()
        }
        ("/dev/avt", "Play") => {
            let uri = state.lock().unwrap().uri.clone();
            let (status, body) = super::fetch(&uri, Some("bytes=0-")).await;
            assert_eq!(status, 206);
            let mut s = state.lock().unwrap();
            s.fetched = body.len();
            s.playing = true;
            s.paused = false;
            String::new()
        }
        ("/dev/avt", "Stop") => {
            let mut s = state.lock().unwrap();
            s.playing = false;
            s.paused = false;
            String::new()
        }
        ("/dev/avt", "GetTransportInfo") => {
            let s = state.lock().unwrap();
            let now = if s.paused {
                "PAUSED_PLAYBACK"
            } else if s.playing {
                "PLAYING"
            } else {
                "STOPPED"
            };
            format!(
                "<CurrentTransportState>{now}</CurrentTransportState><CurrentTransportStatus>OK</CurrentTransportStatus>"
            )
        }
        ("/dev/avt", "GetPositionInfo") => {
            let position = state
                .lock()
                .unwrap()
                .position
                .clone()
                .unwrap_or_else(|| "0:00:12".into());
            format!("<RelTime>{position}</RelTime><TrackDuration>0:03:00</TrackDuration>")
        }
        ("/dev/avt", "Pause" | "Seek") if state.lock().unwrap().controls => {
            let mut s = state.lock().unwrap();
            if action == "Pause" {
                s.paused = true;
            } else if !s.seek_ignored {
                s.position = Some(args.text_of("Target").into());
            }
            String::new()
        }
        ("/rc", "SetVolume") => {
            state.lock().unwrap().volume = args.text_of("DesiredVolume").to_owned();
            String::new()
        }
        ("/cm", "GetProtocolInfo") => {
            "<Source></Source><Sink>http-get:*:audio/mp4:*,http-get:*:audio/mpeg:*</Sink>"
                .to_owned()
        }
        _ => {
            let fault = "<s:Envelope xmlns:s=\"http://schemas.xmlsoap.org/soap/envelope/\"><s:Body><s:Fault>\
                <detail><UPnPError xmlns=\"urn:schemas-upnp-org:control-1-0\"><errorCode>401</errorCode>\
                <errorDescription>Invalid Action</errorDescription></UPnPError></detail></s:Fault></s:Body></s:Envelope>";
            return (500, fault.to_owned());
        }
    };
    let body = format!(
        "<?xml version=\"1.0\"?><s:Envelope xmlns:s=\"http://schemas.xmlsoap.org/soap/envelope/\"><s:Body>\
         <u:{action}Response xmlns:u=\"x\">{out}</u:{action}Response></s:Body></s:Envelope>"
    );
    (200, body)
}

#[tokio::test]
async fn dlna_remote_session_loads_seeks_pauses_changes_volume_and_stops() {
    let state = Arc::new(Mutex::new(State {
        controls: true,
        ..State::default()
    }));
    let ssdp = fake_renderer(state.clone()).await;
    let renderer = dlna::scan(ssdp, Duration::from_millis(100))
        .await
        .unwrap()
        .remove(0);
    let device = crate::Device::Dlna(renderer);
    let crate::session::Connection::Ready(mut session) = crate::session::Session::connect(
        &device,
        crate::discovery::Policy {
            local_only: true,
            shield_only: false,
        },
        false,
    )
    .await
    .unwrap() else {
        panic!("local renderer")
    };
    assert!(!session.supports("audio/webm"));
    assert!(session.supports("audio/mp4"));
    let file = TestFile::new(50_000);
    let metadata = crate::castv2::Media {
        title: "Local song".into(),
        content_type: "audio/mp4".into(),
        ..Default::default()
    };
    session
        .load(Source::File(file.path.clone()), &metadata, 17.0, true, 45.0)
        .await
        .unwrap();
    assert_eq!(session.poll().await.unwrap().position, 17.0);
    session.pause(true).await.unwrap();
    assert!(!session.poll().await.unwrap().playing);
    session.seek(35.0).await.unwrap();
    session.volume(22.0).await.unwrap();
    session.pause(false).await.unwrap();
    assert!(session.poll().await.unwrap().playing);
    session.stop().await.unwrap();
    let s = state.lock().unwrap();
    assert_eq!(s.fetched, file.bytes.len());
    assert_eq!(s.volume, "22");
    assert_eq!(s.position.as_deref(), Some("0:00:35"));
    assert_eq!(s.meta_title, "Local song");
}

#[tokio::test]
async fn dlna_seek_requires_position_evidence_not_only_a_soap_acknowledgement() {
    let state = Arc::new(Mutex::new(State {
        controls: true,
        seek_ignored: true,
        ..State::default()
    }));
    let ssdp = fake_renderer(state).await;
    let renderer = dlna::scan(ssdp, Duration::from_millis(100))
        .await
        .unwrap()
        .remove(0);
    let crate::session::Connection::Ready(mut session) = crate::session::Session::connect(
        &crate::Device::Dlna(renderer),
        crate::discovery::Policy {
            local_only: true,
            shield_only: false,
        },
        false,
    )
    .await
    .unwrap() else {
        panic!("local renderer")
    };
    let file = TestFile::new(50_000);
    session
        .load(
            Source::File(file.path.clone()),
            &crate::castv2::Media {
                content_type: "audio/mp4".into(),
                ..Default::default()
            },
            0.0,
            true,
            40.0,
        )
        .await
        .unwrap();
    assert!(
        session
            .seek(35.0)
            .await
            .unwrap_err()
            .to_string()
            .contains("didn't move")
    );
    session.stop().await.unwrap();
}

#[tokio::test]
async fn a_renderer_is_found_told_the_uri_and_plays_from_the_relay() {
    let file = TestFile::new(50_000);
    let relay = Relay::start("127.0.0.1".parse().unwrap()).await.unwrap();
    let url = relay
        .publish(Source::File(file.path.clone()), "audio/mp4")
        .unwrap();
    let state = Arc::new(Mutex::new(State::default()));
    let ssdp = fake_renderer(state.clone()).await;

    let renderers = dlna::scan(ssdp, Duration::from_millis(500)).await.unwrap();
    assert_eq!(renderers.len(), 1);
    let renderer = &renderers[0];
    assert_eq!(renderer.name, "Fake renderer");
    assert!(
        renderer.av_transport.ends_with("/dev/avt"),
        "relative to the description"
    );

    let track = Track {
        url: url.clone(),
        mime: "audio/mp4".into(),
        title: "Song & dance".into(),
        artist: "Band".into(),
        ..Track::default()
    };
    assert_eq!(renderer.sink_formats().await.unwrap().len(), 2);
    renderer.set_uri(&track).await.unwrap();
    renderer.play().await.unwrap();
    assert_eq!(renderer.state().await.unwrap(), "PLAYING");
    assert_eq!(
        renderer.position().await.unwrap(),
        (Some(12.0), Some(180.0))
    );
    renderer.set_volume(30).await.unwrap();
    let error = renderer.pause().await.unwrap_err().to_string();
    assert!(error.contains("UPnP error 401"), "{error}");
    renderer.stop().await.unwrap();
    assert_eq!(renderer.state().await.unwrap(), "STOPPED");

    let log = accesses(&relay, 1).await;
    let s = state.lock().unwrap();
    assert_eq!(s.uri, url);
    assert_eq!(s.meta_title, "Song & dance");
    assert_eq!(s.fetched, 50_000);
    assert_eq!(s.volume, "30");
    assert_eq!(log[0].status, 206);
}
