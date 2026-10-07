//! What the engine plays, as spectrum bands at 60 Hz.
//!
//! Samples come from the audio engine in this process (`ytfast_audio::Tap`,
//! on every OS): the mono mix as it goes to the device, at the device's
//! rate. A thread takes a hop of 1/60 s of samples each frame, runs a
//! 4096-point FFT over the newest 4096 and folds it into log-spaced bands,
//! plus a bass level and a beat pulse ("kick") for the backdrop. The output
//! callback hands over a period (~43 ms) at a time; taking one hop per
//! frame spreads it evenly and keeps the bands about a period behind, near
//! what the device is playing.
//!
//! Dropping the [`AudioTap`] ends the thread, and with no tap open the
//! engine skips the copy.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use ytfast_audio::Tap;

pub const BANDS: usize = 32;
/// Frames per second the bands move at.
const HOPS_PER_SEC: u32 = 60;
const FFT: usize = 4096;
const LOW_HZ: f32 = 50.0;
const HIGH_HZ: f32 = 16_000.0;
/// Levels count as current for this long after the last hop; after that
/// (no engine yet) [`AudioTap::bands`] reports silence.
const STALE: Duration = Duration::from_millis(150);
/// Waiting samples beyond this many seconds are dropped (the thread was
/// held up), so the bands don't lag behind the music.
const MAX_LAG: f32 = 0.2;

/// The newest analysis.
#[derive(Clone, Copy, Debug, Default)]
pub struct Bands {
    /// 0..1 per band, low to high, smoothed (fast attack, slow release).
    pub levels: [f32; BANDS],
    /// The lowest bands' level, 0..1.
    pub bass: f32,
    /// A pulse on each bass onset that decays in ~0.25 s, 0..1.
    pub kick: f32,
    /// The mean of all bands, 0..1.
    pub level: f32,
    /// 1 while the engine's output runs and the tap hears it, else 0.
    pub linked: usize,
}

#[derive(Default)]
struct Shared {
    stop: AtomicBool,
    bands: Mutex<(Bands, Option<Instant>)>,
    linked: AtomicUsize,
}

/// A running tap; [`Self::bands`] reads the newest levels.
pub struct AudioTap {
    shared: Arc<Shared>,
}

impl AudioTap {
    /// Starts analysing what the engine plays on a background thread.
    pub fn start() -> Self {
        let shared = Arc::new(Shared::default());
        let thread_shared = shared.clone();
        let spawned = thread::Builder::new()
            .name("visuals-spectrum".into())
            .spawn(move || run(&thread_shared));
        if let Err(e) = spawned {
            log::warn!("visuals: no spectrum thread: {e}");
        }
        Self { shared }
    }

    /// The newest bands, or silence when nothing came for a moment.
    pub fn bands(&self) -> Bands {
        let (mut bands, at) = *self.shared.bands.lock().expect("bands");
        bands.linked = self.shared.linked.load(Ordering::Relaxed);
        if at.is_none_or(|at| at.elapsed() > STALE) {
            return Bands {
                linked: bands.linked,
                ..Bands::default()
            };
        }
        bands
    }
}

impl Drop for AudioTap {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::Relaxed);
    }
}

/// One hop per frame from the engine's tap until the [`AudioTap`] goes.
fn run(shared: &Shared) {
    let mut tap = Tap::open();
    let mut analyser: Option<Analyser> = None;
    let mut hop = Vec::new();
    let frame = Duration::from_secs(1) / HOPS_PER_SEC;
    let mut next = Instant::now();
    while !shared.stop.load(Ordering::Relaxed) {
        next += frame;
        let now = Instant::now();
        if next > now {
            thread::sleep(next - now);
        } else {
            next = now;
        }
        let rate = tap.rate();
        shared
            .linked
            .store(usize::from(rate > 0), Ordering::Relaxed);
        if rate == 0 {
            continue;
        }
        let analyser = match &mut analyser {
            Some(a) if a.rate == rate => a,
            slot => slot.insert(Analyser::new(rate)),
        };
        let size = (rate / HOPS_PER_SEC) as usize;
        tap.skip_to((rate as f32 * MAX_LAG) as usize);
        if tap.available() < size {
            continue;
        }
        hop.clear();
        tap.read(size, &mut hop);
        let bands = analyser.hop(hop.iter().copied());
        *shared.bands.lock().expect("bands") = (bands, Some(Instant::now()));
        if analyser.hops % 600 == 1 {
            log::info!(
                "visuals: spectrum hop {}: {}",
                analyser.hops,
                bars(&bands.levels)
            );
        }
    }
}

