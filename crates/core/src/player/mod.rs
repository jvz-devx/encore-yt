//! What the backend asks of the audio engine (`encore-audio`).
//!
//! A [`Player`] is one deck: a playlist of at most the current file and the
//! next one (gapless), with per-file options (start, loudness gain, user
//! agent), pause, seek, volume and the equalizer. Its events are positions
//! and durations of the current file, the playlist moving on, and files
//! starting and ending with a reason. Every event carries the deck's
//! [serial](Player::serial), unique for the run, so the backend can tell
//! decks apart as they swap roles (see `backend::deck`).

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use anyhow::Result;
use serde_json::Value;
use tokio::sync::mpsc;

use crate::equalizer::Equalizer;

mod rust;

/// Serials of players, unique for the run.
static SERIAL: AtomicU64 = AtomicU64::new(1);

pub(crate) fn next_serial() -> u64 {
    SERIAL.fetch_add(1, Ordering::Relaxed)
}

/// Where every player's events go, tagged with its serial.
pub type Events = mpsc::UnboundedSender<(u64, PlayerEvent)>;

#[derive(Debug)]
pub enum PlayerEvent {
    /// The current file's position in seconds; `None` while nothing plays.
    Position(Option<f64>),
    /// The current file's length in seconds, once known.
    Duration(Option<f64>),
    Pause(bool),
    /// Waiting for data: the cache ran dry, or a seek is under way.
    Buffering(bool),
    /// Nothing loaded: the playlist ended or was stopped.
    Idle(bool),
    /// The playlist position of the current file (0 or 1), `None` when idle.
    PlaylistPos(Option<i64>),
    /// A playlist entry finished.
    EndFile {
        reason: EndReason,
        entry: i64,
        error: Option<String>,
    },
    StartFile {
        entry: i64,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EndReason {
    /// Played to its end.
    Eof,
    /// Replaced, removed or stopped.
    Stop,
    Error,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoadMode {
    /// Play it now, dropping the playlist.
    Replace,
    /// Queue it behind the current file, to follow without a gap.
    Append,
}

/// Where a file starts.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Start {
    Seconds(f64),
    /// A share of its length (0 to 1).
    Share(f64),
}

/// Options that hold for one file only, the gapless handoff included.
#[derive(Clone, Debug, Default)]
pub struct FileOptions {
    /// Loudness gain in dB.
    pub gain: f64,
    /// The user agent the stream was resolved with.
    pub user_agent: Option<String>,
    pub start: Option<Start>,
}

/// One deck of the in-process engine (`encore-audio`, docs/gpui/AUDIO.md).
pub struct Player(rust::Deck);

impl Player {
    /// Starts a deck at `volume` (0 to 100: amplitude is the cube of
    /// `volume / 100`); its events go to `events`.
    pub async fn spawn(volume: f64, events: Events) -> Result<Arc<Self>> {
        Ok(Arc::new(Player(
            rust::Deck::spawn(next_serial(), volume, events).await?,
        )))
    }

    /// This deck's serial: its events carry it.
    pub fn serial(&self) -> u64 {
        self.0.serial()
    }

    /// Loads `url`; returns its playlist entry id.
    pub async fn load(&self, url: &str, mode: LoadMode, options: &FileOptions) -> Result<i64> {
        self.0.load(url, mode, options)
    }

    /// Stops and empties the playlist.
    pub fn stop(&self) {
        self.0.stop();
    }

    /// Moves on to the next file in the playlist now.
    pub fn skip(&self) {
        self.0.skip();
    }

    /// Removes playlist entry `index` (0 or 1).
    pub fn remove(&self, index: i64) {
        self.0.remove(index);
    }

    pub fn seek(&self, seconds: f64) {
        self.0.seek(seconds);
    }

    pub fn set_pause(&self, paused: bool) {
        self.0.set_pause(paused);
    }

    /// 0 to 100: amplitude is the cube of `volume / 100`.
    pub fn set_volume(&self, volume: f64) {
        self.0.set_volume(volume);
    }

    /// The current file's loudness gain in dB, until it ends.
    pub fn set_gain(&self, gain: f64) {
        self.0.set_gain(gain);
    }

    /// Repeat one: the current file plays again at its end.
    pub fn set_loop(&self, looping: bool) {
        self.0.set_loop(looping);
    }

    /// Sets the whole equalizer.
    pub fn set_equalizer(&self, equalizer: &Equalizer) {
        self.0.set_equalizer(equalizer);
    }

    /// Changes the equalizer's bands in place, without a gap.
    pub async fn edit_equalizer(&self, _before: &Equalizer, now: &Equalizer) {
        self.0.set_equalizer(now);
    }

    /// The current file's length, asked now.
    pub async fn duration(&self) -> Option<f64> {
        self.0.duration()
    }

    /// A property, for checks and logs: `time-pos`, `duration`, `pause`,
    /// `volume`, `volume-gain`, `af`.
    pub async fn property(&self, name: &str) -> Option<Value> {
        self.0.property(name)
    }
}
