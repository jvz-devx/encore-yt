//! The Rust engine (`ytfast-audio`, docs/gpui/AUDIO.md) behind the player
//! interface, with the playlist model kept here.
//!
//! One engine per process owns the audio output; each [`Deck`] is one of its
//! decks. The engine plays the current track and, queued behind it, the
//! next one, and switches in the same buffer (gapless); this side keeps the
//! playlist entries, their ids and options, and turns the engine's events
//! and a 100 ms poll of its counters into [`PlayerEvent`]s: start and end
//! of a file with its reason, the playlist moving on, position, duration,
//! waiting for data, pause and idle.

use std::sync::mpsc::RecvTimeoutError;
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, Weak};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow, bail};
use serde_json::{Value, json};
use ytfast_audio as audio;

use super::{EndReason, Events, FileOptions, LoadMode, PlayerEvent, Start};
use crate::equalizer::Equalizer;

/// How often positions and buffering are read from the engine.
const POLL: Duration = Duration::from_millis(100);
/// A playing track whose position hasn't moved for this many polls is
/// waiting for data.
const STALLED_POLLS: u32 = 3;

/// The process's engine and which deck each [`Deck`] holds.
struct Shared {
    engine: audio::Engine,
    slots: Mutex<[Option<Weak<Inner>>; audio::DECKS]>,
    /// The band gains the engine's equalizer has (`None`: off).
    equalizer: Mutex<Option<[f32; 10]>>,
}

static SHARED: OnceLock<tokio::sync::Mutex<Option<Arc<Shared>>>> = OnceLock::new();

/// The engine, started on first use. A failed start (no audio device) is
/// tried again by the next deck.
async fn shared() -> Result<Arc<Shared>> {
    let mut guard = SHARED.get_or_init(Default::default).lock().await;
    if let Some(shared) = guard.as_ref() {
        return Ok(shared.clone());
    }
    // Opening the output and the blocking HTTP client must not run on
    // the async runtime's threads.
    let engine = tokio::task::spawn_blocking(audio::Engine::start)
        .await
        .context("starting the audio engine")??;
    let events = engine
        .take_events()
        .ok_or_else(|| anyhow!("the audio engine's events are taken"))?;
    log::info!(
        "audio: the Rust engine plays at {} Hz",
        engine.output_rate()
    );
    let shared = Arc::new(Shared {
        engine,
        slots: Mutex::new(Default::default()),
        equalizer: Mutex::new(None),
    });
    let weak = Arc::downgrade(&shared);
    thread::Builder::new()
        .name("audio-player".into())
        .spawn(move || route(weak, events))?;
    *guard = Some(shared.clone());
    Ok(shared)
}

impl Shared {
    fn slots(&self) -> MutexGuard<'_, [Option<Weak<Inner>>; audio::DECKS]> {
        self.slots.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn decks(&self) -> Vec<Arc<Inner>> {
        self.slots()
            .iter()
            .flatten()
            .filter_map(Weak::upgrade)
            .collect()
    }

    fn deck(&self, index: usize) -> Option<Arc<Inner>> {
        self.slots().get(index)?.as_ref()?.upgrade()
    }
}

