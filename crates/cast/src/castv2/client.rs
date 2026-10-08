//! A Cast v2 sender connection: TLS to the device's port 8009, framed
//! `CastMessage`s, answers to the device's heartbeat, and requests matched
//! to their replies by `requestId`.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex as StdMutex};
use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail};
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::{CryptoProvider, verify_tls12_signature, verify_tls13_signature};
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{DigitallySignedStruct, SignatureScheme};
use serde_json::Value;
use tokio::io::{AsyncReadExt, AsyncWriteExt, ReadHalf, WriteHalf};
use tokio::net::TcpStream;
use tokio::sync::{Mutex, mpsc, oneshot};
use tokio::task::JoinHandle;
use tokio_rustls::TlsConnector;
use tokio_rustls::client::TlsStream;

use super::messages::{
    self, App, Media, MediaStatus, NS_CONNECTION, NS_HEARTBEAT, NS_MEDIA, NS_RECEIVER, PLATFORM,
    ReceiverStatus,
};
use super::proto::{CastMessage, MAX_MESSAGE};

/// The sender's endpoint name on the device.
const SENDER: &str = "sender-encore";
const REPLY_TIMEOUT: Duration = Duration::from_secs(15);

type Writer = Arc<Mutex<WriteHalf<TlsStream<TcpStream>>>>;
type Pending = Arc<StdMutex<HashMap<u64, oneshot::Sender<Value>>>>;

/// A message nobody asked for: status broadcasts (MEDIA_STATUS as playback
/// moves, RECEIVER_STATUS when another sender takes over), CLOSE.
#[derive(Debug)]
pub struct Event {
    pub source: String,
    pub namespace: String,
    pub payload: Value,
}

pub struct Client {
    writer: Writer,
    pending: Pending,
    next_id: AtomicU64,
    pub events: mpsc::UnboundedReceiver<Event>,
    tasks: Vec<JoinHandle<()>>,
}

impl Drop for Client {
    fn drop(&mut self) {
        for task in &self.tasks {
            task.abort();
        }
    }
}

impl Client {
    /// Opens the TLS channel and connects to the device's platform endpoint.
    pub async fn connect(addr: SocketAddr) -> Result<Client> {
        let tcp = tokio::time::timeout(Duration::from_secs(5), TcpStream::connect(addr))
            .await
            .context("connect timed out")??;
        tcp.set_nodelay(true)?;
        let tls = connector()?
            .connect(ServerName::IpAddress(addr.ip().into()), tcp)
            .await
            .context("TLS handshake")?;
        let (read, write) = tokio::io::split(tls);
        let writer: Writer = Arc::new(Mutex::new(write));
        let pending: Pending = Arc::default();
        let (events_tx, events) = mpsc::unbounded_channel();
        let reader = tokio::spawn(read_loop(read, writer.clone(), pending.clone(), events_tx));
        let pinger = tokio::spawn(ping_loop(writer.clone()));
        let client = Client {
            writer,
            pending,
            next_id: AtomicU64::new(1),
            events,
            tasks: vec![reader, pinger],
        };
        client
            .send(PLATFORM, NS_CONNECTION, &messages::connect())
            .await?;
        Ok(client)
    }

    pub async fn send(&self, destination: &str, namespace: &str, payload: &Value) -> Result<()> {
        send(&self.writer, destination, namespace, payload).await
    }

    /// Sends a payload built with a fresh `requestId` and waits for the reply
    /// carrying it.
    pub async fn request(
        &self,
        destination: &str,
        namespace: &str,
        build: impl FnOnce(u64) -> Value,
    ) -> Result<Value> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = oneshot::channel();
        self.pending.lock().expect("pending").insert(id, tx);
        self.send(destination, namespace, &build(id)).await?;
        let reply = tokio::time::timeout(REPLY_TIMEOUT, rx)
            .await
            .map_err(|_| anyhow!("no reply to request {id} on {namespace}"))?
            .map_err(|_| anyhow!("the device closed the connection"))?;
        Ok(reply)
    }

    pub async fn receiver_status(&self) -> Result<ReceiverStatus> {
        let reply = self
            .request(PLATFORM, NS_RECEIVER, messages::get_status)
            .await?;
        Ok(ReceiverStatus::parse(&reply))
    }

    /// Launches `app_id` (or finds it already running) and connects to it.
    pub async fn launch(&self, app_id: &str) -> Result<App> {
        let reply = self
            .request(PLATFORM, NS_RECEIVER, |id| messages::launch(id, app_id))
            .await?;
        if reply["type"] != "RECEIVER_STATUS" {
            bail!("launch failed: {}", reply);
        }
        let app = ReceiverStatus::parse(&reply)
            .apps
            .into_iter()
            .find(|a| a.app_id == app_id)
            .context("the device didn't start the app")?;
        self.send(&app.transport_id, NS_CONNECTION, &messages::connect())
            .await?;
        Ok(app)
    }

    pub async fn load(&self, app: &App, media: &Media) -> Result<MediaStatus> {
        let reply = self
            .request(&app.transport_id, NS_MEDIA, |id| {
                messages::load(id, &app.session_id, media)
            })
            .await?;
        if reply["type"] != "MEDIA_STATUS" {
            bail!("load failed: {}", reply);
        }
        MediaStatus::parse(&reply).context("no media status after LOAD")
    }

    /// PLAY, PAUSE, STOP or GET_STATUS on the loaded media.
    pub async fn media(
        &self,
        app: &App,
        kind: &str,
        media_session_id: i64,
    ) -> Result<Option<MediaStatus>> {
        let reply = self
            .request(&app.transport_id, NS_MEDIA, |id| {
                messages::media_command(id, kind, media_session_id)
            })
            .await?;
        Ok(MediaStatus::parse(&reply))
    }

    pub async fn seek(
        &self,
        app: &App,
        media_session_id: i64,
        seconds: f64,
    ) -> Result<Option<MediaStatus>> {
        let reply = self
            .request(&app.transport_id, NS_MEDIA, |id| {
                messages::seek(id, media_session_id, seconds)
            })
            .await?;
        Ok(MediaStatus::parse(&reply))
    }

    /// Stops the app on the device (the receiver returns to its idle screen).
    pub async fn stop_app(&self, app: &App) -> Result<()> {
        self.request(PLATFORM, NS_RECEIVER, |id| {
            messages::stop(id, &app.session_id)
        })
        .await?;
        Ok(())
    }
}

