//! One thread per track: HTTP source → symphonia demux → decode (AAC in
//! symphonia, Opus in libopus) → stereo → resample to the device rate →
//! the track's sample ring, which the mixer drains.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, TryRecvError};
use std::sync::{Arc, LazyLock, Mutex};
use std::thread;
use std::time::Duration;

use anyhow::{Context, Result, anyhow};
use reqwest::blocking::Client;
use reqwest::header::HeaderMap;
use rtrb::{Consumer, Producer, RingBuffer};
use symphonia::core::codecs::audio::{AudioDecoder, AudioDecoderOptions};
use symphonia::core::codecs::registry::CodecRegistry;
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, FormatReader, SeekMode, SeekTo, TrackType};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::units::{Time, TimeBase};
use symphonia_adapter_libopus::OpusDecoder;

use crate::http::{HttpSource, StatsHandle};
use crate::mixer::{Command, TrackShared};
use crate::padding;
use crate::resample::Resampler;

/// Seconds of output each track's ring holds.
const RING_SECS: u32 = 2;
/// Decode this far before a seek target and drop it: Opus needs ~80 ms to
/// converge after a reset, AAC one frame for its overlap.
const PREROLL: f64 = 0.08;

pub(crate) static CODECS: LazyLock<CodecRegistry> = LazyLock::new(|| {
    let mut registry = CodecRegistry::new();
    symphonia::default::register_enabled_codecs(&mut registry);
    registry.register_audio_decoder::<OpusDecoder>();
    registry
});

pub enum Control {
    Seek(f64),
    Stop,
}

pub struct Job {
    pub deck: usize,
    pub shared: Arc<TrackShared>,
    pub ring: Producer<f32>,
    pub done: Ending,
    pub url: String,
    pub headers: HeaderMap,
    pub start: f64,
    /// Start at this share of the track's length instead, once it is known.
    pub start_share: Option<f64>,
    pub rate: u32,
    pub client: Client,
    pub control: Receiver<Control>,
    pub mixer: Arc<Mutex<Producer<Command>>>,
    pub stats: Arc<Mutex<Option<StatsHandle>>>,
}

pub fn ring(rate: u32) -> (Producer<f32>, Consumer<f32>) {
    RingBuffer::new((rate * RING_SECS * 2) as usize)
}

/// Marks a ring's end for the mixer when dropped (end of stream, error, or
/// stop), unless a seek retired the ring first.
pub struct Ending(Option<Arc<AtomicBool>>);

impl Ending {
    pub fn new(flag: Arc<AtomicBool>) -> Self {
        Self(Some(flag))
    }

    fn retire(mut self) {
        self.0 = None;
    }
}

impl Drop for Ending {
    fn drop(&mut self) {
        if let Some(flag) = self.0.take() {
            flag.store(true, Ordering::Release);
        }
    }
}

/// Runs the job on its own thread; `Err` is reported by the caller.
pub fn spawn(job: Job, on_error: impl FnOnce(anyhow::Error) + Send + 'static) -> Result<()> {
    thread::Builder::new()
        .name(format!("audio-decode-{}", job.shared.id))
        .spawn(move || {
            // Dropping the job's `Ending` lets the mixer move on, whatever
            // happened.
            if let Err(error) = run(job) {
                on_error(error);
            }
        })
        .context("spawn audio decoder thread")?;
    Ok(())
}

struct Decoding {
    job: Job,
    format: Box<dyn FormatReader>,
    decoder: Box<dyn AudioDecoder>,
    track_id: u32,
    time_base: TimeBase,
    codec_rate: u32,
    resampler: Option<Resampler>,
    /// Drop decoded audio before this time (seconds), after a seek.
    skip_until: f64,
    samples: Vec<f32>,
    stereo: Vec<f32>,
    /// The last decoded packet, held back so the end padding can come off.
    held: Vec<f32>,
    resampled: Vec<f32>,
    matroska: bool,
}

enum Pushed {
    All,
    Interrupted(Control),
}

