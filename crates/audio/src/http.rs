//! A seekable HTTP source: bounded `Range` requests on a fetch thread into
//! one buffer the size of the file, read through `Read + Seek`.
//!
//! The fetch thread downloads ahead of the reader (up to [`READ_AHEAD`]) in
//! chunks of [`CHUNK`] bytes, as yt-dlp does for googlevideo (which throttles
//! open-ended requests). A read outside what is downloaded or about to be
//! moves the fetch there: the running request is dropped and a new `Range`
//! request starts at the read position. Bytes once downloaded stay, so a
//! seek back costs nothing. Network errors retry the chunk from where it
//! stopped; an HTTP error status (403 for an expired URL) ends the source.

use std::io::{self, Read, Seek, SeekFrom};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use reqwest::blocking::{Client, Response};
use reqwest::header::{CONTENT_RANGE, HeaderMap, RANGE};
use symphonia::core::io::MediaSource;

/// Bytes per range request.
pub const CHUNK: u64 = 2 << 20;
/// How far ahead of the reader the fetch thread downloads.
pub const READ_AHEAD: u64 = 32 << 20;
/// A read past the fetch position that the running request reaches within
/// this time waits for it instead of starting a new request (a new request
/// costs a round trip, ~100-300 ms on googlevideo).
const NEAR_SECS: f64 = 0.25;
const RETRIES: u32 = 3;

/// Counters for one source, for logs and measurements.
#[derive(Clone, Debug, Default)]
pub struct HttpStats {
    pub requests: u32,
    pub downloaded: u64,
    pub len: u64,
}

struct State {
    data: Vec<u8>,
    /// Downloaded byte ranges, sorted and merged.
    have: Vec<(u64, u64)>,
    /// Where the running request is writing next.
    fetch_pos: u64,
    /// Where the reader is; the fetch thread works ahead of it.
    read_pos: u64,
    /// Set by a read that the running request won't reach soon.
    restart: Option<u64>,
    error: Option<String>,
    closed: bool,
    stats: HttpStats,
    /// Bytes per second of the running request.
    throughput: f64,
}

impl State {
    /// How far ahead of the fetch position counts as "about to arrive".
    fn near(&self) -> u64 {
        ((self.throughput * NEAR_SECS) as u64).clamp(16 << 10, 1 << 20)
    }

    fn contiguous_end(&self, pos: u64) -> Option<u64> {
        self.have
            .iter()
            .find(|(s, e)| *s <= pos && pos < *e)
            .map(|(_, e)| *e)
    }

    /// The first byte at or after `pos` that is not downloaded.
    fn first_missing(&self, pos: u64) -> u64 {
        self.contiguous_end(pos).unwrap_or(pos)
    }

    fn mark(&mut self, start: u64, end: u64) {
        self.have.push((start, end));
        self.have.sort_unstable();
        let mut merged: Vec<(u64, u64)> = Vec::with_capacity(self.have.len());
        for (s, e) in self.have.drain(..) {
            match merged.last_mut() {
                Some(last) if s <= last.1 => last.1 = last.1.max(e),
                _ => merged.push((s, e)),
            }
        }
        self.have = merged;
    }
}

struct Shared {
    state: Mutex<State>,
    wake: Condvar,
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }
}

/// The reader half; the fetch thread ends when it is dropped.
pub struct HttpSource {
    shared: Arc<Shared>,
    pos: u64,
    len: u64,
}

impl HttpSource {
    /// Starts the first range request and returns once its headers arrive
    /// (they carry the file's length).
    pub fn open(client: &Client, url: &str, headers: HeaderMap) -> Result<Self> {
        let started = Instant::now();
        let response = request(client, url, &headers, 0, CHUNK - 1)?;
        let len = total_len(&response).context("server sent no Content-Range length")?;
        log::info!(
            "http: {} bytes, first response in {:?}",
            len,
            started.elapsed()
        );
        let shared = Arc::new(Shared {
            state: Mutex::new(State {
                // Zeroed pages are only committed as they are written.
                data: vec![0; len as usize],
                have: Vec::new(),
                fetch_pos: 0,
                read_pos: 0,
                restart: None,
                error: None,
                closed: false,
                throughput: 0.0,
                stats: HttpStats {
                    requests: 1,
                    downloaded: 0,
                    len,
                },
            }),
            wake: Condvar::new(),
        });
        let fetcher = Fetcher {
            shared: shared.clone(),
            client: client.clone(),
            url: url.to_owned(),
            headers,
            len,
        };
        thread::Builder::new()
            .name("audio-http".into())
            .spawn(move || fetcher.run(response))?;
        Ok(Self {
            shared,
            pos: 0,
            len,
        })
    }

