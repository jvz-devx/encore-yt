//! The relay: an HTTP server on the LAN address the device reaches us by,
//! serving each published stream under an unguessable path. A stream is a
//! local file or a remote URL (googlevideo); remote bytes are fetched as the
//! device asks for them, its `Range` passed on, so seeking on the device
//! costs one range request upstream and nothing is buffered here.

use std::collections::{HashMap, VecDeque};
use std::net::{IpAddr, SocketAddr};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{Context, Result};
use futures_util::StreamExt;
use tokio::io::{AsyncSeekExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};
use tokio::task::{JoinHandle, JoinSet};

use crate::http::{self, Range, Request};
use crate::sync::lock;

const MAX_ENTRIES: usize = 128;
const MAX_ACCESSES: usize = 256;
const MAX_CONNECTIONS: usize = 32;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

/// What DLNA renderers expect next to the body: streaming transfer, and
/// `OP=01` (byte seeks supported). Cast receivers ignore them.
const DLNA_FEATURES: &str =
    "DLNA.ORG_OP=01;DLNA.ORG_CI=0;DLNA.ORG_FLAGS=01700000000000000000000000000000";

#[derive(Clone, Debug)]
pub enum Source {
    File(PathBuf),
    Remote(String),
}

/// One request the relay answered, for the spike's evidence: do receivers
/// use ranges, HEAD, several connections?
#[derive(Clone, Debug)]
pub struct Access {
    pub peer: SocketAddr,
    pub method: String,
    pub range: Option<String>,
    pub status: u16,
    pub bytes: u64,
    pub user_agent: String,
}

#[derive(Clone)]
struct Entry {
    source: Source,
    mime: String,
}

struct State {
    entries: Mutex<HashMap<String, Entry>>,
    log: Mutex<VecDeque<Access>>,
    http: reqwest::Client,
}

pub struct Relay {
    addr: SocketAddr,
    state: Arc<State>,
    task: JoinHandle<()>,
}

impl Relay {
    /// Listens on `ip` (the LAN address from `local_ip_for`), on a port the
    /// OS picks. Binding to that address rather than 0.0.0.0 keeps it off
    /// other networks (VPNs, containers).
    pub async fn start(ip: IpAddr) -> Result<Relay> {
        let listener = TcpListener::bind(SocketAddr::new(ip, 0))
            .await
            .context("bind relay")?;
        let addr = listener.local_addr().context("read relay address")?;
        let state = Arc::new(State {
            entries: Mutex::new(HashMap::new()),
            log: Mutex::new(VecDeque::new()),
            http: reqwest::Client::builder()
                .user_agent("Mozilla/5.0 (X11; Linux x86_64) encore-yt")
                .connect_timeout(REQUEST_TIMEOUT)
                .read_timeout(Duration::from_secs(30))
                .build()
                .context("build relay HTTP client")?,
        });
        let task = tokio::spawn(accept(listener, state.clone()));
        log::info!("relay listening on {addr}");
        Ok(Relay { addr, state, task })
    }

    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    /// Serves `source` as `mime` and returns its URL. The path ends with an
    /// extension for the renderers that guess the format from it.
    pub fn publish(&self, source: Source, mime: &str) -> Result<String> {
        let mut token = [0u8; 16];
        getrandom::fill(&mut token).map_err(|e| anyhow::anyhow!("random token: {e}"))?;
        let token: String = token.iter().map(|b| format!("{b:02x}")).collect();
        let ext = match mime.split(';').next().unwrap_or_default() {
            "audio/webm" => "webm",
            "audio/mp4" | "audio/x-m4a" => "m4a",
            "audio/ogg" => "ogg",
            "audio/mpeg" => "mp3",
            "audio/flac" => "flac",
            "audio/wav" | "audio/L16" => "wav",
            _ => "bin",
        };
        let path = format!("{token}.{ext}");
        let entry = Entry {
            source,
            mime: mime.to_owned(),
        };
        let mut entries = lock(&self.state.entries);
        anyhow::ensure!(
            entries.len() < MAX_ENTRIES,
            "unpublish old relay sources before adding more"
        );
        entries.insert(path.clone(), entry);
        Ok(format!("http://{}/s/{path}", self.addr))
    }

