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
    payloads: Vec<Value>,
    finished: bool,
    finished_empty: bool,
    replaced: bool,
    busy: bool,
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
        tls.write_all(&ping.frame().unwrap()).await.unwrap();
        let mut content = String::new();
        let mut running = false;
        let mut position = 0.0;
        let mut playing = true;
        let mut receiver_level = 0.3;
        let mut receiver_muted = false;
        let mut media_level = 1.0;
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
            seen.lock().unwrap().payloads.push(payload.clone());
            let id = payload["requestId"].clone();
            let reply = match (msg.namespace.as_str(), kind.as_str()) {
                (NS_RECEIVER, "GET_STATUS") => {
                    let s = seen.lock().unwrap();
                    let mut reply = receiver_status(id, running || s.busy);
                    if s.replaced {
                        reply["status"]["applications"][0]["sessionId"] = json!("another-sender");
                    }
                    Some(reply)
                }
                (NS_RECEIVER, "LAUNCH") => {
                    running = true;
                    Some(receiver_status(id, true))
                }
                (NS_RECEIVER, "STOP") => {
                    running = false;
                    Some(receiver_status(id, false))
                }
                (NS_RECEIVER, "SET_VOLUME") => {
                    receiver_level = payload["volume"]["level"].as_f64().unwrap();
                    receiver_muted = payload["volume"]["muted"].as_bool().unwrap();
                    Some(receiver_status(id, running))
                }
                (NS_MEDIA, "LOAD") => {
                    assert_eq!(msg.destination, "web-7");
                    content = payload["media"]["contentId"].as_str().unwrap().to_owned();
                    let (_, bytes) = super::fetch(&content, Some("bytes=0-")).await;
                    seen.lock().unwrap().fetched = bytes;
                    position = payload["currentTime"].as_f64().unwrap();
                    playing = payload["autoplay"].as_bool().unwrap();
                    Some(media_status(
                        id,
                        if playing { "PLAYING" } else { "PAUSED" },
                        position,
                    ))
                }
                (NS_MEDIA, "SEEK") => {
                    let at = payload["currentTime"].as_f64().unwrap();
                    position = at;
                    super::fetch(&content, Some("bytes=40000-")).await;
                    Some(media_status(id, "PLAYING", at))
                }
                (NS_MEDIA, "PAUSE" | "PLAY") => {
                    playing = kind == "PLAY";
                    Some(media_status(
                        id,
                        if playing { "PLAYING" } else { "PAUSED" },
                        position,
                    ))
                }
                (NS_MEDIA, "SET_VOLUME" | "GET_STATUS") => {
                    if kind == "SET_VOLUME" {
                        media_level = payload["volume"]["level"].as_f64().unwrap();
                    }
                    let finished = seen.lock().unwrap().finished;
                    let empty = seen.lock().unwrap().finished_empty && kind == "GET_STATUS";
                    let mut reply = media_status(
                        id,
                        if finished {
                            "IDLE"
                        } else if playing {
                            "PLAYING"
                        } else {
                            "PAUSED"
                        },
                        position,
                    );
                    if finished {
                        reply["status"][0]["idleReason"] = json!("FINISHED");
                    }
                    if finished && empty {
                        let event = CastMessage::new(
                            "web-7",
                            &msg.source,
                            NS_MEDIA,
                            media_status(json!(0), "IDLE", position).to_string(),
                        );
                        let mut payload: Value =
                            serde_json::from_str(event.payload.as_deref().unwrap()).unwrap();
                        payload["status"][0]["idleReason"] = json!("FINISHED");
                        let event =
                            CastMessage::new("web-7", &msg.source, NS_MEDIA, payload.to_string());
                        tls.write_all(&event.frame().unwrap()).await.unwrap();
                        reply["status"] = json!([]);
                    }
                    Some(reply)
                }
                _ => None,
            };
            if let Some(mut reply) = reply {
                if msg.namespace == NS_MEDIA
                    && reply["status"].as_array().is_some_and(|s| !s.is_empty())
                {
                    reply["status"][0]["volume"] = json!({"level": media_level, "muted": false});
                }
                if msg.namespace == NS_RECEIVER {
                    reply["status"]["volume"]["level"] = json!(receiver_level);
                    reply["status"]["volume"]["muted"] = json!(receiver_muted);
                }
                let ns = msg.namespace.clone();
                let out = CastMessage::new(&msg.destination, &msg.source, &ns, reply.to_string());
                tls.write_all(&out.frame().unwrap()).await.unwrap();
            }
        }
    });
    addr
}

#[tokio::test]
async fn receiver_volume_can_be_snapshotted_and_restored_without_launching_media() {
    let seen = Arc::new(Mutex::new(Seen::default()));
    let addr = fake_device(seen.clone()).await;
    let client = Client::connect(addr).await.unwrap();
    let original = client.receiver_status().await.unwrap();
    assert_eq!(original.volume, Some(0.3));
    assert_eq!(
        client.receiver_volume(0.15, false).await.unwrap().volume,
        Some(0.15)
    );
    let restored = client
        .receiver_volume(original.volume.unwrap(), original.muted)
        .await
        .unwrap();
    assert_eq!(restored.volume, original.volume);
    assert_eq!(restored.muted, original.muted);
    assert!(
        !seen
            .lock()
            .unwrap()
            .payloads
            .iter()
            .any(|p| matches!(p["type"].as_str(), Some("LAUNCH" | "LOAD")))
    );
}

