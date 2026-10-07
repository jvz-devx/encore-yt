//! Turns a video id into a playable audio URL, in Rust (`crate::streams`:
//! InnerTube and the embedded JS engine; docs/gpui/RESOLVER.md).
//!
//! Resolves run in parallel at two priorities. Playback (the current song,
//! then the next one) has its own slots and never waits behind speculation;
//! speculation (songs on screen, under the pointer, further ahead in the
//! queue) has two more slots and keeps a short most-likely-first backlog. A
//! song asked for twice shares one run, and a playback run nobody waits for
//! any more is stopped. Results are cached until ten minutes before the URL
//! expires, in memory and in the runtime directory (0600), so a relaunch
//! can start at once. A song whose stream failed to play is resolved again
//! without the account (`crate::streams::Native::resolve`).

use std::collections::{HashMap, HashSet, VecDeque};
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, anyhow, bail};
use serde::{Deserialize, Serialize};
use tokio::sync::{OwnedSemaphorePermit, Semaphore, watch};

use crate::innertube::Stream;

/// Development and testing: `ENCORE_FAKE_STREAM=<audio file>` plays that
/// local file for every song instead of resolving streams, so
/// UI, performance and effects checks don't make stream requests to
/// YouTube (whose rate limits an account can hit). Plays aren't reported to
/// history in this mode.
pub fn fake_stream() -> Option<Stream> {
    static FAKE: std::sync::OnceLock<Option<String>> = std::sync::OnceLock::new();
    FAKE.get_or_init(|| {
        let path = std::env::var("ENCORE_FAKE_STREAM").ok()?;
        log::warn!("ENCORE_FAKE_STREAM is set: every song plays {path}");
        Some(path)
    })
    .as_ref()
    .map(|path| Stream {
        itag: 251,
        url: path.clone(),
        user_agent: None,
        expires: now() + 24 * 3600,
    })
}

/// Runs at once for playback: a click can start while the song before it
/// still resolves.
const PLAYBACK_SLOTS: usize = 2;
const SPECULATIVE_SLOTS: usize = 2;
/// Guesses waiting for a speculative slot; older ones fall off.
const BACKLOG: usize = 8;
/// A cached URL is used until this many seconds before it expires.
const MARGIN: u64 = 600;

#[derive(Clone, Serialize, Deserialize)]
struct Cached {
    itag: u32,
    url: String,
    user_agent: Option<String>,
    expires: u64,
    /// Resolved with the account's cookies, so with its formats.
    signed_in: bool,
}

impl Cached {
    fn stream(&self) -> Stream {
        Stream {
            itag: self.itag,
            url: self.url.clone(),
            user_agent: self.user_agent.clone(),
            expires: self.expires,
        }
    }
}

type Outcome = Option<Result<Stream, String>>;

/// One resolve, shared by everyone who wants its song.
struct Flight {
    result: watch::Sender<Outcome>,
    waiters: AtomicUsize,
    /// Started for playback: stopped when nobody waits for it any more.
    cancellable: AtomicBool,
    abort: Mutex<Option<tokio::task::AbortHandle>>,
}

pub struct Resolver {
    cache: Mutex<HashMap<String, Cached>>,
    /// The runtime directory: the saved cache.
    scratch: PathBuf,
    flights: Mutex<HashMap<String, Arc<Flight>>>,
    playback: Arc<Semaphore>,
    speculative: Arc<Semaphore>,
    backlog: Mutex<VecDeque<String>>,
    /// Serialises writes of the saved cache.
    saving: Mutex<()>,
    /// InnerTube and the JS challenges, set by [`Resolver::use_innertube`].
    native: OnceLock<crate::streams::Native>,
    /// Songs whose stream failed to play: resolved without the account
    /// next time.
    failed: Mutex<HashSet<String>>,
}

pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

pub fn describe(itag: u32) -> String {
    match itag {
        774 => "Opus 256 kbps · Premium (itag 774)".into(),
        141 => "AAC 256 kbps · Premium (itag 141)".into(),
        251 => "Opus 160 kbps (itag 251)".into(),
        140 => "AAC 128 kbps (itag 140)".into(),
        250 => "Opus 70 kbps (itag 250)".into(),
        249 => "Opus 50 kbps (itag 249)".into(),
        other => format!("itag {other}"),
    }
}

