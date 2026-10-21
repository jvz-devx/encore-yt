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
use crate::model::{PlaylistCounts, PlaylistLoad, Target, Track};
use crate::parse::playlist::{Part, first, more};
use crate::paths;

use super::{Event, Internal};

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
    #[serde(default)]
    counts: Option<PlaylistCounts>,
}

#[derive(Debug)]
pub(super) struct Loaded {
    pub tracks: Vec<Track>,
    pub warning: Option<String>,
    pub counts: Option<PlaylistCounts>,
    pub cached: bool,
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
    optimistic_entry: Option<u64>,
    advance_when_ready: bool,
}

impl Failure {
    #[cfg(test)]
    pub fn message(&self) -> String {
        self.message_for(false)
    }

    fn message_for(&self, optimistic: bool) -> String {
        let reason = match self.error {
            ApiError::Auth => "YouTube Music didn't accept the sign-in. Reconnect and try again.",
            ApiError::Http(429) => "YouTube Music is limiting requests. Try again later.",
            ApiError::Offline(_) => {
                "Couldn't reach YouTube Music after retrying. Check your connection and try again."
            }
            ApiError::Http(_) => "YouTube Music couldn't load the playlist. Try again later.",
            ApiError::Invalid(_) => "The playlist couldn't be loaded completely. Try again.",
        };
        let kept = if optimistic {
            "The selected song is kept; the rest wasn't added."
        } else {
            "Your queue is unchanged."
        };
        format!(
            "{reason} Loaded {} songs. {kept}: {}",
            self.loaded, self.error
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

impl super::Worker {
    pub(super) fn provisional_playlist(&self) -> bool {
        self.playlist_request
            .as_ref()
            .is_some_and(|pending| pending.optimistic_entry.is_some())
    }

    pub(super) fn keep_playlist_current_song(&mut self) {
        if let Some(pending) = &mut self.playlist_request {
            pending.advance_when_ready = false;
        }
    }

    pub(super) fn cancel_optimistic_playlist(&mut self) {
        if self.provisional_playlist() {
            self.cancel_playlist();
        }
    }

    pub(super) fn wait_for_playlist(&mut self, pos: usize) -> bool {
        if pos + 1 != self.queue.len() {
            return false;
        }
        let Some(pending) = &mut self.playlist_request else {
            return false;
        };
        if pending.optimistic_entry.is_none() {
            return false;
        }
        pending.advance_when_ready = true;
        true
    }

    pub(super) async fn play_playlist_song(&mut self, target: Target, track: Track) {
        if !matches!(&target, Target::Browse { id, .. } if id.starts_with("VL")) {
            return;
        }
        let reuse = self
            .playlist_request
            .as_ref()
            .is_some_and(|pending| pending.target == target);
        if !reuse {
            self.new_epoch();
        }
        self.decks.radio = false;
        self.decks.autoplay.clear();
        let video = track.video_id.clone();
        self.set_queue(vec![track], 0);
        self.start(0).await;
        if reuse {
            if let Some(pending) = &mut self.playlist_request {
                pending.start = Some(video);
                pending.shuffle = false;
            }
        } else {
            self.play_playlist(target, Some(video), false);
        }
        if let Some(pending) = &mut self.playlist_request {
            pending.optimistic_entry = self.queue.id(0);
            pending.advance_when_ready = false;
        }
    }

    pub(super) fn playlist_progress(&mut self, epoch: u64, account: &str, progress: PlaylistLoad) {
        if epoch != self.epoch || self.playlist_request.is_none() || account != scope(&self.client)
        {
            return;
        }
        self.state.playlist_load = Some(progress);
        self.emit(true);
    }

    pub(super) async fn playlist_result(
        &mut self,
        epoch: u64,
        account: String,
        result: Result<Loaded, Failure>,
    ) {
        if epoch != self.epoch {
            return;
        }
        if account != scope(&self.client) {
            self.cancel_playlist();
            self.sink.send(Event::Error(
                "The sign-in changed while loading the playlist. Try again.".into(),
            ));
            return;
        }
        let Some(pending) = self.playlist_request.take() else {
            return;
        };
        self.playlist_task = None;
        self.state.playlist_load = None;
        let optimistic = pending.optimistic_entry.is_some();
        let loaded = match result {
            Ok(loaded) if !loaded.tracks.is_empty() => loaded,
            Ok(_) => {
                self.emit(true);
                self.sink.send(Event::Error(
                    if optimistic {
                        "There are no other songs to add. The selected song is kept."
                    } else {
                        "There are no songs to play here. Your queue is unchanged."
                    }
                    .into(),
                ));
                return;
            }
            Err(failure) => {
                self.emit(true);
                self.sink
                    .send(Event::Error(failure.message_for(optimistic)));
                return;
            }
        };
        let selected_entry = pending
            .optimistic_entry
            .and_then(|id| self.queue.position(id))
            .and_then(|pos| self.queue.track(pos));
        let selected = selected_entry
            .and_then(|track| track.set_video_id.as_ref())
            .and_then(|entry| {
                loaded
                    .tracks
                    .iter()
                    .position(|track| track.set_video_id.as_ref() == Some(entry))
            })
            .or_else(|| {
                pending
                    .start
                    .as_ref()
                    .and_then(|id| loaded.tracks.iter().position(|track| &track.video_id == id))
            });
        if pending.start.is_some() && selected.is_none() {
            self.emit(true);
            self.sink.send(Event::Error("That song isn't in the saved playlist. Refresh the playlist and try again. The current queue is kept.".into()));
            return;
        }
        let progress = PlaylistLoad {
            key: pending.target.key(),
            entries: loaded.tracks.len(),
            unique_tracks: loaded
                .tracks
                .iter()
                .map(|track| &track.video_id)
                .collect::<HashSet<_>>()
                .len(),
            counts: loaded.counts,
            complete: true,
            cached: loaded.cached,
        };
        if let Some(warning) = loaded.warning {
            self.sink.send(Event::Error(warning));
        }
        if let Some(id) = pending.optimistic_entry {
            let at = self.queue.complete_playlist(
                id,
                loaded.tracks,
                selected.unwrap_or(0),
                self.state.shuffle,
            );
            if let Some(at) = at {
                self.pos = Some(at);
                self.state.index = self.pos;
                self.state.playlist_load = Some(progress);
                self.send_queue();
                self.prefetch();
                self.emit(true);
                if pending.advance_when_ready {
                    self.next(false).await;
                }
            } else {
                self.emit(true);
                self.sink.send(Event::Error(
                    "The queue changed while the playlist loaded, so it was kept.".into(),
                ));
            }
        } else {
            if pending.shuffle {
                self.state.shuffle = true;
            }
            let at = selected.unwrap_or_else(|| {
                if pending.shuffle {
                    fastrand::usize(..loaded.tracks.len())
                } else {
                    0
                }
            });
            self.new_epoch();
            self.state.playlist_load = Some(progress);
            self.set_queue(loaded.tracks, at);
            if let Some(pos) = self.pos {
                self.start(pos).await;
            }
        }
    }

    pub(super) fn cancel_playlist(&mut self) {
        self.playlist_request = None;
        self.state.playlist_load = None;
        if let Some(task) = self.playlist_task.take() {
            task.abort();
            self.epoch += 1;
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
            pending.optimistic_entry = None;
            pending.advance_when_ready = false;
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
            optimistic_entry: None,
            advance_when_ready: false,
        });
        self.state.playlist_load = Some(PlaylistLoad {
            key: target.key(),
            ..Default::default()
        });
        self.emit(true);
        let client = self.client.clone();
        let library = self.playlists.clone();
        let directory = self.paths.cache.join("playlists");
        let tx = self.internal_tx.clone();
        let account = scope(&client);
        self.playlist_task = Some(tokio::spawn(async move {
            let result = library
                .load_with_progress(
                    &directory,
                    &account,
                    &target,
                    |token| {
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
                                first(&client.browse(&id, params.as_deref()).await?)
                            };
                            if scope(&client) != account {
                                return Err(ApiError::Auth);
                            }
                            part
                        }
                    },
                    |progress| {
                        let _ = tx.send(Internal::PlaylistProgress {
                            epoch,
                            scope: account.clone(),
                            progress,
                        });
                    },
                )
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
    #[cfg(test)]
    pub async fn load<F, Fut>(
        &self,
        directory: &Path,
        scope: &str,
        target: &Target,
        fetch: F,
    ) -> Result<Loaded, Failure>
    where
        F: FnMut(Option<String>) -> Fut,
        Fut: Future<Output = Result<Part, ApiError>>,
    {
        self.load_with_progress(directory, scope, target, fetch, |_| {})
            .await
    }

    pub async fn load_with_progress<F, Fut>(
        &self,
        directory: &Path,
        scope: &str,
        target: &Target,
        mut fetch: F,
        mut progress: impl FnMut(PlaylistLoad),
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
                counts: snapshot.counts.clone(),
                cached: true,
            });
        }
        let result = if cooldown.is_some_and(|until| until > Instant::now()) {
            Err(Failure {
                loaded: 0,
                error: ApiError::Http(429),
            })
        } else {
            collect_with_progress(&mut fetch, PAGE_PAUSE, &target.key(), &mut progress).await
        };
        let disk = self.disk.clone().lock_owned().await;
        if self.revision.load(Ordering::Acquire) != revision {
            return Err(Failure {
                loaded: result
                    .as_ref()
                    .map_or_else(|failure| failure.loaded, |(tracks, _)| tracks.len()),
                error: ApiError::Invalid("playlist changed during loading".into()),
            });
        }
        match result {
            Ok((tracks, counts)) => {
                let snapshot = Snapshot {
                    version: 2,
                    saved: now(),
                    tracks,
                    counts: Some(counts),
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
                    counts: snapshot.counts,
                    cached: false,
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
                        counts: snapshot.counts,
                        cached: true,
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

#[cfg(test)]
async fn collect<F, Fut>(fetch: &mut F, pause: Duration) -> Result<Vec<Track>, Failure>
where
    F: FnMut(Option<String>) -> Fut,
    Fut: Future<Output = Result<Part, ApiError>>,
{
    collect_with_progress(fetch, pause, "synthetic", &mut |_| {})
        .await
        .map(|(tracks, _)| tracks)
}

async fn collect_with_progress<F, Fut>(
    fetch: &mut F,
    pause: Duration,
    key: &str,
    progress: &mut impl FnMut(PlaylistLoad),
) -> Result<(Vec<Track>, PlaylistCounts), Failure>
where
    F: FnMut(Option<String>) -> Fut,
    Fut: Future<Output = Result<Part, ApiError>>,
{
    let mut tracks = Vec::new();
    let mut token = None;
    let mut tokens = HashSet::new();
    let mut entries = HashSet::new();
    let mut videos = HashSet::new();
    let mut counts = PlaylistCounts::default();
    loop {
        let part = request(fetch, token.clone(), pause)
            .await
            .map_err(|error| Failure {
                loaded: tracks.len(),
                error,
            })?;
        counts.rows += part.counts.rows;
        counts.unavailable += part.counts.unavailable;
        counts.unparsed += part.counts.unparsed;
        counts.expected = counts.expected.or(part.counts.expected);
        for track in part.tracks {
            if let Some(id) = &track.set_video_id
                && !entries.insert(id.clone())
            {
                counts.overlapping_entries += 1;
                continue;
            }
            videos.insert(track.video_id.clone());
            tracks.push(track);
        }
        progress(PlaylistLoad {
            key: key.into(),
            entries: tracks.len(),
            unique_tracks: videos.len(),
            counts: Some(counts.clone()),
            ..Default::default()
        });
        if counts.unparsed > 0 {
            return Err(Failure {
                loaded: tracks.len(),
                error: ApiError::Invalid("some playlist entries could not be read".into()),
            });
        }
        log::debug!("playlist scan: {} songs loaded", tracks.len());
        let Some(next) = part.next else {
            return Ok((tracks, counts));
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
    ((1..=2).contains(&snapshot.version) && snapshot.saved <= now()).then_some(snapshot)
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
