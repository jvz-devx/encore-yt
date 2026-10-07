//! A pure Rust playback engine, as a spike for replacing mpv (PLAN M11,
//! docs/gpui/AUDIO.md).
//!
//! ```text
//! HttpSource (Range requests, read-ahead) → symphonia demux (WebM, MP4)
//!   → decode (libopus, symphonia AAC) → stereo → rubato → track ring
//!   → mixer (decks, gapless next, volume, loudness gain) → EQ → cpal
//! ```
//!
//! [`Engine`] owns the output and up to [`DECKS`] decks (the backend's main,
//! smooth-mix and audition decks). Each loaded track decodes on its own
//! thread into a lock-free ring; the output callback only reads rings and
//! commands, so it never waits on the network or a lock.

mod decode;
mod eq;
mod http;
mod mixer;
mod output;
mod padding;
mod resample;

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread;
use std::time::Duration;

use anyhow::Result;
use reqwest::blocking::Client;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use rtrb::{Consumer, Producer, RingBuffer};

pub use eq::{BANDS, RANGE};
pub use http::HttpStats;
pub use mixer::DECKS;

use decode::{Control, Ending, Job};
use http::StatsHandle;
use mixer::{Command, MixEvent, Source, TrackShared};

pub type TrackId = u64;

/// How a track is loaded.
#[derive(Clone, Debug, Default)]
pub struct Load {
    /// Start position in seconds.
    pub start: f64,
    /// Loudness gain in dB for this track only (mpv's `volume-gain`).
    pub gain_db: f32,
    /// Extra request headers (the user agent yt-dlp resolved with).
    pub headers: Vec<(String, String)>,
}

#[derive(Clone, Debug)]
pub enum Event {
    /// The track's first frame went to the device at output time `at`.
    Started {
        deck: usize,
        track: TrackId,
        at: Duration,
    },
    /// The track's last frame went out; a queued track starts at the same
    /// `at` (a gapless join has equal times). `length` is where the track
    /// ended in its own time, in seconds (its true length when trimmed).
    Ended {
        deck: usize,
        track: TrackId,
        at: Duration,
        length: f64,
    },
    /// The first frame after a seek went out.
    Seeked {
        deck: usize,
        track: TrackId,
        at: Duration,
    },
    Error {
        deck: usize,
        track: TrackId,
        message: String,
    },
}

/// Counters for one playing track.
#[derive(Clone, Debug, Default)]
pub struct TrackStats {
    pub track: TrackId,
    pub position: f64,
    /// Seconds of silence while waiting for data after the track started.
    pub starved: f64,
    pub http: HttpStats,
}

struct Track {
    shared: Arc<TrackShared>,
    control: Sender<Control>,
    stats: Arc<Mutex<Option<StatsHandle>>>,
}

impl Drop for Track {
    fn drop(&mut self) {
        let _ = self.control.send(Control::Stop);
    }
}

#[derive(Default)]
struct Deck {
    current: Option<Track>,
    next: Option<Track>,
}

type Decks = Arc<Mutex<[Deck; DECKS]>>;

pub struct Engine {
    mixer: Arc<Mutex<Producer<Command>>>,
    decks: Decks,
    events: Receiver<Event>,
    event_tx: Sender<Event>,
    client: Client,
    rate: u32,
    next_id: AtomicU64,
    stop: Arc<AtomicBool>,
    threads: Vec<thread::JoinHandle<()>>,
}

impl Engine {
    /// Opens the default output device.
    pub fn start() -> Result<Self> {
        let (commands, command_rx) = RingBuffer::new(256);
        let (event_producer, event_consumer) = RingBuffer::new(256);
        let stop = Arc::new(AtomicBool::new(false));
        let (output, rate) = output::spawn(command_rx, event_producer, stop.clone())?;
        let (event_tx, events) = mpsc::channel();
        let decks: Decks = Default::default();
        let forwarder = {
            let (decks, tx, stop) = (decks.clone(), event_tx.clone(), stop.clone());
            thread::Builder::new()
                .name("audio-events".into())
                .spawn(move || forward(event_consumer, decks, tx, rate, stop))?
        };
        Ok(Self {
            mixer: Arc::new(Mutex::new(commands)),
            decks,
            events,
            event_tx,
            client: Client::builder().build()?,
            rate,
            next_id: AtomicU64::new(1),
            stop,
            threads: vec![output, forwarder],
        })
    }

    pub fn output_rate(&self) -> u32 {
        self.rate
    }

    pub fn events(&self) -> &Receiver<Event> {
        &self.events
    }

    /// Plays `url` on `deck` now, replacing what it plays and its next track.
    pub fn load(&self, deck: usize, url: &str, load: Load) -> Result<TrackId> {
        let (track, source) = self.spawn_track(deck, url, &load)?;
        let id = track.shared.id;
        let mut decks = self.decks();
        decks[deck].current = Some(track);
        decks[deck].next = None;
        self.send(Command::Load(deck, source));
        Ok(id)
    }

    /// Queues `url` to start the moment the deck's current track ends.
    /// It starts downloading and decoding now, so the join has no gap.
    pub fn queue(&self, deck: usize, url: &str, load: Load) -> Result<TrackId> {
        let (track, source) = self.spawn_track(deck, url, &load)?;
        let id = track.shared.id;
        self.decks()[deck].next = Some(track);
        self.send(Command::Queue(deck, source));
        Ok(id)
    }