/// The engine's events and the poll, on a thread of their own.
fn route(shared: Weak<Shared>, events: std::sync::mpsc::Receiver<audio::Event>) {
    let mut polled = Instant::now();
    loop {
        let Some(shared) = shared.upgrade() else {
            return;
        };
        match events.recv_timeout(Duration::from_millis(20)) {
            Ok(event) => {
                let deck = match &event {
                    audio::Event::Started { deck, .. }
                    | audio::Event::Ended { deck, .. }
                    | audio::Event::Seeked { deck, .. }
                    | audio::Event::Error { deck, .. } => *deck,
                };
                if let Some(inner) = shared.deck(deck) {
                    inner.engine_event(event);
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return,
        }
        if polled.elapsed() >= POLL {
            polled = Instant::now();
            for inner in shared.decks() {
                inner.poll();
            }
        }
    }
}

/// One playlist entry.
struct Entry {
    id: i64,
    url: String,
    options: FileOptions,
    /// The engine's track, while the engine has it loaded or queued.
    track: Option<audio::TrackId>,
    /// The engine reported an error for it before it became current.
    failed: Option<String>,
}

impl Entry {
    fn load(&self) -> audio::Load {
        let (start, start_share) = match self.options.start {
            Some(Start::Seconds(at)) => (at.max(0.0), None),
            Some(Start::Share(share)) => (0.0, Some(share)),
            None => (0.0, None),
        };
        audio::Load {
            start,
            start_share,
            gain_db: self.options.gain as f32,
            headers: self
                .options
                .user_agent
                .iter()
                .map(|agent| ("User-Agent".to_owned(), agent.clone()))
                .collect(),
        }
    }
}

#[derive(Default)]
struct State {
    next_id: i64,
    current: Option<Entry>,
    next: Option<Entry>,
    /// The playlist position of the current entry: 1 after a gapless
    /// change until the backend removes the entry before it.
    pos: i64,
    idle: bool,
    paused: bool,
    volume: f64,
    looping: bool,
    equalizer: Option<Equalizer>,
    /// The current track's first frame went out.
    started: bool,
    seeking: bool,
    stalled: u32,
    buffering: bool,
    position: Option<f64>,
    duration: Option<f64>,
}

struct Inner {
    serial: u64,
    deck: usize,
    shared: Arc<Shared>,
    events: Events,
    state: Mutex<State>,
}

/// A deck of the Rust engine.
pub struct Deck {
    inner: Arc<Inner>,
}

impl Deck {
    pub async fn spawn(serial: u64, volume: f64, events: Events) -> Result<Self> {
        let shared = shared().await?;
        let inner = {
            let mut slots = shared.slots();
            let deck = slots
                .iter()
                .position(Option::is_none)
                .ok_or_else(|| anyhow!("all {} audio decks are in use", audio::DECKS))?;
            let inner = Arc::new(Inner {
                serial,
                deck,
                shared: shared.clone(),
                events,
                state: Mutex::new(State {
                    next_id: 1,
                    idle: true,
                    volume,
                    ..State::default()
                }),
            });
            slots[deck] = Some(Arc::downgrade(&inner));
            inner
        };
        let engine = &shared.engine;
        engine.stop(inner.deck);
        engine.pause(inner.deck, false);
        engine.set_volume(inner.deck, amplitude(volume));
        // The state a new deck reports.
        for event in [
            PlayerEvent::Position(None),
            PlayerEvent::Duration(None),
            PlayerEvent::Pause(false),
            PlayerEvent::PlaylistPos(None),
            PlayerEvent::Idle(true),
            PlayerEvent::Buffering(false),
        ] {
            inner.emit(event);
        }
        Ok(Self { inner })
    }

    pub fn serial(&self) -> u64 {
        self.inner.serial
    }

    pub fn load(&self, url: &str, mode: LoadMode, options: &FileOptions) -> Result<i64> {
        let inner = &self.inner;
        let mut st = inner.state();
        let id = st.next_id;
        let entry = Entry {
            id,
            url: url.to_owned(),
            options: options.clone(),
            track: None,
            failed: None,
        };
        match mode {
            LoadMode::Replace => {
                let track = inner.engine().load(inner.deck, url, entry.load())?;
                log::info!(
                    "audio: deck {} plays entry {id} (track {track}, gain {:+.2} dB, start {:?})",
                    inner.deck,
                    options.gain,
                    options.start
                );
                st.next_id += 1;
                let old = st.current.replace(Entry {
                    track: Some(track),
                    ..entry
                });
                st.next = None;
                if let Some(old) = old {
                    inner.emit(end(EndReason::Stop, old.id, None));
                }
                inner.started(&mut st, id, 0);
            }
            LoadMode::Append => {
                if st.current.is_none() {
                    bail!("nothing is playing to queue behind");
                }
                let mut entry = entry;
                if !st.looping {
                    entry.track = Some(inner.engine().queue(inner.deck, url, entry.load())?);
                }
                log::info!(
                    "audio: deck {} queues entry {id} (gain {:+.2} dB) for a gapless change",
                    inner.deck,
                    options.gain
                );
                st.next_id += 1;
                st.next = Some(entry);
            }
        }
        Ok(id)
    }

    pub fn stop(&self) {
        let inner = &self.inner;
        let mut st = inner.state();
        inner.engine().stop(inner.deck);
        st.next = None;
        if let Some(old) = st.current.take() {
            inner.emit(end(EndReason::Stop, old.id, None));
        }
        inner.went_idle(&mut st);
    }

    /// Moves on to the next entry now.
    pub fn skip(&self) {
        let inner = &self.inner;
        let mut st = inner.state();
        let Some(next) = st.next.take() else {
            drop(st);
            return self.stop();
        };
        log::info!("audio: deck {} skips to entry {}", inner.deck, next.id);
        let next = match next.track {
            Some(_) => {
                inner.engine().skip(inner.deck);
                next
            }
            // Repeat one held it back from the engine; a failed one has none.
            None if next.failed.is_none() => {
                match inner.engine().load(inner.deck, &next.url, next.load()) {
                    Ok(track) => Entry {
                        track: Some(track),
                        ..next
                    },
                    Err(error) => Entry {
                        failed: Some(format!("{error:#}")),
                        ..next
                    },
                }
            }
            None => next,
        };
        if let Some(old) = st.current.replace(next) {
            inner.emit(end(EndReason::Stop, old.id, None));
        }
        inner.advanced(&mut st);
    }

    pub fn remove(&self, index: i64) {
        let inner = &self.inner;
        let mut st = inner.state();
        if index < st.pos {
            st.pos -= 1;
            inner.emit(PlayerEvent::PlaylistPos(Some(st.pos)));
        } else if index == st.pos + 1 && st.next.take().is_some() {
            inner.engine().clear_next(inner.deck);
        } else if index == st.pos && st.current.is_some() {
            // The current entry goes and the next one plays.
            drop(st);
            self.skip();
        }
    }

    pub fn seek(&self, seconds: f64) {
        let inner = &self.inner;
        let mut st = inner.state();
        if st.current.as_ref().is_some_and(|c| c.track.is_some()) {
            inner.engine().seek(inner.deck, seconds);
            st.seeking = true;
            inner.update_buffering(&mut st);
        }
    }

    pub fn set_pause(&self, paused: bool) {
        let inner = &self.inner;
        let mut st = inner.state();
        inner.engine().pause(inner.deck, paused);
        st.stalled = 0;
        if st.paused != paused {
            st.paused = paused;
            inner.emit(PlayerEvent::Pause(paused));
        }
        inner.update_buffering(&mut st);
    }

    pub fn set_volume(&self, volume: f64) {
        let inner = &self.inner;
        log::debug!("audio: deck {} volume {volume:.1}", inner.deck);
        inner.state().volume = volume;
        inner.engine().set_volume(inner.deck, amplitude(volume));
    }

    pub fn set_gain(&self, gain: f64) {
        let inner = &self.inner;
        let mut st = inner.state();
        if let Some(current) = st.current.as_mut() {
            log::info!(
                "audio: deck {} entry {} gain {gain:+.2} dB",
                inner.deck,
                current.id
            );
            current.options.gain = gain;
            inner.engine().set_gain(inner.deck, gain as f32);
        }
    }

    /// Repeat one: the next entry stays out of the engine, and the current
    /// one loads again at its end.
    pub fn set_loop(&self, looping: bool) {
        let inner = &self.inner;
        let mut st = inner.state();
        if st.looping == looping {
            return;
        }
        st.looping = looping;
        let engine = inner.engine();
        if looping {
            if let Some(next) = st.next.as_mut()
                && next.track.take().is_some()
            {
                engine.clear_next(inner.deck);
            }
        } else if let Some(next) = st.next.as_mut()
            && next.track.is_none()
            && next.failed.is_none()
        {
            match engine.queue(inner.deck, &next.url, next.load()) {
                Ok(track) => next.track = Some(track),
                Err(error) => next.failed = Some(format!("{error:#}")),
            }
        }
    }

    /// The engine's equalizer is on its summed output, so every deck shares it.
    pub fn set_equalizer(&self, equalizer: &Equalizer) {
        let inner = &self.inner;
        let gains = equalizer.active().then_some(equalizer.gains);
        let mut applied = inner
            .shared
            .equalizer
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        if *applied != gains {
            *applied = gains;
            match gains {
                Some(gains) => log::info!(
                    "audio: equalizer {} {gains:?} dB, preamp {:.1} dB",
                    equalizer.preset.label(),
                    equalizer.preamp()
                ),
                None => log::info!("audio: equalizer off"),
            }
            inner.engine().set_equalizer(gains);
        }
        drop(applied);
        inner.state().equalizer = Some(equalizer.clone());
    }

    pub fn duration(&self) -> Option<f64> {
        let inner = &self.inner;
        let st = inner.state();
        let track = st.current.as_ref()?.track?;
        let stats = inner.engine().stats(inner.deck)?;
        (stats.track == track).then_some(stats.duration).flatten()
    }

    pub fn property(&self, name: &str) -> Option<Value> {
        let inner = &self.inner;
        if name == "duration" {
            return self.duration().map(|d| json!(d));
        }
        let st = inner.state();
        Some(match name {
            "time-pos" => json!(st.position?),
            "pause" => json!(st.paused),
            "volume" => json!(st.volume),
            "volume-gain" => json!(st.current.as_ref()?.options.gain),
            "af" => json!(st.equalizer.as_ref().map(Equalizer::filter)),
            "path" => json!(st.current.as_ref()?.url),
            _ => return None,
        })
    }
}

impl Drop for Inner {
    fn drop(&mut self) {
        let mut slots = self.shared.slots();
        self.shared.engine.stop(self.deck);
        slots[self.deck] = None;
    }
}

impl Inner {
    fn state(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn engine(&self) -> &audio::Engine {
        &self.shared.engine
    }

    fn emit(&self, event: PlayerEvent) {
        let _ = self.events.send((self.serial, event));
    }

    /// Entry `id` became current at playlist position `pos`.
    fn started(&self, st: &mut State, id: i64, pos: i64) {
        st.pos = pos;
        st.started = false;
        st.seeking = false;
        st.stalled = 0;
        st.position = None;
        st.duration = None;
        self.emit(PlayerEvent::StartFile { entry: id });
        self.emit(PlayerEvent::PlaylistPos(Some(pos)));
        if st.idle {
            st.idle = false;
            self.emit(PlayerEvent::Idle(false));
        }
        self.update_buffering(st);
    }

    /// The next entry is current now (gapless, or skipped to); one the
    /// engine failed on ends with its error at once.
    fn advanced(&self, st: &mut State) {
        let Some((id, failed)) = st.current.as_mut().map(|c| (c.id, c.failed.take())) else {
            return;
        };
        self.started(st, id, 1);
        if let Some(error) = failed {
            self.engine().stop(self.deck);
            st.current = None;
            self.emit(end(EndReason::Error, id, Some(error)));
            self.went_idle(st);
        }
    }

    fn went_idle(&self, st: &mut State) {
        st.started = false;
        st.seeking = false;
        st.position = None;
        st.duration = None;
        if !st.idle {
            st.idle = true;
            self.emit(PlayerEvent::Position(None));
            self.emit(PlayerEvent::PlaylistPos(None));
            self.emit(PlayerEvent::Idle(true));
        }
        self.update_buffering(st);
    }

    /// Waiting for data and seeking as one flag: loaded and not
    /// yet playing, a seek under way, or stalled. Never while paused: the
    /// engine starts tracks and lands seeks only once it plays.
    fn update_buffering(&self, st: &mut State) {
        let waiting = st.current.is_some()
            && !st.paused
            && (!st.started || st.seeking || st.stalled >= STALLED_POLLS);
        if waiting != st.buffering {
            st.buffering = waiting;
            self.emit(PlayerEvent::Buffering(waiting));
        }
    }

    fn engine_event(&self, event: audio::Event) {
        let mut st = self.state();
        let current = st.current.as_ref().and_then(|c| c.track);
        match event {
            audio::Event::Started { track, .. } if Some(track) == current => {
                st.started = true;
                self.update_buffering(&mut st);
            }
            audio::Event::Seeked { track, .. } if Some(track) == current => {
                st.seeking = false;
                st.stalled = 0;
                self.update_buffering(&mut st);
            }
            audio::Event::Ended { track, length, .. } if Some(track) == current => {
                log::info!(
                    "audio: deck {} track {track} ended, {length:.3} s played to its end",
                    self.deck
                );
                self.ended(&mut st);
            }
            audio::Event::Error { track, message, .. } => {
                log::warn!("audio: deck {} track {track}: {message}", self.deck);
                if Some(track) == current {
                    let Some(entry) = st.current.take() else {
                        return;
                    };
                    self.engine().stop(self.deck);
                    st.next = None;
                    self.emit(end(EndReason::Error, entry.id, Some(message)));
                    self.went_idle(&mut st);
                } else if let Some(next) = st.next.as_mut().filter(|n| n.track == Some(track)) {
                    next.track = None;
                    next.failed = Some(message);
                    self.engine().clear_next(self.deck);
                }
            }
            _ => {}
        }
    }

    /// The current track played to its end.
    fn ended(&self, st: &mut State) {
        if st.looping
            && let Some(current) = st.current.as_mut()
        {
            let mut load = current.load();
            (load.start, load.start_share) = (0.0, None);
            match self.engine().load(self.deck, &current.url, load) {
                Ok(track) => {
                    current.track = Some(track);
                    st.started = false;
                    st.position = None;
                    self.update_buffering(st);
                    return;
                }
                Err(error) => log::warn!("audio: repeating the song: {error:#}"),
            }
        }
        let Some(old) = st.current.take() else {
            return;
        };
        self.emit(end(EndReason::Eof, old.id, None));
        match st.next.take() {
            // The engine started it in the same buffer.
            Some(next) => {
                log::info!(
                    "audio: deck {} entry {} follows entry {} without a gap",
                    self.deck,
                    next.id,
                    old.id
                );
                st.current = Some(next);
                self.advanced(st);
            }
            None => self.went_idle(st),
        }
    }

    /// Position, duration and stalls from the engine's counters.
    fn poll(&self) {
        let mut st = self.state();
        let Some(track) = st.current.as_ref().and_then(|c| c.track) else {
            return;
        };
        let Some(stats) = self.engine().stats(self.deck).filter(|s| s.track == track) else {
            return;
        };
        if stats.duration.is_some() && stats.duration != st.duration {
            st.duration = stats.duration;
            self.emit(PlayerEvent::Duration(stats.duration));
        }
        if !st.started || st.seeking {
            return;
        }
        let moved = st
            .position
            .is_none_or(|p| (p - stats.position).abs() > 1e-6);
        st.stalled = if moved || st.paused {
            0
        } else {
            st.stalled + 1
        };
        if moved {
            st.position = Some(stats.position);
            self.emit(PlayerEvent::Position(Some(stats.position)));
        }
        self.update_buffering(&mut st);
    }
}

fn end(reason: EndReason, entry: i64, error: Option<String>) -> PlayerEvent {
    PlayerEvent::EndFile {
        reason,
        entry,
        error,
    }
}

/// The volume scale: amplitude is the cube of `volume / 100`.
fn amplitude(volume: f64) -> f32 {
    (volume.max(0.0) / 100.0).powi(3) as f32
}
