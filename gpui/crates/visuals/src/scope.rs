//! The oscilloscope (M22): the visualiser's Scope style, drawn from the
//! samples the engine plays rather than from the spectrum.
//!
//! [`Scope::update`] takes the newest stereo frames ([`AudioTap::recent`])
//! and turns them into the points the shader draws:
//!
//! - **Mono** and **Stereo** (left over right): ~30 ms of the mix, started
//!   on a rising zero crossing of a low-passed copy (with hysteresis, so
//!   noise doesn't trigger). Of the crossings in the last ~40 ms the one
//!   whose window looks most like the trace on screen wins, so a steady
//!   note stands still instead of hopping between its periods; each frame
//!   then eases a quarter of the way back towards the last one.
//! - **X/Y**: the newest ~20 ms as a goniometer (mid up, side across), so
//!   mono is a vertical line and stereo width spreads it sideways. Its
//!   points are not eased: two figures averaged are neither.
//!
//! Levels follow the mix with an automatic gain (the tap hears it after the
//! volume), times the sensitivity, softly limited so loud passages don't
//! hit the edges.
//!
//! [`AudioTap::recent`]: crate::AudioTap::recent

/// The most points per trace the shader holds (the shader's `wave` array
/// holds 1024 values: one trace, two, or X/Y pairs).
pub const MAX_POINTS: usize = 512;
/// Points in the X/Y figure (pairs, so 512 values).
pub const XY_POINTS: usize = 256;
/// Seconds of sound across a trace.
const WINDOW: f32 = 0.03;
/// How far back a trigger is looked for, in seconds: a period of 25 Hz.
const SEARCH: f32 = 0.04;
/// Seconds of sound in the X/Y figure.
const XY_WINDOW: f32 = 0.02;
/// The trigger's low-pass, in Hz: crossings of the fundamental, not of
/// the hiss on top of it.
const TRIGGER_HZ: f32 = 400.0;
/// Of the last trace, how much each frame at 20 fps keeps.
const EASE: f32 = 0.25;
/// The gain stays under this many times (silence isn't blown up).
const MOST_GAIN: f32 = 40.0;
/// Where the loudest recent sample lands, of the half height.
const HEADROOM: f32 = 0.85;

/// How the scope shows the channels.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Channels {
    /// One trace of the mix.
    #[default]
    Mono,
    /// The left channel over the right.
    Stereo,
    /// Mid against side.
    XY,
}

impl Channels {
    /// The shader's number for it.
    pub fn index(self) -> u32 {
        self as u32
    }
}

/// The seconds of sound [`Scope::update`] needs: a trace, the room to
/// look back for its trigger and the low-pass's run-in.
pub const SPAN: f32 = WINDOW + SEARCH + 0.002;

/// [`SPAN`] in frames at `rate`.
#[cfg(test)]
pub(crate) fn frames_needed(rate: u32) -> usize {
    (SPAN * rate as f32).ceil() as usize
}

/// The traces on screen and what the next frame needs of them.
#[derive(Clone, Debug, Default)]
pub struct Scope {
    /// What the shader draws: one trace of `points`, two (left then
    /// right), or `XY_POINTS` x/y pairs.
    pub values: Vec<f32>,
    pub points: usize,
    pub channels: Channels,
    /// The loudest recent sample, for the gain.
    peak: f32,
    /// The mono trace of the last frame, before easing, for the trigger.
    last: Vec<f32>,
    low: Vec<f32>,
}

impl Scope {
    /// Moves to the newest `frames` (oldest first) at `rate`, `dt` seconds
    /// after the last call; `points` per trace (clamped to
    /// [`MAX_POINTS`]).
    pub fn update(
        &mut self,
        frames: &[[f32; 2]],
        rate: u32,
        channels: Channels,
        sensitivity: f32,
        points: usize,
        dt: f32,
    ) {
        let points = points.clamp(16, MAX_POINTS);
        let window = (WINDOW * rate as f32) as usize;
        if self.channels != channels || self.points != points {
            *self = Self {
                channels,
                points,
                peak: self.peak,
                ..Self::default()
            };
        }
        let shown = match channels {
            Channels::XY => (XY_WINDOW * rate as f32) as usize,
            _ => window,
        };
        if rate == 0 || frames.len() < shown.max(2) {
            self.fade(dt);
            return;
        }
        self.follow(&frames[frames.len() - shown..], dt);
        let gain = sensitivity * HEADROOM / self.peak.max(1.0 / MOST_GAIN);
        let values = match channels {
            Channels::XY => figure(&frames[frames.len() - shown..], gain),
            _ => {
                let start = self.trigger(frames, rate, window, points, gain);
                let span = &frames[start..start + window];
                let mono = trace(span, points, gain, |[l, r]| (l + r) * 0.5);
                let mut values = if channels == Channels::Stereo {
                    let mut both = trace(span, points, gain, |[l, _]| l);
                    both.extend(trace(span, points, gain, |[_, r]| r));
                    both
                } else {
                    mono.clone()
                };
                self.last = mono;
                if self.values.len() == values.len() {
                    let keep = EASE.powf(dt * 20.0);
                    for (v, old) in values.iter_mut().zip(&self.values) {
                        *v += (old - *v) * keep;
                    }
                }
                values
            }
        };
        self.values = values;
    }