/// A resolve asked for: known at once, or a share of a run.
pub enum Request {
    Ready(Result<Stream>),
    Waiting(Waiting),
}

/// A share of a run; dropping the last share of a playback run stops it.
pub struct Waiting {
    resolver: Arc<Resolver>,
    id: String,
    flight: Arc<Flight>,
}

impl Request {
    pub async fn wait(self) -> Result<Stream> {
        let waiting = match self {
            Request::Ready(result) => return result,
            Request::Waiting(waiting) => waiting,
        };
        let mut rx = waiting.flight.result.subscribe();
        let outcome = rx
            .wait_for(Option::is_some)
            .await
            .map(|o| o.clone())
            .map_err(|_| anyhow!("the stream lookup stopped"))?;
        drop(waiting);
        outcome
            .unwrap_or_else(|| Err("the stream lookup stopped".into()))
            .map_err(|e| anyhow!(e))
    }
}

impl Drop for Waiting {
    fn drop(&mut self) {
        let abort = {
            let mut flights = self.resolver.flights.lock().expect("flights lock");
            let last = self.flight.waiters.fetch_sub(1, Ordering::SeqCst) == 1;
            if !(last
                && self.flight.cancellable.load(Ordering::SeqCst)
                && self.flight.result.borrow().is_none())
            {
                return;
            }
            if flights
                .get(&self.id)
                .is_some_and(|f| Arc::ptr_eq(f, &self.flight))
            {
                flights.remove(&self.id);
            }
            self.flight.abort.lock().expect("abort lock").take()
        };
        if let Some(abort) = abort {
            abort.abort();
            log::info!("stopped resolving {}: no longer wanted", self.id);
        }
    }
}

/// Removes a finished or stopped run from the table.
struct Landing {
    resolver: Arc<Resolver>,
    id: String,
    flight: Arc<Flight>,
}

impl Drop for Landing {
    fn drop(&mut self) {
        // A run that ends without an answer (stopped, or panicked) says so.
        if self.flight.result.borrow().is_none() {
            self.flight
                .result
                .send_replace(Some(Err("the stream lookup stopped".into())));
        }
        let mut flights = self.resolver.flights.lock().expect("flights lock");
        if flights
            .get(&self.id)
            .is_some_and(|f| Arc::ptr_eq(f, &self.flight))
        {
            flights.remove(&self.id);
        }
    }
}

impl Resolver {
    /// Starts with the streams saved by an earlier run that are still valid.
    pub fn new(scratch: PathBuf) -> Self {
        let deadline = now() + MARGIN;
        let saved: HashMap<String, Cached> = std::fs::read(scratch.join("streams.json"))
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default();
        let cache: HashMap<String, Cached> = saved
            .into_iter()
            .filter(|(_, c)| c.expires > deadline)
            .collect();
        if !cache.is_empty() {
            log::info!("{} saved streams still valid", cache.len());
        }
        Self {
            cache: Mutex::new(cache),
            scratch,
            flights: Mutex::default(),
            playback: Arc::new(Semaphore::new(PLAYBACK_SLOTS)),
            speculative: Arc::new(Semaphore::new(SPECULATIVE_SLOTS)),
            backlog: Mutex::default(),
            saving: Mutex::default(),
            native: OnceLock::new(),
            failed: Mutex::default(),
        }
    }

    /// Resolves through `client`'s InnerTube session (`crate::streams`).
    pub fn use_innertube(
        &self,
        client: Arc<crate::innertube::Client>,
        paths: &crate::paths::Paths,
    ) {
        let native = crate::streams::Native::new(client, &paths.cache, &paths.config);
        let _ = self.native.set(native);
    }

    /// Whether the InnerTube session is signed in. Streams resolved signed
    /// out don't count then (they lack the account's formats); the others
    /// stay usable whatever happens to the session, since stream URLs carry
    /// no cookies.
    fn signed_in(&self) -> bool {
        self.native.get().is_some_and(|n| n.signed_in())
    }

    /// A cached stream still valid for ten minutes.
    pub fn cached(&self, video_id: &str) -> Option<Stream> {
        if let Some(stream) = fake_stream() {
            return Some(stream);
        }
        let signed_in = self.signed_in();
        let cache = self.cache.lock().expect("cache lock");
        cache
            .get(video_id)
            .filter(|c| c.expires > now() + MARGIN && (c.signed_in || !signed_in))
            .map(Cached::stream)
    }