async fn send(writer: &Writer, destination: &str, namespace: &str, payload: &Value) -> Result<()> {
    let frame = CastMessage::new(SENDER, destination, namespace, payload.to_string()).frame();
    let mut writer = writer.lock().await;
    writer.write_all(&frame).await?;
    writer.flush().await?;
    Ok(())
}

async fn read_loop(
    mut read: ReadHalf<TlsStream<TcpStream>>,
    writer: Writer,
    pending: Pending,
    events: mpsc::UnboundedSender<Event>,
) {
    loop {
        let message = match read_message(&mut read).await {
            Ok(message) => message,
            Err(error) => {
                log::info!("cast: connection ended: {error:#}");
                pending.lock().expect("pending").clear();
                return;
            }
        };
        let Some(payload) = message
            .payload
            .as_deref()
            .and_then(|p| serde_json::from_str::<Value>(p).ok())
        else {
            continue;
        };
        if message.namespace == NS_HEARTBEAT && payload["type"] == "PING" {
            let _ = send(&writer, &message.source, NS_HEARTBEAT, &messages::pong()).await;
            continue;
        }
        let waiting = payload["requestId"]
            .as_u64()
            .filter(|id| *id != 0)
            .and_then(|id| pending.lock().expect("pending").remove(&id));
        match waiting {
            Some(tx) => {
                let _ = tx.send(payload);
            }
            None => {
                let _ = events.send(Event {
                    source: message.source,
                    namespace: message.namespace,
                    payload,
                });
            }
        }
    }
}

async fn read_message(read: &mut ReadHalf<TlsStream<TcpStream>>) -> Result<CastMessage> {
    let mut len = [0u8; 4];
    read.read_exact(&mut len).await?;
    let len = u32::from_be_bytes(len) as usize;
    if len > MAX_MESSAGE {
        bail!("message of {len} bytes");
    }
    let mut body = vec![0u8; len];
    read.read_exact(&mut body).await?;
    CastMessage::decode(&body)
}

/// Devices close a channel that stays quiet; a PING every 5 s keeps it.
async fn ping_loop(writer: Writer) {
    let mut every = tokio::time::interval(Duration::from_secs(5));
    loop {
        every.tick().await;
        if send(&writer, PLATFORM, NS_HEARTBEAT, &messages::ping())
            .await
            .is_err()
        {
            return;
        }
    }
}

/// TLS for Cast devices. Their certificates are self-signed per device, so
/// there is no chain to check; proving the device is genuine is the optional
/// `deviceauth` challenge, which a media sender can skip. The handshake
/// signature is still verified against the certificate the device sent.
fn connector() -> Result<TlsConnector> {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let config = rustls::ClientConfig::builder_with_provider(provider.clone())
        .with_safe_default_protocol_versions()?
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(AnyCertificate(provider)))
        .with_no_client_auth();
    Ok(TlsConnector::from(Arc::new(config)))
}

#[derive(Debug)]
struct AnyCertificate(Arc<CryptoProvider>);

impl ServerCertVerifier for AnyCertificate {
    fn verify_server_cert(
        &self,
        _end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        verify_tls12_signature(
            message,
            cert,
            dss,
            &self.0.signature_verification_algorithms,
        )
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        verify_tls13_signature(
            message,
            cert,
            dss,
            &self.0.signature_verification_algorithms,
        )
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.0.signature_verification_algorithms.supported_schemes()
    }
}