fn open_format(job: &Job) -> Result<Box<dyn FormatReader>> {
    let source = match local_path(&job.url) {
        Some(path) => HttpSource::local(path)?,
        None => HttpSource::open(&job.client, &job.url, job.headers.clone())?,
    };
    *job.stats.lock().unwrap_or_else(|e| e.into_inner()) = Some(source.stats_handle());
    let mss = MediaSourceStream::new(Box::new(source), Default::default());
    symphonia::default::get_probe()
        .probe(
            &hint(&job.url),
            mss,
            FormatOptions::default(),
            MetadataOptions::default(),
        )
        .context("unsupported container")
}

fn run(job: Job) -> Result<()> {
    let format = open_format(&job)?;
    let track = format
        .default_track(TrackType::Audio)
        .context("no audio track")?;
    let params = track
        .codec_params
        .as_ref()
        .and_then(|p| p.audio())
        .context("no audio codec parameters")?
        .clone();
    let track_id = track.id;
    let codec_rate = params
        .sample_rate
        .filter(|rate| *rate > 0)
        .context("no valid sample rate")?;
    let time_base = track
        .time_base
        .or_else(|| TimeBase::try_from_recip(codec_rate))
        .context("no time base")?;
    let duration = track
        .duration
        .and_then(|d| time_base.calc_duration(d))
        .map(|t| t.as_secs_f64())
        .or_else(|| track.num_frames.map(|n| n as f64 / f64::from(codec_rate)))
        .or_else(|| {
            // WebM gives its length for the whole segment only.
            let info = format.media_info();
            info.time_base?
                .calc_duration(info.duration?)
                .map(|t| t.as_secs_f64())
        })
        .filter(|d| *d > 0.0);
    if let Some(duration) = duration {
        job.shared
            .duration
            .store(duration.to_bits(), Ordering::Relaxed);
    }
    let decoder = CODECS
        .make_audio_decoder(&params, &AudioDecoderOptions::default())
        .context("unsupported codec")?;
    log::info!(
        "decode {}: {:?} at {} Hz, {} channels",
        job.shared.id,
        params.codec,
        codec_rate,
        params.channels.as_ref().map_or(0, |c| c.count())
    );
    let resampler = new_resampler(codec_rate, job.rate)?;
    let start = match (job.start_share, duration) {
        (Some(share), Some(duration)) => share.clamp(0.0, 1.0) * duration,
        _ => job.start,
    };
    let matroska = format.format_info().short_name == "matroska";
    let mut decoding = Decoding {
        job,
        format,
        decoder,
        track_id,
        time_base,
        codec_rate,
        resampler,
        skip_until: 0.0,
        samples: Vec::new(),
        stereo: Vec::new(),
        held: Vec::new(),
        resampled: Vec::new(),
        matroska,
    };
    if start > 0.0 {
        decoding.seek(start, false)?;
    }
    decoding.run()
}

fn new_resampler(from: u32, to: u32) -> Result<Option<Resampler>> {
    Ok(if from == to {
        None
    } else {
        Some(Resampler::new(from, to)?)
    })
}

impl Decoding {
    fn run(&mut self) -> Result<()> {
        loop {
            match self.job.control.try_recv() {
                Ok(control) => {
                    if self.handle(control)? {
                        return Ok(());
                    }
                    continue;
                }
                Err(TryRecvError::Disconnected) => return Ok(()),
                Err(TryRecvError::Empty) => {}
            }
            let packet = match self.format.next_packet() {
                Ok(Some(packet)) => packet,
                Ok(None) => return self.finish(),
                Err(SymphoniaError::IoError(e))
                    if e.kind() == std::io::ErrorKind::UnexpectedEof =>
                {
                    return self.finish();
                }
                Err(e) => return Err(anyhow!(e).context("demux")),
            };
            if packet.track_id != self.track_id {
                continue;
            }
            let pts = self.time_base.calc_time_saturating(packet.pts);
            let decoded = match self.decoder.decode(&packet) {
                Ok(decoded) => decoded,
                Err(SymphoniaError::DecodeError(e)) => {
                    log::warn!("decode {}: skipped a packet: {e}", self.job.shared.id);
                    continue;
                }
                Err(e) => return Err(anyhow!(e).context("decode")),
            };
            let channels = decoded.spec().channels().count().max(1);
            decoded.copy_to_vec_interleaved::<f32>(&mut self.samples);
            to_stereo(&self.samples, channels, &mut self.stereo);
            let target = self.skip_until;
            if let Some(start) =
                trim_to_target(&mut self.stereo, self.codec_rate, &mut self.skip_until, pts)
            {
                log::info!(
                    "decode {}: seek to {target:.3}s plays from {start:.3}s",
                    self.job.shared.id
                );
            }
            if let Pushed::Interrupted(control) = self.emit()?
                && self.handle(control)?
            {
                return Ok(());
            }
        }
    }