    pub fn clear_next(&self, deck: usize) {
        self.decks()[deck].next = None;
        self.send(Command::ClearNext(deck));
    }

    pub fn seek(&self, deck: usize, seconds: f64) {
        if let Some(track) = &self.decks()[deck].current {
            self.send(Command::Hold(deck));
            let _ = track.control.send(Control::Seek(seconds.max(0.0)));
        }
    }

    pub fn pause(&self, deck: usize, paused: bool) {
        self.send(Command::Pause(deck, paused));
    }

    /// Linear amplitude (the backend maps its 0–100 volume onto this).
    pub fn set_volume(&self, deck: usize, amplitude: f32) {
        self.send(Command::Volume(deck, amplitude.max(0.0)));
    }

    pub fn stop(&self, deck: usize) {
        let mut decks = self.decks();
        decks[deck] = Deck::default();
        self.send(Command::Stop(deck));
    }

    /// Band gains in dB at [`BANDS`], or `None` (off). Applies to every deck.
    pub fn set_equalizer(&self, gains: Option<[f32; 10]>) {
        let settings = gains.and_then(|g| eq::EqSettings::new(g, self.rate));
        self.send(Command::Equalizer(settings));
    }

    /// The deck's current track and its position in seconds.
    pub fn stats(&self, deck: usize) -> Option<TrackStats> {
        let decks = self.decks();
        let track = decks[deck].current.as_ref()?;
        let shared = &track.shared;
        let frames = shared.base.load(Ordering::Relaxed) + shared.played.load(Ordering::Relaxed);
        let http = track
            .stats
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
            .map(StatsHandle::get)
            .unwrap_or_default();
        Some(TrackStats {
            track: shared.id,
            position: frames as f64 / self.rate as f64,
            starved: shared.starved.load(Ordering::Relaxed) as f64 / self.rate as f64,
            http,
        })
    }

    fn spawn_track(&self, deck: usize, url: &str, load: &Load) -> Result<(Track, Source)> {
        anyhow::ensure!(deck < DECKS, "no deck {deck}");
        let mut headers = HeaderMap::new();
        for (name, value) in &load.headers {
            headers.insert(
                HeaderName::from_bytes(name.as_bytes())?,
                HeaderValue::from_str(value)?,
            );
        }
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let shared = Arc::new(TrackShared {
            id,
            ..Default::default()
        });
        let (producer, consumer): (Producer<f32>, Consumer<f32>) = decode::ring(self.rate);
        let done = Arc::new(AtomicBool::new(false));
        let (control, control_rx) = mpsc::channel();
        let stats = Arc::new(Mutex::new(None));
        let job = Job {
            deck,
            shared: shared.clone(),
            ring: producer,
            done: Ending::new(done.clone()),
            url: url.to_owned(),
            headers,
            start: load.start,
            rate: self.rate,
            client: self.client.clone(),
            control: control_rx,
            mixer: self.mixer.clone(),
            stats: stats.clone(),
        };
        let errors = self.event_tx.clone();
        decode::spawn(job, move |error| {
            log::warn!("track {id}: {error:#}");
            let _ = errors.send(Event::Error {
                deck,
                track: id,
                message: format!("{error:#}"),
            });
        });
        let gain = 10f32.powf(load.gain_db / 20.0);
        let source = Source::new(shared.clone(), consumer, done, gain);
        let track = Track {
            shared,
            control,
            stats,
        };
        Ok((track, source))
    }

    fn send(&self, command: Command) {
        decode::send(&self.mixer, command);
    }

    fn decks(&self) -> MutexGuard<'_, [Deck; DECKS]> {
        self.decks.lock().unwrap_or_else(|e| e.into_inner())
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        for thread in self.threads.drain(..) {
            thread.thread().unpark();
            let _ = thread.join();
        }
    }
}

/// Moves mixer events to the API's channel and keeps the deck bookkeeping
/// (a queued track becomes current when the one before it ends).
fn forward(
    mut events: Consumer<MixEvent>,
    decks: Decks,
    tx: Sender<Event>,
    rate: u32,
    stop: Arc<AtomicBool>,
) {
    let time = |frames: u64| Duration::from_secs_f64(frames as f64 / rate as f64);
    while !stop.load(Ordering::Acquire) {
        while let Ok(event) = events.pop() {
            let event = match event {
                MixEvent::Started { deck, track, at } => Event::Started {
                    deck,
                    track,
                    at: time(at),
                },
                MixEvent::Seeked { deck, track, at } => Event::Seeked {
                    deck,
                    track,
                    at: time(at),
                },
                MixEvent::Ended {
                    deck,
                    track,
                    at,
                    end,
                } => {
                    let mut decks = decks.lock().unwrap_or_else(|e| e.into_inner());
                    let d = &mut decks[deck];
                    if d.current.as_ref().is_some_and(|t| t.shared.id == track) {
                        d.current = d.next.take();
                    }
                    Event::Ended {
                        deck,
                        track,
                        at: time(at),
                        length: end as f64 / rate as f64,
                    }
                }
            };
            let _ = tx.send(event);
        }
        thread::sleep(Duration::from_millis(5));
    }
}