#[tokio::test]
async fn cast_finished_broadcast_survives_an_empty_polled_media_status() {
    let seen = Arc::new(Mutex::new(Seen::default()));
    let addr = fake_device(seen.clone()).await;
    let crate::session::Connection::Ready(mut session) = crate::session::Session::connect(
        &device(addr),
        crate::discovery::Policy {
            local_only: true,
            shield_only: false,
        },
        false,
    )
    .await
    .unwrap() else {
        panic!("idle fake")
    };
    let file = TestFile::new(80_000);
    session
        .load(
            Source::File(file.path.clone()),
            &Media {
                content_type: "audio/webm".into(),
                ..Default::default()
            },
            0.0,
            true,
            50.0,
        )
        .await
        .unwrap();
    {
        let mut seen = seen.lock().unwrap();
        seen.finished = true;
        seen.finished_empty = true;
    }
    assert!(session.poll().await.unwrap().finished);
    session.stop().await.unwrap();
}

fn device(addr: SocketAddr) -> crate::Device {
    crate::Device::Cast(crate::mdns::CastDevice {
        name: "Local Cast receiver".into(),
        model: "Test".into(),
        id: "fake".into(),
        addr,
        status: String::new(),
    })
}

#[tokio::test]
async fn remote_session_keeps_position_pause_volume_and_finish_on_the_receiver() {
    use crate::discovery::Policy;
    use crate::session::{Connection, Session};
    let file = TestFile::new(80_000);
    let seen = Arc::new(Mutex::new(Seen::default()));
    let addr = fake_device(seen.clone()).await;
    let Connection::Ready(mut session) = Session::connect(
        &device(addr),
        Policy {
            local_only: true,
            shield_only: false,
        },
        false,
    )
    .await
    .unwrap() else {
        panic!("idle fake should connect")
    };
    let media = Media {
        content_type: "audio/webm".into(),
        title: "Song".into(),
        duration: Some(180.0),
        ..Media::default()
    };
    let status = session
        .load(Source::File(file.path.clone()), &media, 12.0, true, 35.0)
        .await
        .unwrap();
    assert_eq!(status.position, 12.0);
    assert!(status.playing);
    session.pause(true).await.unwrap();
    assert!(!session.poll().await.unwrap().playing);
    session.seek(30.0).await.unwrap();
    session.pause(false).await.unwrap();
    session.volume(25.0).await.unwrap();
    assert_eq!(session.poll().await.unwrap().position, 30.0);
    seen.lock().unwrap().finished = true;
    assert!(session.poll().await.unwrap().finished);
    session.stop().await.unwrap();
    let seen = seen.lock().unwrap();
    assert_eq!(seen.fetched, file.bytes);
    let load = seen.payloads.iter().find(|v| v["type"] == "LOAD").unwrap();
    assert_eq!(load["currentTime"], 12.0);
    assert!(
        load["media"]["contentId"]
            .as_str()
            .unwrap()
            .starts_with("http://127.0.0.1:")
    );
    assert!(
        seen.payloads
            .iter()
            .any(|v| v["type"] == "SET_VOLUME" && v["volume"]["level"] == 0.25)
    );
}

#[tokio::test]
async fn busy_cast_receiver_is_not_launched_without_confirmation() {
    let seen = Arc::new(Mutex::new(Seen {
        busy: true,
        ..Seen::default()
    }));
    let addr = fake_device(seen.clone()).await;
    let result = crate::session::Session::connect(
        &device(addr),
        crate::discovery::Policy {
            local_only: true,
            shield_only: false,
        },
        false,
    )
    .await
    .unwrap();
    assert!(matches!(result, crate::session::Connection::Busy(_)));
    let seen = seen.lock().unwrap();
    assert!(
        !seen
            .payloads
            .iter()
            .any(|v| matches!(v["type"].as_str(), Some("LAUNCH" | "LOAD" | "STOP")))
    );
}

#[tokio::test]
async fn replaced_cast_session_is_reported_as_lost_not_finished() {
    let seen = Arc::new(Mutex::new(Seen::default()));
    let addr = fake_device(seen.clone()).await;
    let crate::session::Connection::Ready(mut session) = crate::session::Session::connect(
        &device(addr),
        crate::discovery::Policy {
            local_only: true,
            shield_only: false,
        },
        false,
    )
    .await
    .unwrap() else {
        panic!("idle fake")
    };
    let file = TestFile::new(80_000);
    session
        .load(
            Source::File(file.path.clone()),
            &Media {
                content_type: "audio/webm".into(),
                ..Media::default()
            },
            0.0,
            true,
            50.0,
        )
        .await
        .unwrap();
    seen.lock().unwrap().replaced = true;
    assert!(
        session
            .poll()
            .await
            .unwrap_err()
            .to_string()
            .contains("replaced")
    );
    drop(session);
    assert!(
        !seen
            .lock()
            .unwrap()
            .payloads
            .iter()
            .any(|v| v["type"] == "STOP")
    );
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