    /// A local file, read whole at once (tests, `YTFAST_FAKE_STREAM`).
    pub fn local(path: &str) -> Result<Self> {
        let data = std::fs::read(path).with_context(|| format!("reading {path}"))?;
        let len = data.len() as u64;
        let shared = Arc::new(Shared {
            state: Mutex::new(State {
                data,
                have: vec![(0, len)],
                fetch_pos: len,
                read_pos: 0,
                restart: None,
                error: None,
                closed: false,
                throughput: 0.0,
                stats: HttpStats {
                    requests: 0,
                    downloaded: len,
                    len,
                },
            }),
            wake: Condvar::new(),
        });
        Ok(Self {
            shared,
            pos: 0,
            len,
        })
    }

    /// A handle for reading the counters while the decoder owns the source.
    pub fn stats_handle(&self) -> StatsHandle {
        StatsHandle(self.shared.clone())
    }
}

/// Reads [`HttpStats`] from another thread.
#[derive(Clone)]
pub struct StatsHandle(Arc<Shared>);

impl StatsHandle {
    pub fn get(&self) -> HttpStats {
        self.0.lock().stats.clone()
    }

    /// Up to `n` of the file's last bytes, as far as they are downloaded.
    pub fn tail(&self, n: usize) -> Vec<u8> {
        let state = self.0.lock();
        let len = state.data.len() as u64;
        match state.have.iter().find(|(_, e)| *e == len) {
            Some((s, _)) => state.data[(*s).max(len.saturating_sub(n as u64)) as usize..].to_vec(),
            None => Vec::new(),
        }
    }
}

impl Drop for HttpSource {
    fn drop(&mut self) {
        self.shared.lock().closed = true;
        self.shared.wake.notify_all();
    }
}

impl Read for HttpSource {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.pos >= self.len || buf.is_empty() {
            return Ok(0);
        }
        let mut state = self.shared.lock();
        state.read_pos = self.pos;
        loop {
            if let Some(end) = state.contiguous_end(self.pos) {
                let n = buf.len().min((end - self.pos) as usize);
                let at = self.pos as usize;
                buf[..n].copy_from_slice(&state.data[at..at + n]);
                self.pos += n as u64;
                state.read_pos = self.pos;
                self.shared.wake.notify_all();
                return Ok(n);
            }
            if let Some(error) = &state.error {
                return Err(io::Error::other(error.clone()));
            }
            let coming = self.pos >= state.fetch_pos && self.pos < state.fetch_pos + state.near();
            if !coming && state.restart != Some(self.pos) {
                state.restart = Some(self.pos);
                self.shared.wake.notify_all();
            }
            state = self
                .shared
                .wake
                .wait(state)
                .unwrap_or_else(|e| e.into_inner());
        }
    }
}

impl Seek for HttpSource {
    fn seek(&mut self, to: SeekFrom) -> io::Result<u64> {
        let pos = match to {
            SeekFrom::Start(p) => p as i64,
            SeekFrom::End(d) => self.len as i64 + d,
            SeekFrom::Current(d) => self.pos as i64 + d,
        };
        if pos < 0 {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "seek before 0"));
        }
        self.pos = pos as u64;
        Ok(self.pos)
    }
}

impl MediaSource for HttpSource {
    fn is_seekable(&self) -> bool {
        true
    }

    fn byte_len(&self) -> Option<u64> {
        Some(self.len)
    }
}

struct Fetcher {
    shared: Arc<Shared>,
    client: Client,
    url: String,
    headers: HeaderMap,
    len: u64,
}

enum Copied {
    Done,
    /// The reader wants another position.
    Moved,
    Closed,
    Failed(String),
}