    /// Everything has settled to a flat line (or a dot).
    pub fn quiet(&self) -> bool {
        self.values.iter().all(|v| v.abs() < 0.004)
    }

    /// No sound: the traces ease down to rest.
    fn fade(&mut self, dt: f32) {
        let keep = EASE.powf(dt * 20.0);
        for v in &mut self.values {
            *v *= keep;
        }
        self.last.clear();
    }

    /// The gain follows the loudest sample: up within a few frames, down
    /// over a second or two.
    fn follow(&mut self, frames: &[[f32; 2]], dt: f32) {
        let loudest = frames.iter().flatten().fold(0.0f32, |m, s| m.max(s.abs()));
        let speed = if loudest > self.peak {
            1.0 - 0.3f32.powf(dt * 20.0)
        } else {
            1.0 - 0.5f32.powf(dt / 0.8)
        };
        self.peak += (loudest - self.peak) * speed;
    }

    /// Where the trace starts in `frames`: on a rising zero crossing, the
    /// one most like the last trace (newer ones break ties), or the newest
    /// window when there is none.
    fn trigger(
        &mut self,
        frames: &[[f32; 2]],
        rate: u32,
        window: usize,
        points: usize,
        gain: f32,
    ) -> usize {
        let newest = frames.len() - window;
        let search = ((SEARCH * rate as f32) as usize).min(newest);
        let from = newest - search;
        // The low-pass runs in from a little earlier so it has settled.
        let lead = from.min(64);
        let a = 1.0 - (-std::f32::consts::TAU * TRIGGER_HZ / rate as f32).exp();
        self.low.clear();
        let mut y = 0.0;
        for [l, r] in &frames[from - lead..=newest] {
            y += ((l + r) * 0.5 - y) * a;
            self.low.push(y);
        }
        let low = &self.low[lead..];
        // The low-pass lags the sound by about this many frames; the trace
        // starts where the sound itself crosses.
        let lag = (rate as f32 / (std::f32::consts::TAU * TRIGGER_HZ)) as usize;
        let hysteresis = 0.08 * self.peak;
        let mut armed = false;
        let mut crossings = Vec::new();
        for (i, pair) in low.windows(2).enumerate() {
            if pair[1] < -hysteresis {
                armed = true;
            }
            if armed && pair[0] < 0.0 && pair[1] >= 0.0 {
                crossings.push((from + i + 1).saturating_sub(lag));
                armed = false;
            }
        }
        let Some(&latest) = crossings.last() else {
            return newest;
        };
        if self.last.len() != points {
            return latest;
        }
        let energy = self.last.iter().map(|v| v * v).sum::<f32>() / points as f32;
        let probe = |start: usize| {
            let step = points / 64;
            (0..points)
                .step_by(step.max(1))
                .map(|k| {
                    let at = start + (k * window + window / (2 * points)) / points;
                    let [l, r] = frames[at.min(frames.len() - 1)];
                    let d = soft((l + r) * 0.5 * gain) - self.last[k];
                    d * d
                })
                .sum::<f32>()
                / (points / step.max(1)) as f32
        };
        crossings
            .iter()
            .map(|&at| {
                // Older crossings cost a little, so a tie goes to the newest.
                let age = (newest - at) as f32 / search.max(1) as f32;
                (at, probe(at) + 0.1 * energy * age)
            })
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map_or(latest, |(at, _)| at)
    }
}