    /// Drops `video_id`'s stream after it failed to play; the next resolve
    /// goes without the account.
    pub fn forget(&self, video_id: &str) {
        {
            let mut failed = self.failed.lock().expect("failed lock");
            if failed.len() > 256 {
                failed.clear();
            }
            failed.insert(video_id.to_owned());
        }
        let gone = self.cache.lock().expect("cache lock").remove(video_id);
        if let (Some(gone), Some(native)) = (gone, self.native.get()) {
            native.stream_failed(&gone.url);
        }
        self.save();
    }

    /// The best stream the account can get, for playback.
    pub async fn resolve(self: &Arc<Self>, video_id: &str) -> Result<Stream> {
        self.request(video_id).wait().await
    }

    /// Asks for a stream for playback. The share is taken at once, so a run
    /// handed from one waiter to the next is never stopped in between.
    pub fn request(self: &Arc<Self>, video_id: &str) -> Request {
        let mut flights = self.flights.lock().expect("flights lock");
        if let Some(stream) = self.cached(video_id) {
            return Request::Ready(Ok(stream));
        }
        let flight = match flights.get(video_id) {
            Some(flight) => flight.clone(),
            None => self.launch(&mut flights, video_id, None),
        };
        flight.waiters.fetch_add(1, Ordering::SeqCst);
        Request::Waiting(Waiting {
            resolver: self.clone(),
            id: video_id.to_owned(),
            flight,
        })
    }

    /// Resolves likely songs ahead of a click, most likely first, without
    /// taking a playback slot.
    pub fn prepare_many(self: &Arc<Self>, mut video_ids: Vec<String>) {
        video_ids.retain(|id| self.cached(id).is_none());
        if video_ids.is_empty() {
            return;
        }
        {
            // A playback run that is also a good guess keeps going if playback moves on.
            let flights = self.flights.lock().expect("flights lock");
            for id in &video_ids {
                if let Some(flight) = flights.get(id) {
                    flight.cancellable.store(false, Ordering::SeqCst);
                }
            }
        }
        {
            let mut backlog = self.backlog.lock().expect("backlog lock");
            backlog.retain(|id| !video_ids.contains(id));
            for id in video_ids.into_iter().rev() {
                backlog.push_front(id);
            }
            backlog.truncate(BACKLOG);
        }
        self.pump();
    }

    pub fn prepare(self: &Arc<Self>, video_id: &str) {
        self.prepare_many(vec![video_id.to_owned()]);
    }

    /// Waits for a song through the speculative path: prepared ahead (most
    /// likely first), sharing a run already under way, never taking a
    /// playback slot. For previews (Audition), which must not delay playback.
    pub async fn prepared(self: &Arc<Self>, video_id: &str) -> Result<Stream> {
        let deadline = Instant::now() + std::time::Duration::from_secs(90);
        loop {
            if let Some(stream) = self.cached(video_id) {
                return Ok(stream);
            }
            let share = {
                let flights = self.flights.lock().expect("flights lock");
                flights.get(video_id).cloned().map(|flight| {
                    flight.waiters.fetch_add(1, Ordering::SeqCst);
                    Waiting {
                        resolver: self.clone(),
                        id: video_id.to_owned(),
                        flight,
                    }
                })
            };
            if let Some(share) = share {
                return Request::Waiting(share).wait().await;
            }
            if Instant::now() > deadline {
                bail!("the stream lookup took too long");
            }
            // Still waiting for a speculative slot: stay first in line.
            self.prepare(video_id);
            if !self
                .flights
                .lock()
                .expect("flights lock")
                .contains_key(video_id)
            {
                tokio::time::sleep(std::time::Duration::from_millis(200)).await;
            }
        }
    }

    /// Starts guesses from the backlog while speculative slots are free.
    fn pump(self: &Arc<Self>) {
        loop {
            let Ok(permit) = self.speculative.clone().try_acquire_owned() else {
                return;
            };
            let mut flights = self.flights.lock().expect("flights lock");
            let next = loop {
                let Some(id) = self.backlog.lock().expect("backlog lock").pop_front() else {
                    break None;
                };
                if !flights.contains_key(&id) && self.cached(&id).is_none() {
                    break Some(id);
                }
            };
            let Some(id) = next else { return };
            self.launch(&mut flights, &id, Some(permit));
        }
    }

