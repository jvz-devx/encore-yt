//! The 10-band equalizer: peaking biquads one octave wide at the backend's
//! centres (src/equalizer.rs), with the same headroom preamp. These are
//! RBJ cookbook peaking filters with the bandwidth in octaves, which is what
//! FFmpeg's `equalizer=f=..:t=o:w=1` (the mpv graph today) computes.

use std::f32::consts::PI;

pub const BANDS: [f32; 10] = [
    31.0, 62.0, 125.0, 250.0, 500.0, 1000.0, 2000.0, 4000.0, 8000.0, 16000.0,
];
pub const RANGE: f32 = 12.0;

#[derive(Clone, Copy, Debug, Default)]
struct Coeffs {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
}

impl Coeffs {
    fn peaking(freq: f32, gain_db: f32, rate: f32) -> Self {
        let a = 10f32.powf(gain_db / 40.0);
        let w0 = 2.0 * PI * freq / rate;
        let (sin, cos) = w0.sin_cos();
        // Bandwidth of one octave.
        let alpha = sin * ((2f32.ln() / 2.0) * w0 / sin).sinh();
        let a0 = 1.0 + alpha / a;
        Self {
            b0: (1.0 + alpha * a) / a0,
            b1: (-2.0 * cos) / a0,
            b2: (1.0 - alpha * a) / a0,
            a1: (-2.0 * cos) / a0,
            a2: (1.0 - alpha / a) / a0,
        }
    }
}

/// Filter memory for one channel of one band (transposed direct form II).
#[derive(Clone, Copy, Debug, Default)]
struct Memory {
    s1: f32,
    s2: f32,
}

/// Coefficients for every band, computed off the audio thread.
#[derive(Clone, Debug)]
pub struct EqSettings {
    coeffs: Vec<Coeffs>,
    preamp: f32,
}

impl EqSettings {
    /// `None` when the gains are flat (the mixer then skips the filters).
    pub fn new(gains: [f32; 10], rate: u32) -> Option<Self> {
        if gains.iter().all(|g| g.abs() < 0.05) {
            return None;
        }
        let coeffs = BANDS
            .iter()
            .zip(gains)
            .filter(|(f, g)| g.abs() >= 0.05 && **f < rate as f32 * 0.45)
            .map(|(f, g)| Coeffs::peaking(*f, g.clamp(-RANGE, RANGE), rate as f32))
            .collect();
        let peak = gains.iter().fold(0f32, |m, g| m.max(*g)).min(RANGE);
        Some(Self {
            coeffs,
            preamp: 10f32.powf(-peak / 20.0),
        })
    }
}

/// The running filter for interleaved stereo.
#[derive(Default)]
pub struct Equalizer {
    settings: Option<EqSettings>,
    memory: Vec<[Memory; 2]>,
}

impl Equalizer {
    /// Swaps in new coefficients, keeping the filter memory of bands that
    /// stay, so a band edit doesn't click.
    pub fn set(&mut self, settings: Option<EqSettings>) {
        let bands = settings.as_ref().map_or(0, |s| s.coeffs.len());
        self.memory.resize(bands, [Memory::default(); 2]);
        self.settings = settings;
    }

    pub fn process(&mut self, frames: &mut [f32]) {
        let Some(settings) = &self.settings else {
            return;
        };
        for frame in frames.as_chunks_mut::<2>().0 {
            for (ch, sample) in frame.iter_mut().enumerate() {
                let mut x = *sample * settings.preamp;
                for (c, mem) in settings.coeffs.iter().zip(self.memory.iter_mut()) {
                    let m = &mut mem[ch];
                    let y = c.b0 * x + m.s1;
                    m.s1 = c.b1 * x - c.a1 * y + m.s2;
                    m.s2 = c.b2 * x - c.a2 * y;
                    x = y;
                }
                *sample = x;
            }
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

    fn gain_at(freq: f32, gains: [f32; 10]) -> f32 {
        let rate = 48_000;
        let mut eq = Equalizer::default();
        eq.set(EqSettings::new(gains, rate));
        let n = rate as usize;
        let mut buf: Vec<f32> = (0..n * 2)
            .map(|i| (2.0 * PI * freq * (i / 2) as f32 / rate as f32).sin() * 0.25)
            .collect();
        eq.process(&mut buf);
        let tail = &buf[n..];
        let rms = (tail.iter().map(|s| s * s).sum::<f32>() / tail.len() as f32).sqrt();
        20.0 * (rms / (0.25 / 2f32.sqrt())).log10()
    }

    #[test]
    fn a_band_boosts_its_centre_by_its_gain_minus_the_preamp() {
        let mut gains = [0.0; 10];
        gains[5] = 6.0;
        // +6 dB at 1 kHz, then the -6 dB preamp: unity at the centre.
        assert!(gain_at(1000.0, gains).abs() < 0.3);
        // Two octaves away the band is nearly flat, so the preamp shows.
        assert!((gain_at(4000.0, gains) + 6.0).abs() < 1.0);
    }

    #[test]
    fn flat_gains_bypass_the_filters() {
        assert!(EqSettings::new([0.0; 10], 48_000).is_none());
    }
}
