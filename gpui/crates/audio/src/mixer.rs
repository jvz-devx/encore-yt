//! The output callback's side: decks ("voices") that each play a current
//! track and switch to a queued next one in the same buffer (gapless),
//! summed with their volumes, then the equalizer.
//!
//! Everything here runs on the audio thread: commands and events cross over
//! through lock-free `rtrb` queues, samples through one ring per track.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use rtrb::{Consumer, Producer};

use crate::eq::{EqSettings, Equalizer};

pub const DECKS: usize = 3;
/// Volume and pause changes glide over about 5 ms, so they don't click.
const GLIDE_SECS: f32 = 0.005;

/// What the decoder thread and the API share about one track.
#[derive(Debug, Default)]
pub struct TrackShared {
    pub id: u64,
    /// Output frames played since `base`.
    pub played: AtomicU64,
    /// The output frame (in the track's own time) that `played` counts from.
    pub base: AtomicU64,
    /// Output frames of silence while the track waited for data, after it
    /// had started (re-buffering).
    pub starved: AtomicU64,
}

pub struct Source {
    pub shared: Arc<TrackShared>,
    pub ring: Consumer<f32>,
    /// Set once the decoder has pushed this ring's last sample. A seek
    /// brings a new ring with its own flag, so an old ring running dry
    /// never reads as the end.
    pub done: Arc<AtomicBool>,
    /// Linear loudness gain for this track only.
    pub gain: f32,
    started: bool,
    seeked: bool,
    /// A seek is under way: silent until its samples arrive.
    held: bool,
}

impl Source {
    pub fn new(
        shared: Arc<TrackShared>,
        ring: Consumer<f32>,
        done: Arc<AtomicBool>,
        gain: f32,
    ) -> Self {
        Self {
            shared,
            ring,
            done,
            gain,
            started: false,
            seeked: false,
            held: false,
        }
    }
}

pub enum Command {
    /// Plays this track now, dropping the deck's current and next ones.
    Load(usize, Source),
    /// Plays this track right after the current one ends.
    Queue(usize, Source),
    ClearNext(usize),
    /// Silences the deck's current track until its seek lands.
    Hold(usize),
    /// The decoder of `track` has seeked: its samples continue in `ring`.
    Replace {
        deck: usize,
        track: u64,
        ring: Consumer<f32>,
        done: Arc<AtomicBool>,
        base: u64,
    },
    Stop(usize),
    Pause(usize, bool),
    Volume(usize, f32),
    Equalizer(Option<EqSettings>),
}

#[derive(Clone, Copy, Debug)]
pub enum MixEvent {
    /// The track's first frame went out at output frame `at`.
    Started { deck: usize, track: u64, at: u64 },
    /// The track's last frame went out just before output frame `at`;
    /// `end` is the track's own length in output frames as played.
    Ended {
        deck: usize,
        track: u64,
        at: u64,
        end: u64,
    },
    /// The first frame after a seek went out.
    Seeked { deck: usize, track: u64, at: u64 },
}

#[derive(Default)]
struct Voice {
    current: Option<Source>,
    next: Option<Source>,
    volume: f32,
    level: f32,
    paused: bool,
}

pub struct Mixer {
    voices: [Voice; DECKS],
    commands: Consumer<Command>,
    events: Producer<MixEvent>,
    eq: Equalizer,
    scratch: Vec<f32>,
    glide: f32,
    /// Output frames rendered so far.
    frames: u64,
}

impl Mixer {
    pub fn new(rate: u32, commands: Consumer<Command>, events: Producer<MixEvent>) -> Self {
        let mut voices: [Voice; DECKS] = Default::default();
        for voice in &mut voices {
            voice.volume = 1.0;
            voice.level = 1.0;
        }
        Self {
            voices,
            commands,
            events,
            eq: Equalizer::default(),
            scratch: vec![0.0; 8192],
            glide: 1.0 - (-1.0 / (GLIDE_SECS * rate as f32)).exp(),
            frames: 0,
        }
    }

