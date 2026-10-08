//! The session across launches, in `~/.cache/encore-yt/session.json`: the
//! queue (play order, list order and additions, and whether it is a radio
//! for Smooth mixes), the current song and its
//! position, volume, shuffle, repeat and autoplay. It is saved on pause,
//! track changes, queue changes and exit, and every ten seconds while
//! playing. At launch it comes back paused, and the current song resolves
//! ahead so Play starts at once from the same position.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use super::*;

#[derive(Serialize, Deserialize)]
struct Saved {
    queue: queue::Snapshot,
    /// The current song's position in the play order.
    index: Option<usize>,
    /// Seconds into it.
    position: f64,
    volume: f64,
    shuffle: bool,
    repeat: Repeat,
    autoplay: bool,
    /// The queue is a radio or a mix (Smooth mixes blend its changes).
    #[serde(default)]
    radio: bool,
    /// Play-order positions of the songs autoplay added.
    #[serde(default)]
    autoplayed: Vec<usize>,
}

/// How often the session is saved while only the position changes.
const EVERY: Duration = Duration::from_secs(10);

/// A single writer with one replaceable pending snapshot. Slow storage cannot
/// block playback or accumulate a queue of obsolete session files.
pub(super) struct Writer {
    snapshots: tokio::sync::watch::Sender<Option<Arc<Saved>>>,
    task: tokio::task::JoinHandle<()>,
}

impl Writer {
    fn new(path: PathBuf) -> Self {
        let (snapshots, mut pending) = tokio::sync::watch::channel(None::<Arc<Saved>>);
        let task = tokio::spawn(async move {
            while pending.changed().await.is_ok() {
                let Some(saved) = pending.borrow_and_update().clone() else {
                    continue;
                };
                let path = path.clone();
                let result = tokio::task::spawn_blocking(move || {
                    let bytes = serde_json::to_vec(&*saved).map_err(std::io::Error::other)?;
                    crate::paths::write_atomic(&path, &bytes)
                })
                .await;
                match result {
                    Ok(Ok(())) => {}
                    Ok(Err(error)) => log::warn!("couldn't save the session: {error}"),
                    Err(error) => log::warn!("session writer stopped: {error}"),
                }
            }
        });
        Self { snapshots, task }
    }

    fn save(&self, saved: Saved) {
        self.snapshots.send_replace(Some(Arc::new(saved)));
    }

    async fn finish(self) {
        drop(self.snapshots);
        if let Err(error) = self.task.await {
            log::warn!("session writer stopped before shutdown: {error}");
        }
    }
}

impl super::Worker {
    fn session_file(&self) -> PathBuf {
        self.paths.cache.join("session.json")
    }

    /// Saves the session now (`now`), or if the last save is ten seconds
    /// old; exit saves whatever is newer.
    pub(super) fn save_session(&mut self, now: bool) {
        if !now && self.last_save.elapsed() < EVERY {
            return;
        }
        self.last_save = Instant::now();
        let position = match (self.current_entry, self.resume_at) {
            (None, Some(at)) => at,
            _ => self.state.position,
        };
        let saved = Saved {
            queue: self.queue.snapshot(),
            index: self.pos,
            position,
            volume: self.state.volume,
            shuffle: self.state.shuffle,
            repeat: self.state.repeat,
            autoplay: self.state.autoplay,
            radio: self.decks.radio,
            autoplayed: (0..self.queue.len())
                .filter(|&p| {
                    self.queue
                        .id(p)
                        .is_some_and(|id| self.decks.autoplay.contains(&id))
                })
                .collect(),
        };
        let path = self.session_file();
        self.session_writer
            .get_or_insert_with(|| Writer::new(path))
            .save(saved);
    }

    pub(super) async fn flush_session(&mut self) {
        if let Some(writer) = self.session_writer.take() {
            writer.finish().await;
        }
    }

    /// Brings back the last session, paused: the player bar shows the song
    /// and position at once.
    pub(super) async fn restore_session(&mut self) {
        let bytes = match tokio::fs::read(self.session_file()).await {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return,
            Err(error) => {
                log::warn!("couldn't read the saved session: {error}");
                return;
            }
        };
        let saved: Saved = match serde_json::from_slice(&bytes) {
            Ok(saved) => saved,
            Err(error) => {
                log::warn!("ignoring a damaged session file: {error}");
                return;
            }
        };
        self.state.volume = saved.volume.clamp(0.0, 100.0);
        self.state.shuffle = saved.shuffle;
        self.state.repeat = saved.repeat;
        self.state.autoplay = saved.autoplay;
        if let Some(queue) = queue::Queue::restore(saved.queue)
            && !queue.is_empty()
        {
            self.queue = queue;
            self.decks.radio = saved.radio;
            self.decks.autoplay = saved
                .autoplayed
                .iter()
                .filter_map(|&p| self.queue.id(p))
                .collect();
            self.pos = saved.index.filter(|&i| i < self.queue.len());
            self.state.index = self.pos;
            if let Some(track) = self.current().cloned() {
                let position = saved.position.max(0.0);
                self.state.position = position;
                self.state.duration = track.duration.map(f64::from).unwrap_or(0.0);
                self.resume_at = Some(position);
                self.fetch_watch_info(&track.video_id);
            }
            self.sink.send(Event::Queue(self.queue.tracks()));
            log::info!(
                "restored the session: {} songs, song {:?} at {:.0}s",
                self.queue.len(),
                self.pos,
                self.state.position
            );
        }
        self.last_save = Instant::now();
        self.emit(true);
    }

    /// Once the session is known (at launch): resolve the restored song for
    /// playback and fetch its loudness, so Play starts at once.
    pub(super) fn prepare_restored(&mut self) {
        if self.current_entry.is_some() || self.state.loading || self.resolving.is_some() {
            return;
        }
        let Some(video_id) = self.current().map(|t| t.video_id.clone()) else {
            return;
        };
        let request = self.resolver.request(&video_id);
        let task = tokio::spawn(async move {
            let _ = request.wait().await;
        });
        self.resolving = Some(task.abort_handle());
        self.fetch_player(&video_id);
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    reason = "tests write only synthetic session state"
)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn shutdown_flushes_the_latest_coalesced_snapshot() {
        let path =
            std::env::temp_dir().join(format!("encore-session-writer-{}", std::process::id()));
        let writer = Writer::new(path.clone());
        for position in 0..100 {
            writer.save(Saved {
                queue: queue::Queue::default().snapshot(),
                index: None,
                position: f64::from(position),
                volume: 50.0,
                shuffle: false,
                repeat: Repeat::Off,
                autoplay: false,
                radio: false,
                autoplayed: Vec::new(),
            });
        }
        writer.finish().await;
        let bytes = tokio::fs::read(&path).await.unwrap();
        let saved: Saved = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(saved.position, 99.0);
        tokio::fs::remove_file(path).await.unwrap();
    }
}