/// The FFT state between hops.
struct Analyser {
    rate: u32,
    edges: Vec<(usize, usize)>,
    window: Vec<f32>,
    /// The FFT's twiddle factors ([`twiddles`]): worked out once rather
    /// than in every butterfly of every hop.
    twiddles: Vec<(f32, f32)>,
    history: Vec<f32>,
    peak: f32,
    levels: [f32; BANDS],
    bass_slow: f32,
    kick: f32,
    hops: u64,
}

impl Analyser {
    fn new(rate: u32) -> Self {
        Self {
            rate,
            edges: band_edges(rate as f32),
            window: (0..FFT)
                .map(|i| 0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / FFT as f32).cos())
                .collect(),
            twiddles: twiddles(FFT),
            history: vec![0.0; FFT],
            peak: 1e-3,
            levels: [0.0; BANDS],
            bass_slow: 0.0,
            kick: 0.0,
            hops: 0,
        }
    }

    /// Takes the next hop of samples and returns the new bands.
    fn hop(&mut self, samples: impl ExactSizeIterator<Item = f32>) -> Bands {
        self.history.drain(..samples.len().min(FFT));
        self.history.extend(samples);
        let excess = self.history.len().saturating_sub(FFT);
        self.history.drain(..excess);
        self.hops += 1;
        let power = spectrum(&self.history, &self.window, &self.twiddles);
        let raw = fold(&power, &self.edges);
        // Automatic gain: the tap hears the mix after the volume, so levels
        // are relative to the recent loudest band.
        let loudest = raw.iter().copied().fold(0.0f32, f32::max);
        self.peak = (self.peak * 0.995).max(loudest).max(1e-4);
        for (level, value) in self.levels.iter_mut().zip(raw) {
            let db = 20.0 * (value / self.peak).max(1e-6).log10();
            let target = ((db + 48.0) / 48.0).clamp(0.0, 1.0);
            let speed = if target > *level { 0.6 } else { 0.12 };
            *level += (target - *level) * speed;
        }
        let bass = self.levels[..4].iter().sum::<f32>() / 4.0;
        // A kick is the bass rising above its recent average.
        let onset = ((bass - self.bass_slow) * 4.0).clamp(0.0, 1.0);
        self.bass_slow += (bass - self.bass_slow) * 0.15;
        self.kick = (self.kick * 0.88).max(onset);
        Bands {
            levels: self.levels,
            bass,
            kick: self.kick,
            level: self.levels.iter().sum::<f32>() / BANDS as f32,
            linked: 0,
        }
    }
}

/// The bands as a line of block characters, for the log.
fn bars(levels: &[f32; BANDS]) -> String {
    const STEPS: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
    levels
        .iter()
        .map(|l| STEPS[((l * 7.0).round() as usize).min(7)])
        .collect()
}