impl Fetcher {
    fn run(self, first: Response) {
        let mut response = Some((0, first));
        let mut failures = 0;
        loop {
            let (start, body) = match response.take() {
                Some(r) => r,
                None => match self.next_start() {
                    Some(start) => {
                        let end = (start + CHUNK).min(self.len) - 1;
                        match request(&self.client, &self.url, &self.headers, start, end) {
                            Ok(r) => {
                                self.shared.lock().stats.requests += 1;
                                (start, r)
                            }
                            Err(e) if is_status(&e) => return self.fail(format!("{e:#}")),
                            Err(e) => {
                                if self.retry(&mut failures, format!("{e:#}")) {
                                    continue;
                                }
                                return;
                            }
                        }
                    }
                    None => return,
                },
            };
            match self.copy(start, body) {
                Copied::Done | Copied::Moved => failures = 0,
                Copied::Closed => return,
                Copied::Failed(e) => {
                    if !self.retry(&mut failures, e) {
                        return;
                    }
                }
            }
        }
    }

    /// Waits until there is something to download and returns where.
    fn next_start(&self) -> Option<u64> {
        let mut state = self.shared.lock();
        loop {
            if state.closed {
                return None;
            }
            if let Some(pos) = state.restart.take() {
                let start = state.first_missing(pos);
                if start < self.len {
                    log::info!("http: range request at {start} (read moved)");
                    state.fetch_pos = start;
                    return Some(start);
                }
            }
            let start = state.first_missing(state.read_pos);
            if start < self.len && start < state.read_pos + READ_AHEAD {
                state.fetch_pos = start;
                return Some(start);
            }
            state = self
                .shared
                .wake
                .wait(state)
                .unwrap_or_else(|e| e.into_inner());
        }
    }

    fn copy(&self, start: u64, mut body: Response) -> Copied {
        let mut buf = vec![0u8; 64 << 10];
        let mut pos = start;
        let began = Instant::now();
        loop {
            let n = match body.read(&mut buf) {
                Ok(0) => return Copied::Done,
                Ok(n) => n,
                Err(e) => return Copied::Failed(e.to_string()),
            };
            let n = n.min((self.len - pos) as usize);
            let mut state = self.shared.lock();
            if state.closed {
                return Copied::Closed;
            }
            let at = pos as usize;
            state.data[at..at + n].copy_from_slice(&buf[..n]);
            state.mark(pos, pos + n as u64);
            state.stats.downloaded += n as u64;
            pos += n as u64;
            state.fetch_pos = pos;
            let secs = began.elapsed().as_secs_f64().max(1e-3);
            state.throughput = (pos - start) as f64 / secs;
            self.shared.wake.notify_all();
            if let Some(want) = state.restart {
                let reached = want >= start && want < pos + state.near();
                if reached {
                    state.restart = None;
                } else {
                    return Copied::Moved;
                }
            }
            if pos >= self.len {
                return Copied::Done;
            }
        }
    }

    fn retry(&self, failures: &mut u32, error: String) -> bool {
        *failures += 1;
        if *failures > RETRIES {
            self.fail(error);
            return false;
        }
        log::warn!("http: {error}; retry {failures}/{RETRIES}");
        thread::sleep(Duration::from_millis(250 << *failures));
        true
    }

    fn fail(&self, error: String) {
        log::warn!("http: giving up: {error}");
        self.shared.lock().error = Some(error);
        self.shared.wake.notify_all();
    }
}

#[derive(Debug)]
struct Status(reqwest::StatusCode);

impl std::fmt::Display for Status {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "HTTP {}", self.0)
    }
}

impl std::error::Error for Status {}

fn is_status(error: &anyhow::Error) -> bool {
    error.downcast_ref::<Status>().is_some()
}

fn request(
    client: &Client,
    url: &str,
    headers: &HeaderMap,
    start: u64,
    end: u64,
) -> Result<Response> {
    let response = client
        .get(url)
        .headers(headers.clone())
        .header(RANGE, format!("bytes={start}-{end}"))
        .send()?;
    let status = response.status();
    if status != reqwest::StatusCode::PARTIAL_CONTENT {
        if status.is_success() {
            bail!("server ignored the Range header ({status})");
        }
        return Err(Status(status).into());
    }
    Ok(response)
}

/// The total from `Content-Range: bytes a-b/total`.
fn total_len(response: &Response) -> Option<u64> {
    let value = response.headers().get(CONTENT_RANGE)?.to_str().ok()?;
    value.rsplit('/').next()?.trim().parse().ok()
}