    /// Fills `out` (interleaved, `channels` per frame).
    pub fn render(&mut self, out: &mut [f32], channels: usize) {
        self.apply_commands();
        let frames = out.len() / channels.max(1);
        if self.scratch.len() < frames * 2 {
            self.scratch.resize(frames * 2, 0.0);
        }
        let mix = &mut self.scratch[..frames * 2];
        mix.fill(0.0);
        for (deck, voice) in self.voices.iter_mut().enumerate() {
            voice.mix(deck, mix, self.frames, self.glide, &mut self.events);
        }
        self.eq.process(mix);
        for (frame, stereo) in out.chunks_exact_mut(channels).zip(mix.as_chunks::<2>().0) {
            match channels {
                1 => frame[0] = (stereo[0] + stereo[1]) * 0.5,
                _ => {
                    frame[0] = stereo[0];
                    frame[1] = stereo[1];
                    frame[2..].fill(0.0);
                }
            }
        }
        self.frames += frames as u64;
    }

    fn apply_commands(&mut self) {
        while let Ok(command) = self.commands.pop() {
            match command {
                Command::Load(deck, source) => {
                    let voice = &mut self.voices[deck];
                    voice.current = Some(source);
                    voice.next = None;
                }
                Command::Queue(deck, source) => self.voices[deck].next = Some(source),
                Command::ClearNext(deck) => self.voices[deck].next = None,
                Command::Hold(deck) => {
                    if let Some(source) = self.voices[deck].current.as_mut() {
                        source.held = true;
                    }
                }
                Command::Replace {
                    deck,
                    track,
                    ring,
                    done,
                    base,
                } => {
                    if let Some(source) = self.voices[deck]
                        .current
                        .as_mut()
                        .filter(|s| s.shared.id == track)
                    {
                        source.ring = ring;
                        source.done = done;
                        source.shared.base.store(base, Ordering::Relaxed);
                        source.shared.played.store(0, Ordering::Relaxed);
                        source.seeked = true;
                        source.held = false;
                    }
                }
                Command::Stop(deck) => {
                    let voice = &mut self.voices[deck];
                    voice.current = None;
                    voice.next = None;
                }
                Command::Pause(deck, paused) => self.voices[deck].paused = paused,
                Command::Volume(deck, volume) => self.voices[deck].volume = volume,
                Command::Equalizer(settings) => self.eq.set(settings),
            }
        }
    }
}

impl Voice {
    fn mix(
        &mut self,
        deck: usize,
        mix: &mut [f32],
        frame0: u64,
        glide: f32,
        events: &mut Producer<MixEvent>,
    ) {
        let frames = mix.len() / 2;
        let target = if self.paused { 0.0 } else { self.volume };
        if self.paused && self.level < 1e-4 {
            self.level = 0.0;
            return;
        }
        let mut i = 0;
        while i < frames {
            let Some(source) = self.current.as_mut().filter(|s| !s.held) else {
                break;
            };
            let available = source.ring.slots() / 2;
            if available == 0 {
                if source.done.load(Ordering::Acquire) && source.ring.is_empty() {
                    let shared = &source.shared;
                    let end =
                        shared.base.load(Ordering::Relaxed) + shared.played.load(Ordering::Relaxed);
                    let _ = events.push(MixEvent::Ended {
                        deck,
                        track: shared.id,
                        at: frame0 + i as u64,
                        end,
                    });
                    self.current = self.next.take();
                    continue;
                }
                if source.started {
                    let short = (frames - i) as u64;
                    source.shared.starved.fetch_add(short, Ordering::Relaxed);
                }
                break;
            }
            let at = frame0 + i as u64;
            if !source.started {
                source.started = true;
                let track = source.shared.id;
                let _ = events.push(MixEvent::Started { deck, track, at });
            }
            if source.seeked {
                source.seeked = false;
                let track = source.shared.id;
                let _ = events.push(MixEvent::Seeked { deck, track, at });
            }
            let n = available.min(frames - i);
            let Ok(chunk) = source.ring.read_chunk(n * 2) else {
                break;
            };
            let (a, b) = chunk.as_slices();
            let out = &mut mix[i * 2..(i + n) * 2];
            let pairs = a.as_chunks::<2>().0.iter().chain(b.as_chunks::<2>().0);
            for (o, pair) in out.as_chunks_mut::<2>().0.iter_mut().zip(pairs) {
                self.level += (target - self.level) * glide;
                let g = self.level * source.gain;
                o[0] += pair[0] * g;
                o[1] += pair[1] * g;
            }
            chunk.commit_all();
            source.shared.played.fetch_add(n as u64, Ordering::Relaxed);
            i += n;
        }
    }
}
