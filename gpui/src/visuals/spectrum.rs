//! What mpv is playing, as 32 spectrum bands at 60 Hz.
//!
//! Samples come from a PipeWire tap on mpv's stream (`super::pipewire`): raw
//! f32 stereo at 48 kHz. A thread reads hops of 800 samples (1/60 s), runs a
//! 2048-point FFT over the newest 2048 and folds it into log-spaced bands.
//! While mpv is paused no samples come and the thread sleeps on the pipe.

use std::io::Read as _;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use super::pipewire::Tap;

pub const BANDS: usize = 32;
const RATE: f32 = 48_000.0;
const FFT: usize = 2048;
const HOP: usize = 800;
const LOW_HZ: f32 = 40.0;
const HIGH_HZ: f32 = 16_000.0;

/// The newest bands, shared with the UI.
#[derive(Clone, Default)]
pub struct Bands {
    /// 0..1 per band, smoothed (fast attack, slow release).
    pub levels: [f32; BANDS],
    /// Hops analysed so far.
    pub frames: u64,
    pub at: Option<Instant>,
    /// mpv nodes the tap is linked to, for the overlay.
    pub linked: usize,
}

pub struct SpectrumTap {
    pub bands: Arc<Mutex<Bands>>,
}

impl SpectrumTap {
    pub fn start() -> Self {
        let bands = Arc::new(Mutex::new(Bands::default()));
        let shared = bands.clone();
        let spawned = thread::Builder::new()
            .name("visuals-spectrum".into())
            .spawn(move || run(&shared));
        if let Err(e) = spawned {
            log::warn!("visuals: no spectrum thread: {e}");
        }
        Self { bands }
    }
}

fn run(bands: &Mutex<Bands>) {
    loop {
        match Tap::start() {
            Ok(mut tap) => analyse(&mut tap, bands),
            Err(e) => log::warn!("visuals: no PipeWire tap: {e:#}"),
        }
        // pw-record ended (PipeWire restarted?): start over.
        thread::sleep(Duration::from_secs(2));
    }
}

fn analyse(tap: &mut Tap, bands: &Mutex<Bands>) {
    let edges = band_edges();
    let window: Vec<f32> = (0..FFT)
        .map(|i| 0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / FFT as f32).cos())
        .collect();
    let mut history = vec![0.0f32; FFT];
    let mut bytes = vec![0u8; HOP * 2 * 4];
    let mut peak = 1e-3f32;
    let mut levels = [0.0f32; BANDS];
    while tap.stdout.read_exact(&mut bytes).is_ok() {
        history.drain(..HOP);
        // Frames of two f32 (left, right) to mono.
        history.extend(bytes.as_chunks::<8>().0.iter().map(|frame| {
            let (l, r) = frame.split_at(4);
            let sample = |b: &[u8]| f32::from_ne_bytes(b.try_into().expect("4 bytes"));
            (sample(l) + sample(r)) * 0.5
        }));
        let power = spectrum(&history, &window);
        let raw = fold(&power, &edges);
        // Automatic gain: mpv's volume is applied before PipeWire sees the
        // samples, so levels are relative to the recent loudest band.
        let loudest = raw.iter().copied().fold(0.0f32, f32::max);
        peak = (peak * 0.995).max(loudest).max(1e-4);
        for (level, value) in levels.iter_mut().zip(raw) {
            let db = 20.0 * (value / peak).max(1e-6).log10();
            let target = ((db + 48.0) / 48.0).clamp(0.0, 1.0);
            let speed = if target > *level { 0.6 } else { 0.12 };
            *level += (target - *level) * speed;
        }
        let mut shared = bands.lock().expect("bands");
        shared.levels = levels;
        shared.frames += 1;
        shared.at = Some(Instant::now());
        shared.linked = tap.linked.load(Ordering::Relaxed);
        if shared.frames % 120 == 1 {
            log::info!(
                "visuals: spectrum frame {}: {}",
                shared.frames,
                bars(&levels)
            );
        }
    }
}

/// The bands as a line of block characters, for the log.
pub fn bars(levels: &[f32; BANDS]) -> String {
    const STEPS: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
    levels
        .iter()
        .map(|l| STEPS[((l * 7.0).round() as usize).min(7)])
        .collect()
}

/// FFT bin ranges for log-spaced bands from 40 Hz to 16 kHz.
fn band_edges() -> Vec<(usize, usize)> {
    let bin = |hz: f32| ((hz / RATE * FFT as f32).round() as usize).clamp(1, FFT / 2);
    (0..BANDS)
        .map(|b| {
            let f = |i: usize| LOW_HZ * (HIGH_HZ / LOW_HZ).powf(i as f32 / BANDS as f32);
            let (lo, hi) = (bin(f(b)), bin(f(b + 1)));
            (lo, hi.max(lo + 1))
        })
        .collect()
}

/// RMS magnitude per band.
fn fold(power: &[f32], edges: &[(usize, usize)]) -> [f32; BANDS] {
    let mut out = [0.0; BANDS];
    for (value, &(lo, hi)) in out.iter_mut().zip(edges) {
        let sum: f32 = power[lo..hi].iter().sum();
        *value = (sum / (hi - lo) as f32).sqrt();
    }
    out
}

/// Power spectrum (|X|², first half) of the windowed samples.
fn spectrum(samples: &[f32], window: &[f32]) -> Vec<f32> {
    let mut re: Vec<f32> = samples.iter().zip(window).map(|(s, w)| s * w).collect();
    let mut im = vec![0.0f32; FFT];
    fft(&mut re, &mut im);
    re.iter()
        .zip(&im)
        .take(FFT / 2 + 1)
        .map(|(r, i)| (r * r + i * i) / FFT as f32)
        .collect()
}

/// In-place iterative radix-2 FFT. A spike stand-in for `realfft`.
fn fft(re: &mut [f32], im: &mut [f32]) {
    let n = re.len();
    let mut j = 0;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j |= bit;
        if i < j {
            re.swap(i, j);
            im.swap(i, j);
        }
    }
    let mut len = 2;
    while len <= n {
        let angle = -std::f32::consts::TAU / len as f32;
        for start in (0..n).step_by(len) {
            for k in 0..len / 2 {
                let (s, c) = (angle * k as f32).sin_cos();
                let (a, b) = (start + k, start + k + len / 2);
                let tr = re[b] * c - im[b] * s;
                let ti = re[b] * s + im[b] * c;
                re[b] = re[a] - tr;
                im[b] = im[a] - ti;
                re[a] += tr;
                im[a] += ti;
            }
        }
        len <<= 1;
    }
}