/// `span` averaged down to `points` values of `pick` (each the mean of its
/// slice: a gentle low-pass), with the gain and the soft limit.
fn trace(span: &[[f32; 2]], points: usize, gain: f32, pick: impl Fn([f32; 2]) -> f32) -> Vec<f32> {
    (0..points)
        .map(|k| {
            let lo = k * span.len() / points;
            let hi = ((k + 1) * span.len() / points).max(lo + 1);
            let sum: f32 = span[lo..hi].iter().map(|&f| pick(f)).sum();
            soft(sum / (hi - lo) as f32 * gain)
        })
        .collect()
}

/// The X/Y figure: mid (up) against side (across), as x/y pairs.
fn figure(span: &[[f32; 2]], gain: f32) -> Vec<f32> {
    let mut out = Vec::with_capacity(XY_POINTS * 2);
    for k in 0..XY_POINTS {
        let lo = k * span.len() / XY_POINTS;
        let hi = ((k + 1) * span.len() / XY_POINTS).max(lo + 1);
        let (mut mid, mut side) = (0.0, 0.0);
        for [l, r] in &span[lo..hi] {
            mid += (l + r) * 0.5;
            side += (r - l) * 0.5;
        }
        let n = (hi - lo) as f32;
        out.push(soft(side / n * gain));
        out.push(soft(mid / n * gain));
    }
    out
}

/// A soft limit to ±1 that leaves quiet values as they are.
fn soft(v: f32) -> f32 {
    v.tanh()
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: u32 = 48_000;

    fn tone(hz: f32, phase: f32, n: usize) -> Vec<[f32; 2]> {
        (0..n)
            .map(|i| {
                let t = i as f32 / RATE as f32;
                let s = (std::f32::consts::TAU * hz * t + phase).sin() * 0.4;
                [s, s]
            })
            .collect()
    }

    /// A steady tone heard at any moment gives the same trace: it starts
    /// on a rising zero crossing, whatever the phase the newest sample
    /// happens to be at.
    #[test]
    fn a_tone_stands_still() {
        let n = frames_needed(RATE);
        let mut scope = Scope::default();
        for _ in 0..10 {
            scope.update(&tone(110.0, 0.0, n), RATE, Channels::Mono, 1.0, 256, 0.05);
        }
        let mut traces = Vec::new();
        for phase in [0.0, 1.1, 2.3, 4.0, 5.5] {
            scope.update(&tone(110.0, phase, n), RATE, Channels::Mono, 1.0, 256, 0.05);
            traces.push(scope.last.clone());
        }
        // The trace starts at the crossing, rising.
        let first = &traces[4];
        assert!(
            first[0].abs() < 0.1 && first[3] > first[0],
            "{:?}",
            &first[..4]
        );
        for t in &traces[1..] {
            let diff = t.iter().zip(&traces[0]).map(|(a, b)| (a - b).abs());
            let most = diff.fold(0.0f32, f32::max);
            assert!(most < 0.08, "traces differ by {most}");
        }
        // The automatic gain brings a 0.4 tone near the headroom.
        let top = first.iter().fold(0.0f32, |m, v| m.max(*v));
        assert!(top > 0.5 && top < 0.9, "top {top}");
    }

    /// Stereo keeps the channels apart, X/Y puts a left-only signal on the
    /// side axis's left and silence fades to rest.
    #[test]
    fn channels_and_rest() {
        let n = frames_needed(RATE);
        let left_only: Vec<[f32; 2]> = tone(220.0, 0.0, n).iter().map(|&[l, _]| [l, 0.0]).collect();
        let mut scope = Scope::default();
        scope.update(&left_only, RATE, Channels::Stereo, 1.0, 128, 0.05);
        assert_eq!(scope.values.len(), 256);
        let loud = |v: &[f32]| v.iter().fold(0.0f32, |m, x| m.max(x.abs()));
        assert!(loud(&scope.values[..128]) > 0.3);
        assert!(loud(&scope.values[128..]) < 0.01);

        scope.update(&left_only, RATE, Channels::XY, 1.0, 128, 0.05);
        assert_eq!(scope.values.len(), XY_POINTS * 2);
        // Left only: side and mid move against each other (x = -y).
        for pair in scope.values.chunks(2) {
            assert!((pair[0] + pair[1]).abs() < 0.01, "{pair:?}");
        }

        for _ in 0..40 {
            scope.update(&[], RATE, Channels::XY, 1.0, 128, 0.05);
        }
        assert!(scope.quiet());
    }
}