    /// Returns true when the track should stop.
    fn handle(&mut self, control: Control) -> Result<bool> {
        match control {
            Control::Stop => Ok(true),
            Control::Seek(to) => {
                self.seek(to, true)?;
                Ok(false)
            }
        }
    }

    fn seek(&mut self, to: f64, replace: bool) -> Result<()> {
        let actual = seek_anchor(&mut self.format, self.track_id, self.time_base, to, || {
            open_format(&self.job)
        })?;
        log::info!(
            "decode {}: seek to {to:.3}s anchored at {actual:.3}s",
            self.job.shared.id
        );
        self.decoder.reset();
        self.resampler = new_resampler(self.codec_rate, self.job.rate)?;
        self.skip_until = to;
        self.held.clear();
        let base = (to * self.job.rate as f64).round() as u64;
        if replace {
            let (producer, consumer) = ring(self.job.rate);
            let done = Arc::new(AtomicBool::new(false));
            self.job.ring = producer;
            std::mem::replace(&mut self.job.done, Ending::new(done.clone())).retire();
            let command = Command::Replace {
                deck: self.job.deck,
                track: self.job.shared.id,
                ring: consumer,
                done,
                base,
            };
            send(&self.job.mixer, command);
        } else {
            self.job.shared.base.store(base, Ordering::Relaxed);
        }
        Ok(())
    }

    /// Holds the new packet back and sends the one before it.
    fn emit(&mut self) -> Result<Pushed> {
        std::mem::swap(&mut self.held, &mut self.stereo);
        let ready = std::mem::take(&mut self.stereo);
        let result = self.send(&ready);
        self.stereo = ready;
        result
    }

    /// Resamples interleaved stereo and pushes it to the ring.
    fn send(&mut self, stereo: &[f32]) -> Result<Pushed> {
        Ok(match self.resampler.as_mut() {
            None => push(&mut self.job.ring, stereo, &self.job.control),
            Some(resampler) => {
                let out = &mut self.resampled;
                out.clear();
                resampler.push(stereo, &mut |s| out.extend_from_slice(s))?;
                push(&mut self.job.ring, out, &self.job.control)
            }
        })
    }

    /// Frames of end padding to drop from the last packet (WebM Opus).
    fn end_padding(&self) -> usize {
        if !self.matroska {
            return 0;
        }
        let stats = self.job.stats.lock().unwrap_or_else(|e| e.into_inner());
        let tail = stats.as_ref().map(|s| s.tail(64 << 10)).unwrap_or_default();
        let Some(ns) = padding::discard_padding(&tail) else {
            return 0;
        };
        let frames = (ns as f64 * self.codec_rate as f64 / 1e9).round() as usize;
        log::info!(
            "decode {}: dropping {frames} frames of end padding ({:.1} ms)",
            self.job.shared.id,
            ns as f64 / 1e6
        );
        frames
    }

    /// Flushes the resampler, marks the ring's end, then waits: a seek in
    /// the last seconds (still in the ring) decodes from there again.
    fn finish(&mut self) -> Result<()> {
        let mut pending = None;
        let mut last = std::mem::take(&mut self.held);
        let padding = self.end_padding().min(last.len() / 2);
        last.truncate(last.len() - padding * 2);
        if let Pushed::Interrupted(control) = self.send(&last)? {
            pending = Some(control);
        }
        if let Some(resampler) = self.resampler.as_mut().filter(|_| pending.is_none()) {
            let mut out = Vec::new();
            resampler.finish(&mut |s| out.extend_from_slice(s))?;
            if let Pushed::Interrupted(control) = push(&mut self.job.ring, &out, &self.job.control)
            {
                pending = Some(control);
            }
        }
        if pending.is_none() {
            log::info!("decode {}: end of stream", self.job.shared.id);
            // Dropping the ring's `Ending` marks it.
            drop(std::mem::replace(&mut self.job.done, Ending(None)));
            pending = self.job.control.recv().ok();
        }
        match pending {
            Some(control) => {
                if self.handle(control)? {
                    Ok(())
                } else {
                    self.run()
                }
            }
            None => Ok(()),
        }
    }
}

