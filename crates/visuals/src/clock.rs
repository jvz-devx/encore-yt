//! Animation clocks: f64 seconds on the CPU, small f32 values on the GPU.
//!
//! An f32 clock that only ever grows loses the precision of its frame
//! steps after hours (at 1e5 seconds an f32 moves in steps of 8 ms, a
//! quarter of a frame), and shaders that multiply it into `sin` or noise
//! lose theirs sooner. So every clock is an f64 that the app keeps, and
//! what a shader gets is the f64 less a base, cast to f32. The base moves
//! only where the shader's picture doesn't jump: by a whole period of
//! what the shader uses the clock for ([`wrap`]), or at a moment the
//! picture changes anyway, such as a new cover cross-fading in or a scene
//! re-seeded for a new song ([`Rebase`]). `docs/gpui/VISUALS.md` ("Clocks")
//! has the choice per clock.

use std::f64::consts::PI;

/// The backdrop's light wave and the player bar's glow: every `sin` they
/// take of their clock turns at a whole multiple of 0.005 rad/s (0.05 to
/// 0.165), so both repeat exactly every 2π / 0.005 seconds.
pub const WAVE_PERIOD: f64 = 400.0 * PI;

/// The backdrop's slow turn of the cover: 0.025 rad per second of flow.
pub const TURN_PERIOD: f64 = 80.0 * PI;

/// The visualiser's particles sway at 0.7 and 0.5 rad/s: both repeat every
/// 2π / 0.1 seconds.
pub const SWAY_PERIOD: f64 = 20.0 * PI;

/// The backdrop's dither takes only `fract` of its clock: any whole number
/// of seconds is a period.
pub const GRAIN_PERIOD: f64 = 1000.0;

/// A clock past this many seconds from its base starts again at the next
/// moment a jump is hidden. An f32 of 4096 still moves in steps of 0.5 ms;
/// the next hidden moment (a song later) leaves it well under 1e4.
pub const REBASE_AFTER: f64 = 4096.0;

/// `t` within one `period` (exact for a shader that repeats every
/// `period`), as an f32.
pub fn wrap(t: f64, period: f64) -> f32 {
    t.rem_euclid(period) as f32
}

/// What a shader gets of an f64 clock: the clock less a base, as f32. The
/// base moves only when [`Self::hidden`] says the picture changes anyway.
#[derive(Clone, Copy, Debug, Default)]
pub struct Rebase {
    base: f64,
}

impl Rebase {
    /// The value for the GPU at clock `t`.
    pub fn gpu(&self, t: f64) -> f32 {
        (t - self.base) as f32
    }

    /// A moment a jump is hidden: once the clock `t` is [`REBASE_AFTER`]
    /// past the base, the base moves up to the last whole `period` below
    /// `t` (keeping a use of the clock that repeats every `period` where
    /// it was), or to `t` itself when `period` is 0. Returns whether it
    /// moved.
    pub fn hidden(&mut self, t: f64, period: f64) -> bool {
        if t - self.base < REBASE_AFTER {
            return false;
        }
        self.base = if period > 0.0 {
            (t / period).floor() * period
        } else {
            t
        };
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A rebase by whole periods keeps a periodic use of the clock where it
    /// was, and leaves the value small.
    #[test]
    fn a_rebase_keeps_the_periodic_part() {
        let mut r = Rebase::default();
        let t = 5000.123_f64;
        let before = (r.gpu(t) as f64 * 0.115).sin();
        assert!(r.hidden(t, WAVE_PERIOD));
        assert!(r.gpu(t) < WAVE_PERIOD as f32);
        let after = (r.gpu(t) as f64 * 0.115).sin();
        assert!((before - after).abs() < 1e-3, "{before} {after}");
        // Not again until the clock is far enough on.
        assert!(!r.hidden(t + 60.0, WAVE_PERIOD));
    }

    #[test]
    fn wrap_is_continuous_for_periodic_uses() {
        for k in [1.0, 1000.0, 1e6] {
            let t = WAVE_PERIOD * k + 0.5;
            let s = (f64::from(wrap(t, WAVE_PERIOD)) * 0.09).sin();
            assert!((s - (0.5f64 * 0.09).sin()).abs() < 1e-4);
        }
    }
}
