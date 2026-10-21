//! Complete playlist snapshots. Pagination, pacing, retries and disk reuse
//! live here; the worker only commits a finished list to the queue.

use std::collections::HashSet;
use std::future::Future;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

use crate::innertube::{ApiError, Client};
use crate::model::{Page, Target, Track};
use crate::{parse, paths};

use super::Internal;

const FRESH_FOR: u64 = 24 * 60 * 60;
const PAGE_PAUSE: Duration = Duration::from_millis(500);
const ATTEMPTS: usize = 3;
const MAX_CACHE_BYTES: u64 = 64 * 1024 * 1024;

#[derive(Default)]
pub(super) struct Library {
    // One scan at a time. Account writes must not wait for network scans.
    gate: Arc<Mutex<Option<Instant>>>,
    disk: Arc<Mutex<()>>,
    revision: AtomicU64,
}

#[derive(Serialize, Deserialize)]
struct Snapshot {
    version: u32,
    saved: u64,
    tracks: Vec<Track>,
}

#[derive(Debug)]
pub(super) struct Loaded {
    pub tracks: Vec<Track>,
    pub warning: Option<String>,
}

#[derive(Debug)]
pub(super) struct Part {
    tracks: Vec<Track>,
    next: Option<String>,
}

#[derive(Debug)]
pub(super) struct Failure {
    pub loaded: usize,
    pub error: ApiError,
}

pub(super) struct Pending {
    target: Target,
    start: Option<String>,
    shuffle: bool,
}

impl Pending {
    pub fn selection(self) -> (Option<String>, bool) {
        (self.start, self.shuffle)
    }
}

impl Failure {
    pub fn message(&self) -> String {
        let reason = match self.error {
            ApiError::Auth => "YouTube Music didn't accept the sign-in. Reconnect and try again.",
            ApiError::Http(429) => "YouTube Music is limiting requests. Try again later.",
            ApiError::Offline(_) => {
                "Couldn't reach YouTube Music after retrying. Check your connection and try again."
            }
            ApiError::Http(_) => "YouTube Music couldn't load the playlist. Try again later.",
            ApiError::Invalid(_) => "The playlist couldn't be loaded completely. Try again.",
        };
        format!(
            "{reason} Loaded {} songs. Your queue is unchanged.",
            self.loaded
        )
    }
}

/// Only a hash goes into the cache path. Cookie values never go on disk.
/// Ignore visitor/preferences/timestamp cookies which change during normal
/// requests; use login credentials so a different account cannot reuse a list.
pub(super) fn scope(client: &Client) -> String {
    identity_scope(|name| client.cookie(name), client.page_id())
}

fn identity_scope(cookie: impl Fn(&str) -> Option<String>, channel: Option<String>) -> String {
    let mut identity = String::new();
    for name in [
        "SAPISID",
        "__Secure-1PAPISID",
        "__Secure-3PAPISID",
        "SID",
        "__Secure-1PSID",
        "__Secure-3PSID",
    ] {
        identity.push_str(name);
        identity.push('\0');
        identity.push_str(&cookie(name).unwrap_or_default());
        identity.push('\0');
    }
    identity.push_str(&channel.unwrap_or_default());
    paths::hash(&identity)
}

pub(super) fn first(page: Page) -> Result<Part, ApiError> {
    let shelf = crate::account::entries(&page)
        .ok_or_else(|| ApiError::Invalid("playlist song list missing".into()))?;
    Ok(Part {
        tracks: shelf.items.iter().filter_map(|i| i.track.clone()).collect(),
        next: shelf.continuation.clone().or(page.continuation),
    })
}

pub(super) fn more(value: &serde_json::Value) -> Result<Part, ApiError> {
    let known = ["musicShelfContinuation", "musicPlaylistShelfContinuation"]
        .iter()
        .any(|key| {
            value
                .get("continuationContents")
                .and_then(|cc| cc.get(key))
                .is_some()
        })
        || parse::find(value, "continuationItems").is_some();
    if !known {
        return Err(ApiError::Invalid("playlist continuation missing".into()));
    }
    match parse::more(value) {
        parse::More::Items { items, next } => {
            let tracks: Vec<_> = items.into_iter().filter_map(|i| i.track).collect();
            Ok(Part { tracks, next })
        }
        parse::More::Shelves { .. } => {
            Err(ApiError::Invalid("unexpected playlist continuation".into()))
        }
    }
}

impl super::Worker {
    pub(super) fn cancel_playlist(&mut self) {
        self.playlist_request = None;
        if let Some(task) = self.playlist_task.take() {
            task.abort();
            self.epoch += 1;
            self.state.loading = false;
            self.emit(true);
        }
    }