/// FFT bin ranges for log-spaced bands from `LOW_HZ` to `HIGH_HZ`.
fn band_edges(rate: f32) -> Vec<(usize, usize)> {
    let bin = |hz: f32| ((hz / rate * FFT as f32).round() as usize).clamp(1, FFT / 2);
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
fn spectrum(samples: &[f32], window: &[f32], twiddles: &[(f32, f32)]) -> Vec<f32> {
    let mut re: Vec<f32> = samples.iter().zip(window).map(|(s, w)| s * w).collect();
    let mut im = vec![0.0f32; FFT];
    fft(&mut re, &mut im, twiddles);
    re.iter()
        .zip(&im)
        .take(FFT / 2 + 1)
        .map(|(r, i)| (r * r + i * i) / FFT as f32)
        .collect()
}

/// The twiddle factors for an `n`-point FFT: for each stage (`len` 2, 4,
/// … `n`), the (sin, cos) of -2πk/len for k < len/2, one stage after the
/// other (so a stage's start at `len/2 - 1`).
fn twiddles(n: usize) -> Vec<(f32, f32)> {
    let mut out = Vec::with_capacity(n);
    let mut len = 2;
    while len <= n {
        let angle = -std::f32::consts::TAU / len as f32;
        out.extend((0..len / 2).map(|k| (angle * k as f32).sin_cos()));
        len <<= 1;
    }
    out
}

/// In-place iterative radix-2 FFT; `twiddles` from [`twiddles`] for
/// `re.len()`.
fn fft(re: &mut [f32], im: &mut [f32], twiddles: &[(f32, f32)]) {
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
        let half = len / 2;
        let stage = &twiddles[half - 1..len - 1];
        for (re, im) in re.chunks_exact_mut(len).zip(im.chunks_exact_mut(len)) {
            let (re_a, re_b) = re.split_at_mut(half);
            let (im_a, im_b) = im.split_at_mut(half);
            let pairs = re_a.iter_mut().zip(im_a).zip(re_b.iter_mut().zip(im_b));
            for (((ra, ia), (rb, ib)), &(s, c)) in pairs.zip(stage) {
                let tr = *rb * c - *ib * s;
                let ti = *rb * s + *ib * c;
                *rb = *ra - tr;
                *ib = *ia - ti;
                *ra += tr;
                *ia += ti;
            }
        }
        len <<= 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: f32 = 48_000.0;
    const HOP: usize = 800;

    fn tone(hz: f32, hop: u64) -> impl ExactSizeIterator<Item = f32> {
        (0..HOP).map(move |i| {
            let t = (hop as usize * HOP + i) as f32 / RATE;
            (std::f32::consts::TAU * hz * t).sin() * 0.5
        })
    }

    /// A 1 kHz tone lights the band that holds 1 kHz the most, and leaves
    /// the bass (and so the kick) quiet.
    #[test]
    fn a_tone_lands_in_its_band() {
        let mut analyser = Analyser::new(RATE as u32);
        let mut bands = Bands::default();
        for hop in 0..12 {
            bands = analyser.hop(tone(1000.0, hop));
        }
        let loudest = (0..BANDS)
            .max_by(|&a, &b| bands.levels[a].total_cmp(&bands.levels[b]))
            .expect("bands");
        let (lo, hi) = band_edges(RATE)[loudest];
        let (lo_hz, hi_hz) = (lo as f32 * RATE / FFT as f32, hi as f32 * RATE / FFT as f32);
        assert!(lo_hz <= 1000.0 && 1000.0 <= hi_hz, "{lo_hz}..{hi_hz} Hz");
        assert!(bands.bass < 0.3, "bass {}", bands.bass);
    }

    /// Bass bursts after silence give a kick that then decays.
    #[test]
    fn a_bass_onset_kicks() {
        let mut analyser = Analyser::new(RATE as u32);
        for hop in 0..10 {
            analyser.hop(tone(1000.0, hop).map(|s| s * 0.01));
        }
        let hit = (10..14).map(|hop| analyser.hop(tone(60.0, hop)).kick);
        let peak = hit.fold(0.0f32, f32::max);
        assert!(peak > 0.5, "kick {peak}");
        let later = (14..60)
            .map(|hop| analyser.hop(tone(60.0, hop)).kick)
            .last();
        assert!(later.expect("hops") < 0.2);
    }
}
