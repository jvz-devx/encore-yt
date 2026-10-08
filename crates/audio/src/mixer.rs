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

/// Main, smooth-mix (cued or tail), a spare, and audition, with room for a
/// deck that is still being dropped while its replacement starts.
pub const DECKS: usize = 6;
/// Volume and pause changes glide over about 5 ms, so they don't click.
const GLIDE_SECS: f32 = 0.005;
/// A track starts, and resumes after a seek or running dry, only once this
/// much is decoded (or it ends sooner), so a slow network gives one wait
/// instead of stutter.
const PREBUFFER_SECS: f32 = 0.1;

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
    /// The track's length in seconds as `f64` bits, once the decoder knows
    /// it (0 until then).
    pub duration: AtomicU64,
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
    /// Filling up to the prebuffer before playing.
    waiting: bool,
    /// A seek is under way: silent until its samples arrive.
    held: bool,
    /// Preserve the actual end time while retrying a full event queue.
    ended_at: Option<u64>,
}

impl Source {
    /// Counts silence after the track started, except while a seek lands.
    fn starve(&self, frames: usize) {
        if self.started && !self.seeked {
            let shared = &self.shared;
            shared.starved.fetch_add(frames as u64, Ordering::Relaxed);
        }
    }

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
            waiting: true,
            held: false,
            ended_at: None,
        }
    }
}

pub enum Command {
    /// Plays this track now, dropping the deck's current and next ones.
    Load(usize, Source),
    /// Plays this track right after the current one ends.
    Queue(usize, Source),
    ClearNext(usize),
    /// The queued track becomes current now, dropping the current one.
    Skip(usize),
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
    /// The current track's loudness gain (linear), from now on.
    Gain(usize, f32),
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
    prebuffer: usize,
    /// Output frames rendered so far.
    frames: u64,
}

impl Mixer {
    pub fn new(rate: u32, commands: Consumer<Command>, events: Producer<MixEvent>) -> Self {
        crate::tap::set_rate(rate);
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
            prebuffer: (PREBUFFER_SECS * rate as f32) as usize,
            frames: 0,
        }
    }

    /// Fills `out` (interleaved, `channels` per frame).
    pub fn render(&mut self, out: &mut [f32], channels: usize) {
        self.apply_commands();
        out.fill(0.0);
        if channels == 0 {
            return;
        }
        // Device callback sizes vary. Reuse the fixed stereo scratch buffer
        // for every chunk instead of allocating on the real-time thread.
        let chunk_samples = channels.saturating_mul(self.scratch.len() / 2);
        for chunk in out.chunks_mut(chunk_samples) {
            self.render_chunk(chunk, channels);
        }
    }

    fn render_chunk(&mut self, out: &mut [f32], channels: usize) {
        let frames = out.len() / channels;
        let mix = &mut self.scratch[..frames * 2];
        mix.fill(0.0);
        for (deck, voice) in self.voices.iter_mut().enumerate() {
            let clock = Clock {
                frame0: self.frames,
                glide: self.glide,
                prebuffer: self.prebuffer,
            };
            voice.mix(deck, mix, clock, &mut self.events);
        }
        self.eq.process(mix);
        crate::tap::write(mix);
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
                Command::Skip(deck) => {
                    let voice = &mut self.voices[deck];
                    voice.current = voice.next.take();
                }
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
                        source.waiting = true;
                        source.ended_at = None;
                    }
                }
                Command::Stop(deck) => {
                    let voice = &mut self.voices[deck];
                    voice.current = None;
                    voice.next = None;
                }
                Command::Pause(deck, paused) => self.voices[deck].paused = paused,
                Command::Volume(deck, volume) => self.voices[deck].volume = volume,
                Command::Gain(deck, gain) => {
                    if let Some(source) = self.voices[deck].current.as_mut() {
                        source.gain = gain;
                    }
                }
                Command::Equalizer(settings) => self.eq.set(settings),
            }
        }
    }
}

/// Per-callback constants for the voices.
#[derive(Clone, Copy)]
struct Clock {
    frame0: u64,
    glide: f32,
    prebuffer: usize,
}