    pub fn unpublish_all(&self) {
        lock(&self.state.entries).clear();
    }

    /// The most recent 256 completed requests, oldest first.
    pub fn accesses(&self) -> Vec<Access> {
        lock(&self.state.log).iter().cloned().collect()
    }
}

impl Drop for Relay {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn accept(listener: TcpListener, state: Arc<State>) {
    // Dropping the accept task also aborts its active connections.
    let mut connections = JoinSet::new();
    loop {
        tokio::select! {
            ended = connections.join_next(), if !connections.is_empty() => {
                if let Some(Err(error)) = ended {
                    log::warn!("relay task ended: {error}");
                }
            }
            accepted = listener.accept(), if connections.len() < MAX_CONNECTIONS => {
                match accepted {
                    Ok((stream, peer)) => {
                        let state = state.clone();
                        connections.spawn(async move {
                            if let Err(error) = serve(stream, peer, &state).await {
                                log::debug!("relay: {peer}: {error:#}");
                            }
                        });
                    }
                    Err(error) => {
                        log::warn!("relay accept failed: {error}");
                        tokio::time::sleep(Duration::from_millis(100)).await;
                    }
                }
            }
        }
    }
}

async fn serve(stream: TcpStream, peer: SocketAddr, state: &State) -> Result<()> {
    let mut reader = BufReader::new(stream);
    let Some(request) = tokio::time::timeout(REQUEST_TIMEOUT, http::read_request(&mut reader))
        .await
        .context("relay request timed out")??
    else {
        return Ok(());
    };
    let mut stream = reader.into_inner();
    let entry = request
        .path
        .strip_prefix("/s/")
        .and_then(|path| lock(&state.entries).get(path).cloned());
    let (status, bytes) = match (&*request.method, entry) {
        ("GET" | "HEAD", Some(entry)) => match &entry.source {
            Source::File(path) => send_file(&mut stream, &request, path, &entry.mime).await?,
            Source::Remote(url) => {
                send_remote(&mut stream, &request, url, &entry.mime, &state.http).await?
            }
        },
        ("GET" | "HEAD", None) => {
            stream
                .write_all(
                    http::head(404, "Not Found", &[("Content-Length", "0".into())]).as_bytes(),
                )
                .await?;
            (404, 0)
        }
        _ => {
            stream
                .write_all(
                    http::head(405, "Method Not Allowed", &[("Content-Length", "0".into())])
                        .as_bytes(),
                )
                .await?;
            (405, 0)
        }
    };
    let access = Access {
        peer,
        method: request.method.clone(),
        range: request.header("range").map(str::to_owned),
        status,
        bytes,
        user_agent: request.header("user-agent").unwrap_or_default().to_owned(),
    };
    log::info!(
        "relay: {} {} range {:?} -> {} ({} bytes) [{}]",
        access.peer,
        access.method,
        access.range,
        access.status,
        access.bytes,
        access.user_agent
    );
    record_access(&state.log, access);
    Ok(())
}

fn record_access(log: &Mutex<VecDeque<Access>>, access: Access) {
    let mut log = lock(log);
    if log.len() == MAX_ACCESSES {
        log.pop_front();
    }
    log.push_back(access);
}

fn common_headers(mime: &str) -> Vec<(&'static str, String)> {
    vec![
        ("Content-Type", mime.to_owned()),
        ("Accept-Ranges", "bytes".into()),
        ("transferMode.dlna.org", "Streaming".into()),
        ("contentFeatures.dlna.org", DLNA_FEATURES.into()),
        // Cast receivers fetching with CORS (Shaka, MSE) need this; a plain
        // <audio> source doesn't.
        ("Access-Control-Allow-Origin", "*".into()),
    ]
}

async fn send_file(
    stream: &mut TcpStream,
    request: &Request,
    path: &PathBuf,
    mime: &str,
) -> Result<(u16, u64)> {
    let mut file = tokio::fs::File::open(path).await.context("open")?;
    let len = file.metadata().await?.len();
    let mut headers = common_headers(mime);
    let (status, start, count) = match http::range(request.header("range"), len) {
        Range::Full => (200, 0, len),
        Range::Part { start, end } => {
            headers.push(("Content-Range", format!("bytes {start}-{end}/{len}")));
            (206, start, end - start + 1)
        }
        Range::Unsatisfiable => {
            headers.push(("Content-Range", format!("bytes */{len}")));
            headers.push(("Content-Length", "0".into()));
            stream
                .write_all(http::head(416, "Range Not Satisfiable", &headers).as_bytes())
                .await?;
            return Ok((416, 0));
        }
    };
    headers.push(("Content-Length", count.to_string()));
    let reason = if status == 206 {
        "Partial Content"
    } else {
        "OK"
    };
    stream
        .write_all(http::head(status, reason, &headers).as_bytes())
        .await?;
    if request.method == "HEAD" {
        return Ok((status, 0));
    }
    file.seek(std::io::SeekFrom::Start(start)).await?;
    let mut body = tokio::io::AsyncReadExt::take(file, count);
    let sent = tokio::io::copy(&mut body, stream).await?;
    Ok((status, sent))
}

async fn send_remote(
    stream: &mut TcpStream,
    request: &Request,
    url: &str,
    mime: &str,
    client: &reqwest::Client,
) -> Result<(u16, u64)> {
    let method = if request.method == "HEAD" {
        reqwest::Method::HEAD
    } else {
        reqwest::Method::GET
    };
    let mut upstream = client.request(method, url);
    if let Some(range) = request.header("range") {
        upstream = upstream.header("Range", range);
    }
    let response = upstream.send().await.context("upstream")?;
    let status = response.status().as_u16();
    let mut headers = common_headers(mime);
    for name in ["content-length", "content-range"] {
        if let Some(value) = response.headers().get(name).and_then(|v| v.to_str().ok()) {
            let key = if name == "content-length" {
                "Content-Length"
            } else {
                "Content-Range"
            };
            headers.push((key, value.to_owned()));
        }
    }
    let reason = response.status().canonical_reason().unwrap_or("Upstream");
    stream
        .write_all(http::head(status, reason, &headers).as_bytes())
        .await?;
    let mut sent = 0u64;
    let mut body = response.bytes_stream();
    while let Some(chunk) = body.next().await {
        let chunk = chunk.context("upstream body")?;
        stream.write_all(&chunk).await?;
        sent += chunk.len() as u64;
    }
    Ok((status, sent))
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
    async fn published_sources_are_bounded_and_can_be_released() {
        let relay = Relay::start(std::net::Ipv4Addr::LOCALHOST.into())
            .await
            .unwrap();
        for _ in 0..MAX_ENTRIES {
            relay
                .publish(Source::File(PathBuf::from("fixture")), "audio/webm")
                .unwrap();
        }
        assert!(
            relay
                .publish(Source::File(PathBuf::from("fixture")), "audio/webm")
                .is_err()
        );
        relay.unpublish_all();
        assert!(
            relay
                .publish(Source::File(PathBuf::from("fixture")), "audio/webm")
                .is_ok()
        );
    }

    #[test]
    fn access_history_keeps_only_recent_requests() {
        let log = Mutex::new(VecDeque::new());
        for n in 0..MAX_ACCESSES + 3 {
            record_access(
                &log,
                Access {
                    peer: (std::net::Ipv4Addr::LOCALHOST, 1234).into(),
                    method: "GET".into(),
                    range: None,
                    status: 200,
                    bytes: n as u64,
                    user_agent: String::new(),
                },
            );
        }
        let entries = lock(&log);
        assert_eq!(entries.len(), MAX_ACCESSES);
        assert_eq!(entries.front().unwrap().bytes, 3);
        assert_eq!(entries.back().unwrap().bytes, (MAX_ACCESSES + 2) as u64);
    }
}