    pub(super) fn play_playlist(&mut self, target: Target, start: Option<String>, shuffle: bool) {
        let Target::Browse { id, .. } = &target else {
            return;
        };
        if !id.starts_with("VL") {
            return;
        }
        // Repeated clicks update the requested starting song, not restart a
        // costly scan and fetch the same pages again.
        if let Some(pending) = &mut self.playlist_request
            && pending.target == target
        {
            pending.start = start;
            pending.shuffle = shuffle;
            return;
        }
        self.cancel_playlist();
        self.epoch += 1;
        self.extending = false;
        self.advance_pending = false;
        let epoch = self.epoch;
        self.playlist_request = Some(Pending {
            target: target.clone(),
            start,
            shuffle,
        });
        self.state.loading = true;
        self.emit(true);
        let client = self.client.clone();
        let library = self.playlists.clone();
        let directory = self.paths.cache.join("playlists");
        let tx = self.internal_tx.clone();
        let account = scope(&client);
        self.playlist_task = Some(tokio::spawn(async move {
            let result = library
                .load(&directory, &account, &target, |token| {
                    let client = client.clone();
                    let target = target.clone();
                    let account = account.clone();
                    async move {
                        if scope(&client) != account {
                            return Err(ApiError::Auth);
                        }
                        let part = if let Some(token) = token {
                            more(&client.continuation(&token).await?)
                        } else {
                            let Target::Browse { id, params } = target else {
                                unreachable!()
                            };
                            first(parse::page(&client.browse(&id, params.as_deref()).await?))
                        };
                        if scope(&client) != account {
                            return Err(ApiError::Auth);
                        }
                        part
                    }
                })
                .await;
            let _ = tx.send(Internal::Playlist {
                epoch,
                scope: account,
                result,
            });
        }));
    }
}

impl Library {
    /// Only whole lists are cached or returned. An old whole list is usable
    /// on a transient failure, with an explicit warning, never after sign-out.
    pub async fn load<F, Fut>(
        &self,
        directory: &Path,
        scope: &str,
        target: &Target,
        mut fetch: F,
    ) -> Result<Loaded, Failure>
    where
        F: FnMut(Option<String>) -> Fut,
        Fut: Future<Output = Result<Part, ApiError>>,
    {
        let mut cooldown = self.gate.clone().lock_owned().await;
        let file = directory
            .join(scope)
            .join(format!("{}.json", paths::hash(&target.key())));
        let disk = self.disk.lock().await;
        let revision = self.revision.load(Ordering::Acquire);
        let cached = read(&file).await;
        drop(disk);
        if let Some(snapshot) = &cached
            && now().saturating_sub(snapshot.saved) < FRESH_FOR
        {
            return Ok(Loaded {
                tracks: snapshot.tracks.clone(),
                warning: None,
            });
        }
        let result = if cooldown.is_some_and(|until| until > Instant::now()) {
            Err(Failure {
                loaded: 0,
                error: ApiError::Http(429),
            })
        } else {
            collect(&mut fetch, PAGE_PAUSE).await
        };
        let disk = self.disk.clone().lock_owned().await;
        if self.revision.load(Ordering::Acquire) != revision {
            return Err(Failure {
                loaded: result
                    .as_ref()
                    .map_or_else(|failure| failure.loaded, Vec::len),
                error: ApiError::Invalid("playlist changed during loading".into()),
            });
        }
        match result {
            Ok(tracks) => {
                let snapshot = Snapshot {
                    version: 1,
                    saved: now(),
                    tracks,
                };
                let loaded = snapshot.tracks.len();
                // A cancelled async caller cannot release the scan lock while
                // its blocking atomic write is still running. Invalidation
                // must happen after that write, not race a late rename.
                let (snapshot, saved) = tokio::task::spawn_blocking(move || {
                    let _guard = cooldown;
                    let _disk = disk;
                    let saved = save_snapshot(&file, &snapshot);
                    (snapshot, saved)
                })
                .await
                .map_err(|_| Failure {
                    loaded,
                    error: ApiError::Invalid("playlist cache worker stopped".into()),
                })?;
                let warning = saved.err().map(|error| {
                    log::warn!("saving complete playlist cache: {error}");
                    "All songs loaded, but the playlist couldn't be saved. Next play will need to load it again.".into()
                });
                Ok(Loaded {
                    tracks: snapshot.tracks,
                    warning,
                })
            }
            Err(failure) => {
                if matches!(failure.error, ApiError::Http(429)) {
                    *cooldown = Some(Instant::now() + Duration::from_secs(60));
                }
                if let Some(snapshot) = cached
                    && !matches!(failure.error, ApiError::Auth)
                {
                    let count = snapshot.tracks.len();
                    return Ok(Loaded {
                        tracks: snapshot.tracks,
                        warning: Some(format!(
                            "Couldn't refresh the complete playlist. Using the saved copy of {count} songs; recent changes may be missing. Try again later."
                        )),
                    });
                }
                Err(failure)
            }
        }
    }