impl Voice {
    fn mix(&mut self, deck: usize, mix: &mut [f32], clock: Clock, events: &mut Producer<MixEvent>) {
        let Clock {
            frame0,
            glide,
            prebuffer,
        } = clock;
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
                    let at = *source.ended_at.get_or_insert(frame0 + i as u64);
                    if events
                        .push(MixEvent::Ended {
                            deck,
                            track: shared.id,
                            at,
                            end,
                        })
                        .is_err()
                    {
                        // Retry next callback without losing the transition or
                        // blocking the audio thread when the consumer falls behind.
                        break;
                    }
                    self.current = self.next.take();
                    continue;
                }
                source.waiting = true;
                source.starve(frames - i);
                break;
            }
            if source.waiting {
                if available < prebuffer && !source.done.load(Ordering::Acquire) {
                    source.starve(frames - i);
                    break;
                }
                source.waiting = false;
            }
            let at = frame0 + i as u64;
            if !source.started {
                let track = source.shared.id;
                if events.push(MixEvent::Started { deck, track, at }).is_err() {
                    break;
                }
                source.started = true;
            }
            if source.seeked {
                let track = source.shared.id;
                if events.push(MixEvent::Seeked { deck, track, at }).is_err() {
                    break;
                }
                source.seeked = false;
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

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "test assertions report fixture failures"
)]
mod tests {
    use super::*;
    use rtrb::RingBuffer;

    fn source(frames: usize) -> Source {
        let (mut tx, rx) = RingBuffer::new(frames * 2);
        for _ in 0..frames {
            tx.push(0.25).unwrap();
            tx.push(-0.25).unwrap();
        }
        Source::new(
            Arc::new(TrackShared {
                id: 1,
                ..TrackShared::default()
            }),
            rx,
            Arc::new(AtomicBool::new(true)),
            1.0,
        )
    }

    #[test]
    fn large_callbacks_reuse_scratch_and_keep_frame_positions() {
        let (mut commands, rx) = RingBuffer::new(8);
        let (events, mut received) = RingBuffer::new(8);
        let mut mixer = Mixer::new(48_000, rx, events);
        let scratch = mixer.scratch.as_ptr();
        let capacity = mixer.scratch.capacity();
        assert!(commands.push(Command::Load(0, source(10_000))).is_ok());
        let mut output = vec![0.0; 20_002];
        mixer.render(&mut output, 2);
        assert_eq!(mixer.scratch.as_ptr(), scratch);
        assert_eq!(mixer.scratch.capacity(), capacity);
        assert!(
            output[..20_000]
                .as_chunks::<2>()
                .0
                .iter()
                .all(|s| *s == [0.25, -0.25])
        );
        assert_eq!(&output[20_000..], &[0.0, 0.0]);
        assert!(matches!(
            received.pop(),
            Ok(MixEvent::Started { at: 0, .. })
        ));
        assert!(matches!(
            received.pop(),
            Ok(MixEvent::Ended { at: 10_000, .. })
        ));
    }

    #[test]
    fn full_event_ring_retries_transitions_without_consuming_samples() {
        let (mut commands, rx) = RingBuffer::new(8);
        let (mut events, mut received) = RingBuffer::new(1);
        events
            .push(MixEvent::Started {
                deck: 1,
                track: 0,
                at: 0,
            })
            .unwrap();
        let mut mixer = Mixer::new(48_000, rx, events);
        let source = source(1);
        let shared = source.shared.clone();
        assert!(commands.push(Command::Load(0, source)).is_ok());
        let mut output = [0.0; 4];
        mixer.render(&mut output, 2);
        assert_eq!(shared.played.load(Ordering::Relaxed), 0);
        received.pop().unwrap();
        mixer.render(&mut output, 2);
        assert_eq!(shared.played.load(Ordering::Relaxed), 1);
        assert!(matches!(
            received.pop(),
            Ok(MixEvent::Started { track: 1, .. })
        ));
        mixer.render(&mut output, 2);
        assert!(matches!(
            received.pop(),
            Ok(MixEvent::Ended { track: 1, .. })
        ));
    }

    #[test]
    fn malformed_channel_layout_is_silent() {
        let (_commands, rx) = RingBuffer::new(8);
        let (events, _received) = RingBuffer::new(8);
        let mut mixer = Mixer::new(48_000, rx, events);
        let mut output = [1.0; 3];
        mixer.render(&mut output, 0);
        assert_eq!(output, [0.0; 3]);
    }
}