/// A seek result may point at the following WebM cue even in Accurate mode.
/// Never start decoding after the requested time: back off to an earlier cue
/// and use `trim_to_target` to trim the decoded PCM. At the beginning,
/// reopen without seeking because the first cue can itself be several seconds
/// into the stream. Bound cue attempts before falling back to a fresh reader.
fn seek_anchor(
    format: &mut Box<dyn FormatReader>,
    track_id: u32,
    time_base: TimeBase,
    to: f64,
    mut reopen: impl FnMut() -> Result<Box<dyn FormatReader>>,
) -> Result<f64> {
    let latest = (to - PREROLL).max(0.0);
    let mut from = latest;
    for _ in 0..8 {
        if from <= 0.0 {
            break;
        }
        *format = reopen()?;
        let seeked = format
            .seek(
                SeekMode::Accurate,
                SeekTo::Time {
                    time: Time::try_from_secs_f64(from).context("seek time")?,
                    track_id: Some(track_id),
                },
            )
            .context("seek")?;
        let actual = time_base
            .calc_time_saturating(seeked.actual_ts)
            .as_secs_f64();
        if actual <= latest {
            return Ok(actual);
        }
        from = (from - (actual - from).max(PREROLL) - PREROLL).max(0.0);
    }
    *format = reopen()?;
    Ok(0.0)
}

/// Drops the decoded audio before a seek's `target`. Returns where the kept
/// audio starts once the target is reached (`None` while still before it,
/// or when no seek is pending).
fn trim_to_target(stereo: &mut Vec<f32>, rate: u32, target: &mut f64, pts: Time) -> Option<f64> {
    if *target <= 0.0 {
        return None;
    }
    let late = *target - pts.as_secs_f64();
    let frames = (stereo.len() / 2).min((late * f64::from(rate)).max(0.0) as usize);
    stereo.drain(..frames * 2);
    if stereo.is_empty() && late > 0.0 {
        return None;
    }
    *target = 0.0;
    Some(pts.as_secs_f64() + frames as f64 / f64::from(rate))
}

/// Pushes all of `samples`, waiting while the ring is full; a control
/// message interrupts the wait.
fn push(ring: &mut Producer<f32>, samples: &[f32], control: &Receiver<Control>) -> Pushed {
    let mut rest = samples;
    while !rest.is_empty() {
        let n = ring.slots().min(rest.len()) & !1;
        if n == 0 {
            match control.try_recv() {
                Ok(c) => return Pushed::Interrupted(c),
                Err(TryRecvError::Disconnected) => return Pushed::Interrupted(Control::Stop),
                Err(TryRecvError::Empty) => thread::sleep(Duration::from_millis(10)),
            }
            continue;
        }
        if let Ok(mut chunk) = ring.write_chunk(n) {
            let (a, b) = chunk.as_mut_slices();
            let split = a.len();
            a.copy_from_slice(&rest[..split]);
            b.copy_from_slice(&rest[split..n]);
            chunk.commit_all();
        }
        rest = &rest[n..];
    }
    Pushed::All
}

pub fn send(mixer: &Mutex<Producer<Command>>, command: Command) {
    let mut producer = mixer.lock().unwrap_or_else(|e| e.into_inner());
    if producer.push(command).is_err() {
        log::error!("audio: mixer command queue full");
    }
}

fn to_stereo(samples: &[f32], channels: usize, out: &mut Vec<f32>) {
    out.clear();
    match channels {
        1 => out.extend(samples.iter().flat_map(|s| [*s, *s])),
        2 => out.extend_from_slice(samples),
        n => out.extend(samples.chunks_exact(n).flat_map(|f| [f[0], f[1]])),
    }
}

/// A local file (a path or a `file://` URL) instead of an HTTP stream.
pub(crate) fn local_path(url: &str) -> Option<&str> {
    if let Some(path) = url.strip_prefix("file://") {
        return Some(path);
    }
    (!url.starts_with("http://") && !url.starts_with("https://")).then_some(url)
}

