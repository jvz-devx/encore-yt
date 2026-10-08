//! A fake Cast device: TLS with a self-signed certificate (as real devices
//! have), length-prefixed `CastMessage`s, a PING the sender must answer,
//! and the receiver/media requests of a Default Media Receiver session. On
//! LOAD and SEEK it fetches the media from the relay with a Range, as the
//! receiver's player does.

use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

use super::{TestFile, accesses};
use crate::castv2::messages::{NS_HEARTBEAT, NS_MEDIA, NS_RECEIVER};
use crate::castv2::proto::CastMessage;
use crate::castv2::{Client, DEFAULT_MEDIA_RECEIVER, Media};
use crate::relay::{Relay, Source};

#[derive(Default, Debug)]
struct Seen {
    /// `namespace type` of each message, in order.
    messages: Vec<String>,
    /// Bytes the "player" fetched from the relay.
    fetched: Vec<u8>,
}

async fn fake_device(seen: Arc<Mutex<Seen>>) -> SocketAddr {
    let cert = rcgen::generate_simple_self_signed(vec!["fake-cast".to_owned()]).unwrap();
    let key = rustls::pki_types::PrivateKeyDer::Pkcs8(cert.signing_key.serialize_der().into());
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let config = rustls::ServerConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_no_client_auth()
        .with_single_cert(vec![cert.cert.der().clone()], key)
        .unwrap();
    let acceptor = tokio_rustls::TlsAcceptor::from(Arc::new(config));
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let (tcp, _) = listener.accept().await.unwrap();
        let mut tls = acceptor.accept(tcp).await.unwrap();
        let ping = CastMessage::new(
            "receiver-0",
            "*",
            NS_HEARTBEAT,
            json!({"type":"PING"}).to_string(),
        );
        tls.write_all(&ping.frame()).await.unwrap();
        let mut content = String::new();
        loop {
            let mut len = [0u8; 4];
            if tls.read_exact(&mut len).await.is_err() {
                return;
            }
            let mut body = vec![0u8; u32::from_be_bytes(len) as usize];
            tls.read_exact(&mut body).await.unwrap();
            let msg = CastMessage::decode(&body).unwrap();
            let payload: Value = serde_json::from_str(msg.payload.as_deref().unwrap()).unwrap();
            let kind = payload["type"].as_str().unwrap_or_default().to_owned();
            seen.lock()
                .unwrap()
                .messages
                .push(format!("{} {kind}", short(&msg.namespace)));
            let id = payload["requestId"].clone();
            let reply = match (msg.namespace.as_str(), kind.as_str()) {
                (NS_RECEIVER, "GET_STATUS") => Some(receiver_status(id, false)),
                (NS_RECEIVER, "LAUNCH") => Some(receiver_status(id, true)),
                (NS_RECEIVER, "STOP") => Some(receiver_status(id, false)),
                (NS_MEDIA, "LOAD") => {
                    assert_eq!(msg.destination, "web-7");
                    content = payload["media"]["contentId"].as_str().unwrap().to_owned();
                    let (_, bytes) = super::fetch(&content, Some("bytes=0-")).await;
                    seen.lock().unwrap().fetched = bytes;
                    Some(media_status(id, "PLAYING", 0.0))
                }
                (NS_MEDIA, "SEEK") => {
                    let at = payload["currentTime"].as_f64().unwrap();
                    super::fetch(&content, Some("bytes=40000-")).await;
                    Some(media_status(id, "PLAYING", at))
                }
                (NS_MEDIA, "PAUSE") => Some(media_status(id, "PAUSED", 30.0)),
                _ => None,
            };
            if let Some(reply) = reply {
                let ns = msg.namespace.clone();
                let out = CastMessage::new(&msg.destination, &msg.source, &ns, reply.to_string());
                tls.write_all(&out.frame()).await.unwrap();
            }
        }
    });
    addr
}

fn short(namespace: &str) -> &str {
    namespace.rsplit('.').next().unwrap_or(namespace)
}

fn receiver_status(id: Value, running: bool) -> Value {
    let apps = if running {
        json!([{"appId": DEFAULT_MEDIA_RECEIVER, "displayName": "Default Media Receiver",
                "sessionId": "sess-1", "transportId": "web-7", "isIdleScreen": false}])
    } else {
        json!([{"appId": "E8C28D3C", "displayName": "Backdrop",
                "sessionId": "idle", "transportId": "idle", "isIdleScreen": true}])
    };
    json!({"type": "RECEIVER_STATUS", "requestId": id,
           "status": {"applications": apps, "volume": {"level": 0.3, "muted": false}}})
}

fn media_status(id: Value, state: &str, at: f64) -> Value {
    json!({"type": "MEDIA_STATUS", "requestId": id,
           "status": [{"mediaSessionId": 1, "playerState": state, "currentTime": at}]})
}

#[tokio::test]
async fn a_session_launches_loads_from_the_relay_seeks_pauses_and_stops() {
    let file = TestFile::new(80_000);
    let relay = Relay::start("127.0.0.1".parse().unwrap()).await.unwrap();
    let url = relay
        .publish(Source::File(file.path.clone()), "audio/webm")
        .unwrap();
    let seen = Arc::new(Mutex::new(Seen::default()));
    let device = fake_device(seen.clone()).await;

    let client = Client::connect(device).await.unwrap();
    let status = client.receiver_status().await.unwrap();
    assert!(status.busy_app().is_none(), "only the idle screen runs");
    let app = client.launch(DEFAULT_MEDIA_RECEIVER).await.unwrap();
    assert_eq!(app.transport_id, "web-7");
    let media = Media {
        url,
        content_type: "audio/webm".into(),
        title: "Test".into(),
        ..Media::default()
    };
    let playing = client.load(&app, &media).await.unwrap();
    assert_eq!(playing.player_state, "PLAYING");
    let sought = client
        .seek(&app, playing.media_session_id, 30.0)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(sought.current_time, 30.0);
    let paused = client.media(&app, "PAUSE", 1).await.unwrap().unwrap();
    assert_eq!(paused.player_state, "PAUSED");
    client.stop_app(&app).await.unwrap();

    let log = accesses(&relay, 2).await;
    let seen = seen.lock().unwrap();
    assert_eq!(
        seen.fetched, file.bytes,
        "the device got the file through the relay"
    );
    let control: Vec<&str> = seen
        .messages
        .iter()
        .map(String::as_str)
        .filter(|m| !m.starts_with("heartbeat"))
        .collect();
    assert_eq!(
        control,
        [
            "connection CONNECT",
            "receiver GET_STATUS",
            "receiver LAUNCH",
            "connection CONNECT",
            "media LOAD",
            "media SEEK",
            "media PAUSE",
            "receiver STOP",
        ]
    );
    assert!(
        seen.messages.contains(&"heartbeat PONG".to_owned()),
        "the sender answered PING"
    );
    assert!(
        log.iter()
            .any(|a| a.range.as_deref() == Some("bytes=40000-") && a.status == 206)
    );
}
