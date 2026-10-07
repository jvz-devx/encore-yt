//! What the backend asks of an audio engine, so playback runs on mpv or
//! on another engine with the same behaviour.
//!
//! A [`Player`] is one deck: a playlist of at most the current file and the
//! next one (gapless), with per-file options (start, loudness gain, user
//! agent), pause, seek, volume and the equalizer. Its events follow mpv's
//! model, which the backend was built on: positions and durations of the
//! current file, the playlist moving on, files starting and ending with a
//! reason, and the deck going away. Every event carries the deck's
//! [serial](Player::serial), unique for the run, so the backend can tell
//! decks apart as they swap roles (see `backend::deck`).

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::sync::mpsc;

use crate::equalizer::{Equalizer, LABEL};
use crate::mpv::Mpv;

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
    /// The player went away (mpv exited).
    Died,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EndReason {
    /// Played to its end.
    Eof,
    /// Replaced, removed or stopped.
    Stop,
    Error,
    /// Anything else (mpv's `quit`, `redirect`).
    Other,
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

/// The audio engines.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    /// An mpv process per deck, over its JSON IPC.
    #[default]
    Mpv,
}

impl Kind {
    pub fn label(self) -> &'static str {
        match self {
            Kind::Mpv => "mpv",
        }
    }

    /// The engine to use: `YTFAST_PLAYER=mpv`, else the saved setting,
    /// else the default.
    pub fn choose(setting: Option<Kind>) -> Kind {
        match std::env::var("YTFAST_PLAYER").ok().as_deref() {
            Some("mpv") => return Kind::Mpv,
            Some(other) => log::warn!("YTFAST_PLAYER={other}: unknown, using the setting"),
            None => {}
        }
        setting.unwrap_or_default()
    }
}

/// One deck on one of the engines.
pub enum Player {
    Mpv(Mpv),
}

impl Player {
    /// Starts a deck at `volume` (0 to 100, mpv's scale); its events go to
    /// `events`. `socket` is mpv's control socket.
    pub async fn spawn(
        kind: Kind,
        socket: &Path,
        volume: f64,
        events: Events,
    ) -> Result<Arc<Self>> {
        Ok(Arc::new(match kind {
            Kind::Mpv => Player::Mpv(Mpv::spawn(socket, volume, events).await?),
        }))
    }

    pub fn kind(&self) -> Kind {
        match self {
            Player::Mpv(_) => Kind::Mpv,
        }
    }

    /// This deck's serial: its events carry it.
    pub fn serial(&self) -> u64 {
        match self {
            Player::Mpv(mpv) => mpv.serial(),
        }
    }

    /// Loads `url`; returns its playlist entry id.
    pub async fn load(&self, url: &str, mode: LoadMode, options: &FileOptions) -> Result<i64> {
        match self {
            Player::Mpv(mpv) => {
                let mode = match mode {
                    LoadMode::Replace => "replace",
                    LoadMode::Append => "append",
                };
                // Always set, so a gain changed while the song plays ends with it.
                let mut list = vec![("volume-gain", format!("{:.2}", options.gain))];
                if let Some(agent) = &options.user_agent {
                    list.push(("user-agent", agent.clone()));
                }
                match options.start {
                    Some(Start::Seconds(at)) => list.push(("start", format!("{at:.2}"))),
                    // mpv takes "33%" once it knows the length.
                    Some(Start::Share(share)) => {
                        list.push(("start", format!("{:.0}%", share * 100.0)));
                    }
                    None => {}
                }
                mpv.load(url, mode, &list).await
            }
        }
    }

    /// Stops and empties the playlist.
    pub async fn stop(&self) -> Result<()> {
        match self {
            Player::Mpv(mpv) => mpv.command(json!(["stop"])).await.map(drop),
        }
    }

    /// Moves on to the next file in the playlist now.
    pub async fn skip(&self) -> Result<()> {
        match self {
            Player::Mpv(mpv) => mpv
                .command(json!(["playlist-next", "force"]))
                .await
                .map(drop),
        }
    }

    /// Removes playlist entry `index` (0 or 1).
    pub async fn remove(&self, index: i64) -> Result<()> {
        match self {
            Player::Mpv(mpv) => mpv
                .command(json!(["playlist-remove", index]))
                .await
                .map(drop),
        }
    }

    pub async fn seek(&self, seconds: f64) -> Result<()> {
        match self {
            Player::Mpv(mpv) => mpv
                .command(json!(["seek", seconds, "absolute"]))
                .await
                .map(drop),
        }
    }

    pub async fn set_pause(&self, paused: bool) -> Result<()> {
        match self {
            Player::Mpv(mpv) => mpv.set("pause", json!(paused)).await,
        }
    }

    /// 0 to 100, mpv's scale: amplitude is the cube of `volume / 100`.
    pub async fn set_volume(&self, volume: f64) -> Result<()> {
        match self {
            Player::Mpv(mpv) => mpv.set("volume", json!(volume)).await,
        }
    }

    /// The current file's loudness gain in dB, until it ends.
    pub async fn set_gain(&self, gain: f64) -> Result<()> {
        match self {
            Player::Mpv(mpv) => mpv.set("volume-gain", json!(gain)).await,
        }
    }

    /// Repeat one: the current file plays again at its end.
    pub async fn set_loop(&self, looping: bool) -> Result<()> {
        match self {
            Player::Mpv(mpv) => {
                mpv.set("loop-file", json!(if looping { "inf" } else { "no" }))
                    .await
            }
        }
    }

    /// Sets the whole equalizer (mpv rebuilds its filter graph).
    pub async fn set_equalizer(&self, equalizer: &Equalizer) -> Result<()> {
        match self {
            Player::Mpv(mpv) => mpv.set("af", json!(equalizer.filter())).await,
        }
    }

    /// Changes the bands of the equalizer set as `before` to `now`, in
    /// place and without a gap. Both must be active.
    pub async fn edit_equalizer(&self, before: &Equalizer, now: &Equalizer) {
        match self {
            Player::Mpv(mpv) => {
                for (i, (old, new)) in before.gains.iter().zip(now.gains).enumerate() {
                    if (old - new).abs() >= 0.05 {
                        let _ = mpv
                            .command(json!([
                                "af-command",
                                LABEL,
                                "g",
                                format!("{new:.1}"),
                                format!("equalizer@b{i}")
                            ]))
                            .await;
                    }
                }
                if (before.preamp() - now.preamp()).abs() >= 0.05 {
                    let _ = mpv
                        .command(json!([
                            "af-command",
                            LABEL,
                            "volume",
                            format!("{:.1}dB", now.preamp()),
                            "volume@pre"
                        ]))
                        .await;
                }
            }
        }
    }

    /// The current file's length, asked now.
    pub async fn duration(&self) -> Option<f64> {
        self.property("duration").await.and_then(|v| v.as_f64())
    }

    /// A property by mpv's name, for checks and logs: `time-pos`,
    /// `duration`, `pause`, `volume`, `volume-gain`, `af`.
    pub async fn property(&self, name: &str) -> Option<Value> {
        match self {
            Player::Mpv(mpv) => mpv.get(name).await.ok(),
        }
    }
}