pub(crate) fn hint(url: &str) -> Hint {
    let mut hint = Hint::new();
    let path = url.split(['?', '#']).next().unwrap_or(url);
    let mime = url
        .split(['?', '&'])
        .find_map(|p| p.strip_prefix("mime="))
        .map(|m| m.replace("%2F", "/"));
    if let Some(mime) = &mime {
        hint.mime_type(mime);
    } else if let Some(ext) = path.rsplit('.').next().filter(|e| e.len() <= 4) {
        hint.with_extension(ext);
    }
    hint
}

#[cfg(test)]
#[allow(clippy::unwrap_used, reason = "offline fixture assertions")]
mod seek_tests {
    use super::*;

    fn fixture() -> Result<Box<dyn FormatReader>> {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/cast-seek.webm");
        let file = std::fs::File::open(path)?;
        symphonia::default::get_probe()
            .probe(
                &hint(path),
                MediaSourceStream::new(Box::new(file), Default::default()),
                FormatOptions::default(),
                MetadataOptions::default(),
            )
            .context("fixture")
    }

    #[test]
    fn cast_handoffs_decode_from_before_the_target_not_the_following_webm_cue() {
        for target in [0.5, 6.76, 50.76] {
            let mut format = fixture().unwrap();
            let track = format.default_track(TrackType::Audio).unwrap();
            let (id, base) = (track.id, track.time_base.unwrap());
            let actual = seek_anchor(&mut format, id, base, target, fixture).unwrap();
            assert!(
                actual <= (target - PREROLL).max(0.0),
                "anchor {actual} skips target {target}"
            );
            let packet = format.next_packet().unwrap().unwrap();
            let first = base.calc_time_saturating(packet.pts).as_secs_f64();
            assert!(
                first <= target,
                "first packet {first} skips requested audio at {target}"
            );
        }
    }

    #[test]
    fn handoff_pcm_contains_the_requested_time_not_just_the_requested_clock() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/cast-seek-chirp.webm"
        );
        let fresh = || {
            symphonia::default::get_probe()
                .probe(
                    &hint(path),
                    MediaSourceStream::new(
                        Box::new(std::fs::File::open(path)?),
                        Default::default(),
                    ),
                    FormatOptions::default(),
                    MetadataOptions::default(),
                )
                .context("chirp fixture")
        };
        let mut format = fresh().unwrap();
        let track = format.default_track(TrackType::Audio).unwrap();
        let (id, base) = (track.id, track.time_base.unwrap());
        let params = track
            .codec_params
            .as_ref()
            .unwrap()
            .audio()
            .unwrap()
            .clone();
        let rate = params.sample_rate.unwrap();
        let mut decoder = CODECS
            .make_audio_decoder(&params, &AudioDecoderOptions::default())
            .unwrap();
        let target = 50.76;
        seek_anchor(&mut format, id, base, target, fresh).unwrap();
        let mut skip = target;
        let (mut samples, mut stereo, mut mono) = (Vec::new(), Vec::new(), Vec::new());
        while mono.len() < (rate / 5) as usize {
            let packet = format.next_packet().unwrap().unwrap();
            if packet.track_id != id {
                continue;
            }
            let decoded = decoder.decode(&packet).unwrap();
            let channels = decoded.spec().channels().count();
            decoded.copy_to_vec_interleaved::<f32>(&mut samples);
            to_stereo(&samples, channels, &mut stereo);
            trim_to_target(
                &mut stereo,
                rate,
                &mut skip,
                base.calc_time_saturating(packet.pts),
            );
            mono.extend(
                stereo
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .map(|f| (f[0] + f[1]) * 0.5),
            );
        }
        let crossings: Vec<f64> = mono
            .windows(2)
            .enumerate()
            .filter(|(_, w)| w[0] <= 0.0 && w[1] > 0.0)
            .map(|(i, w)| i as f64 + f64::from(-w[0] / (w[1] - w[0])))
            .collect();
        let frequency = (crossings.len() - 1) as f64 * f64::from(rate)
            / (crossings.last().unwrap() - crossings[0]);
        let middle = target + mono.len() as f64 / f64::from(rate) * 0.5;
        let expected = 200.0 + 20.0 * middle;
        assert!(
            (frequency - expected).abs() < 3.0,
            "PCM frequency {frequency} instead of {expected} means the content is not at {target}s"
        );
    }
}