    /// Account edits invalidate in-flight scans without waiting for their
    /// network requests. The disk lock orders deletion after any atomic write.
    pub async fn invalidate(&self, directory: &Path, scope: &str) -> std::io::Result<()> {
        let _guard = self.disk.lock().await;
        self.revision.fetch_add(1, Ordering::AcqRel);
        let mut entries = match tokio::fs::read_dir(directory.join(scope)).await {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(error),
        };
        while let Some(entry) = entries.next_entry().await? {
            if entry.file_type().await?.is_file()
                && entry
                    .path()
                    .extension()
                    .is_some_and(|extension| extension == "json")
            {
                tokio::fs::remove_file(entry.path()).await?;
            }
        }
        Ok(())
    }
}

async fn collect<F, Fut>(fetch: &mut F, pause: Duration) -> Result<Vec<Track>, Failure>
where
    F: FnMut(Option<String>) -> Fut,
    Fut: Future<Output = Result<Part, ApiError>>,
{
    let mut tracks = Vec::new();
    let mut token = None;
    let mut tokens = HashSet::new();
    loop {
        let part = request(fetch, token.clone(), pause)
            .await
            .map_err(|error| Failure {
                loaded: tracks.len(),
                error,
            })?;
        tracks.extend(part.tracks);
        log::debug!("playlist scan: {} songs loaded", tracks.len());
        let Some(next) = part.next else {
            return Ok(tracks);
        };
        if !tokens.insert(next.clone()) {
            return Err(Failure {
                loaded: tracks.len(),
                error: ApiError::Invalid("repeated playlist continuation".into()),
            });
        }
        token = Some(next);
        tokio::time::sleep(pause).await;
    }
}

async fn request<F, Fut>(
    fetch: &mut F,
    token: Option<String>,
    pause: Duration,
) -> Result<Part, ApiError>
where
    F: FnMut(Option<String>) -> Fut,
    Fut: Future<Output = Result<Part, ApiError>>,
{
    for attempt in 0..ATTEMPTS {
        match fetch(token.clone()).await {
            Ok(part) => return Ok(part),
            Err(error) => {
                log::warn!(
                    "playlist page attempt {} of {ATTEMPTS} failed: {error}",
                    attempt + 1
                );
                let Some(delay) = retry_delay(&error, attempt) else {
                    return Err(error);
                };
                // Tests use zero pacing so they can exercise all attempts offline.
                tokio::time::sleep(if pause.is_zero() {
                    Duration::ZERO
                } else {
                    delay
                })
                .await;
            }
        }
    }
    unreachable!("the last attempt always returns")
}

fn retry_delay(error: &ApiError, attempt: usize) -> Option<Duration> {
    if attempt + 1 >= ATTEMPTS {
        return None;
    }
    let base = match error {
        ApiError::Offline(_) | ApiError::Http(500..=599) => 1,
        ApiError::Http(429) => 5,
        _ => return None,
    };
    Some(Duration::from_secs(base * (1 << attempt)))
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

async fn read(file: &Path) -> Option<Snapshot> {
    let metadata = tokio::fs::metadata(file).await.ok()?;
    if metadata.len() > MAX_CACHE_BYTES {
        return None;
    }
    let bytes = tokio::fs::read(file).await.ok()?;
    let snapshot: Snapshot = serde_json::from_slice(&bytes).ok()?;
    (snapshot.version == 1 && snapshot.saved <= now()).then_some(snapshot)
}

fn save_snapshot(file: &Path, snapshot: &Snapshot) -> anyhow::Result<()> {
    let parent = file
        .parent()
        .ok_or_else(|| anyhow::anyhow!("cache parent missing"))?;
    paths::private_dir(parent)?;
    let bytes = serde_json::to_vec(snapshot)?;
    anyhow::ensure!(
        bytes.len() as u64 <= MAX_CACHE_BYTES,
        "playlist cache exceeds safe size"
    );
    paths::write_atomic(file, &bytes)?;
    Ok(())
}

#[cfg(test)]
async fn save(file: &Path, snapshot: &Snapshot) -> anyhow::Result<()> {
    save_snapshot(file, snapshot)
}

#[cfg(test)]
mod tests;