    /// Starts a run: speculative with a slot in hand, otherwise for playback
    /// once a playback slot is free.
    fn launch(
        self: &Arc<Self>,
        flights: &mut HashMap<String, Arc<Flight>>,
        video_id: &str,
        permit: Option<OwnedSemaphorePermit>,
    ) -> Arc<Flight> {
        let speculative = permit.is_some();
        let flight = Arc::new(Flight {
            result: watch::Sender::new(None),
            waiters: AtomicUsize::new(0),
            cancellable: AtomicBool::new(!speculative),
            abort: Mutex::default(),
        });
        flights.insert(video_id.to_owned(), flight.clone());
        let this = self.clone();
        let id = video_id.to_owned();
        let shared = flight.clone();
        let task = tokio::spawn(async move {
            let _landing = Landing {
                resolver: this.clone(),
                id: id.clone(),
                flight: shared.clone(),
            };
            let queued = Instant::now();
            let permit = match permit {
                Some(permit) => permit,
                None => match this.playback.clone().acquire_owned().await {
                    Ok(permit) => permit,
                    Err(_) => return,
                },
            };
            let waited = queued.elapsed();
            let started = Instant::now();
            let result = this.run(&id).await;
            let kind = if speculative { "ahead" } else { "for playback" };
            match &result {
                Ok((stream, _)) => log::info!(
                    "resolved {id} {kind}: itag {} in {:.1}s (waited {:.1}s for a slot)",
                    stream.itag,
                    started.elapsed().as_secs_f64(),
                    waited.as_secs_f64()
                ),
                Err(error) => log::warn!(
                    "resolving {id} {kind} failed after {:.1}s: {error:#}",
                    started.elapsed().as_secs_f64()
                ),
            }
            let outcome = match result {
                Ok((stream, signed_in)) => {
                    this.store(&id, &stream, signed_in);
                    Ok(stream)
                }
                Err(error) => Err(format!("{error:#}")),
            };
            shared.result.send_replace(Some(outcome));
            drop(permit);
            if speculative {
                this.pump();
            }
        });
        *flight.abort.lock().expect("abort lock") = Some(task.abort_handle());
        flight
    }

    fn store(&self, video_id: &str, stream: &Stream, signed_in: bool) {
        self.cache.lock().expect("cache lock").insert(
            video_id.to_owned(),
            Cached {
                itag: stream.itag,
                url: stream.url.clone(),
                user_agent: stream.user_agent.clone(),
                expires: stream.expires,
                signed_in,
            },
        );
        self.save();
    }

    /// Writes the valid streams to the runtime directory, readable only by
    /// the user (the URLs are tied to the account).
    fn save(&self) {
        let _turn = self.saving.lock().expect("saving lock");
        let bytes = {
            let cache = self.cache.lock().expect("cache lock");
            let deadline = now() + MARGIN;
            let valid: HashMap<&String, &Cached> =
                cache.iter().filter(|(_, c)| c.expires > deadline).collect();
            match serde_json::to_vec(&valid) {
                Ok(bytes) => bytes,
                Err(_) => return,
            }
        };
        let path = self.scratch.join("streams.json");
        let temporary = self
            .scratch
            .join(format!("streams.json.tmp{}", std::process::id()));
        let written = crate::paths::private_file()
            .open(&temporary)
            .and_then(|mut file| file.write_all(&bytes))
            .and_then(|()| std::fs::rename(&temporary, &path));
        if let Err(error) = written {
            log::warn!("couldn't save resolved streams: {error}");
        }
    }

    /// One resolve; also says whether it has the account's formats.
    async fn run(&self, video_id: &str) -> Result<(Stream, bool)> {
        let native = self
            .native
            .get()
            .context("the stream resolver isn't ready")?;
        let failed = self.failed.lock().expect("failed lock").contains(video_id);
        let signed_in = self.signed_in();
        let started = Instant::now();
        let (stream, client) = native.resolve(video_id, signed_in && !failed).await?;
        log::info!(
            "resolved {video_id} as {client}: itag {} in {:.2}s",
            stream.itag,
            started.elapsed().as_secs_f64()
        );
        // Signed out after a failure counts as the account's best: it won't
        // get better until the URL expires. Signed out while the account's
        // client fails for the player (being prepared, or the solver can't
        // use it) is asked again next time.
        let account = signed_in && (failed || client != crate::streams::VISIONOS.name);
        Ok((stream, account))
    }
}
