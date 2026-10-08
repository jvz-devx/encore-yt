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
use crate::sync::lock;

/// The sender's endpoint name on the device.
const SENDER: &str = "sender-encore";
const REPLY_TIMEOUT: Duration = Duration::from_secs(15);
const MAX_PENDING: usize = 128;
const MAX_EVENTS: usize = 128;

type Writer = Arc<Mutex<WriteHalf<TlsStream<TcpStream>>>>;
type Pending = Arc<StdMutex<HashMap<u64, oneshot::Sender<Value>>>>;

/// Removes a request on success, send failure, timeout or future cancellation.
struct PendingRequest<'a> {
    pending: &'a Pending,
    id: u64,
}

impl Drop for PendingRequest<'_> {
    fn drop(&mut self) {
        lock(self.pending).remove(&self.id);
    }
}

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
    pub events: mpsc::Receiver<Event>,
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
        let tls = tokio::time::timeout(
            REPLY_TIMEOUT,
            connector()?.connect(ServerName::IpAddress(addr.ip().into()), tcp),
        )
        .await
        .context("TLS handshake timed out")?
        .context("TLS handshake")?;
        let (read, write) = tokio::io::split(tls);
        let writer: Writer = Arc::new(Mutex::new(write));
        let pending: Pending = Arc::default();
        let (events_tx, events) = mpsc::channel(MAX_EVENTS);
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
        {
            let mut pending = lock(&self.pending);
            anyhow::ensure!(
                pending.len() < MAX_PENDING,
                "too many pending Cast requests"
            );
            pending.insert(id, tx);
        }
        let _registration = PendingRequest {
            pending: &self.pending,
            id,
        };
        tokio::time::timeout(REPLY_TIMEOUT, async {
            self.send(destination, namespace, &build(id)).await?;
            rx.await
                .map_err(|_| anyhow!("the device closed the connection"))
        })
        .await
        .map_err(|_| anyhow!("no reply to request {id} on {namespace}"))?
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
        self.load_at(app, media, 0.0, true).await
    }

    pub async fn load_at(
        &self,
        app: &App,
        media: &Media,
        at: f64,
        playing: bool,
    ) -> Result<MediaStatus> {
        let reply = self
            .request(&app.transport_id, NS_MEDIA, |id| {
                messages::load_at(id, &app.session_id, media, at, playing)
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
        anyhow::ensure!(reply["type"] == "MEDIA_STATUS", "media request refused");
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
        anyhow::ensure!(reply["type"] == "MEDIA_STATUS", "seek request refused");
        Ok(MediaStatus::parse(&reply))
    }

    pub async fn volume(&self, app: &App, media_session_id: i64, level: f64) -> Result<()> {
        let reply = self
            .request(&app.transport_id, NS_MEDIA, |id| {
                messages::media_volume(id, media_session_id, level.clamp(0.0, 1.0))
            })
            .await?;
        anyhow::ensure!(reply["type"] == "MEDIA_STATUS", "volume request refused");
        Ok(())
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
    let frame = CastMessage::new(SENDER, destination, namespace, payload.to_string()).frame()?;
    let mut writer = writer.lock().await;
    writer.write_all(&frame).await.context("write Cast frame")?;
    writer.flush().await.context("flush Cast frame")?;
    Ok(())
}

async fn read_loop(
    mut read: ReadHalf<TlsStream<TcpStream>>,
    writer: Writer,
    pending: Pending,
    events: mpsc::Sender<Event>,
) {
    loop {
        let message = match read_message(&mut read).await {
            Ok(message) => message,
            Err(error) => {
                log::info!("cast: connection ended: {error:#}");
                lock(&pending).clear();
                return;
            }
        };
        let Some(text) = message.payload.as_deref() else {
            // Binary device-auth messages do not carry JSON.
            continue;
        };
        let payload = match serde_json::from_str::<Value>(text) {
            Ok(payload) => payload,
            Err(error) => {
                log::debug!("cast: invalid JSON reply: {error}");
                continue;
            }
        };
        if message.namespace == NS_HEARTBEAT && payload["type"] == "PING" {
            if let Err(error) =
                send(&writer, &message.source, NS_HEARTBEAT, &messages::pong()).await
            {
                log::debug!("cast: heartbeat failed: {error:#}");
                lock(&pending).clear();
                return;
            }
            continue;
        }
        let waiting = payload["requestId"]
            .as_u64()
            .filter(|id| *id != 0)
            .and_then(|id| lock(&pending).remove(&id));
        match waiting {
            Some(tx) => {
                // The request future may have been cancelled after removing its sender.
                let _ = tx.send(payload);
            }
            None => {
                if let Err(error) = events.try_send(Event {
                    source: message.source,
                    namespace: message.namespace,
                    payload,
                }) {
                    // Unsolicited broadcasts must not block heartbeat or request replies.
                    log::debug!("cast: discarded status broadcast: {error}");
                }
            }
        }
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "test assertions report fixture failures"
)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn cancelling_a_request_releases_its_pending_sender() {
        let pending: Pending = Arc::default();
        let (tx, mut rx) = oneshot::channel();
        lock(&pending).insert(1, tx);
        let task_pending = pending.clone();
        let (ready_tx, ready_rx) = oneshot::channel();
        let task = tokio::spawn(async move {
            let _registration = PendingRequest {
                pending: &task_pending,
                id: 1,
            };
            ready_tx.send(()).unwrap();
            std::future::pending::<()>().await;
        });
        ready_rx.await.unwrap();
        task.abort();
        assert!(task.await.unwrap_err().is_cancelled());
        assert!(lock(&pending).is_empty());
        assert!(matches!(
            rx.try_recv(),
            Err(oneshot::error::TryRecvError::Closed)
        ));
    }
}

async fn read_message(read: &mut ReadHalf<TlsStream<TcpStream>>) -> Result<CastMessage> {
    let mut len = [0u8; 4];
    read.read_exact(&mut len)
        .await
        .context("read Cast frame length")?;
    let len = usize::try_from(u32::from_be_bytes(len)).context("Cast frame length")?;
    if len > MAX_MESSAGE {
        bail!("message of {len} bytes");
    }
    let mut body = vec![0u8; len];
    read.read_exact(&mut body)
        .await
        .context("read Cast frame body")?;
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
