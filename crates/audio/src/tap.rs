//! A copy of what the engine plays, for the visualiser (PLAN M22).
//!
//! The output callback writes every mixed frame, both channels in one
//! 64-bit atomic, into one ring, after the equalizer and every deck's
//! volume: what is heard. Readers take it as mono ([`Tap::read`]) or as
//! stereo ([`Tap::read_stereo`], for the oscilloscope).
//! Readers ([`Tap`]) follow it at their own pace on their own threads. The
//! callback never waits: with no reader it skips the copy, it never reads
//! what readers do, and a reader that falls behind by more than the ring
//! holds skips ahead (it may then see a few samples overwritten mid-read,
//! which only a visualiser sees). One engine per process, so one writer.

use std::sync::atomic::{AtomicU32, AtomicU64, AtomicUsize, Ordering};

/// Frames the ring holds: ~0.68 s at 48 kHz, many output periods.
const SIZE: usize = 1 << 15;

/// Each frame's left channel in the high 32 bits, its right in the low.
static RING: [AtomicU64; SIZE] = [const { AtomicU64::new(0) }; SIZE];
/// Samples written since the process started.
static WRITTEN: AtomicU64 = AtomicU64::new(0);
/// The output's sample rate; 0 until the engine has a device.
static RATE: AtomicU32 = AtomicU32::new(0);
static READERS: AtomicUsize = AtomicUsize::new(0);

pub(crate) fn set_rate(rate: u32) {
    RATE.store(rate, Ordering::Relaxed);
}

/// The output callback's interleaved stereo mix, as it goes to the device.
pub(crate) fn write(stereo: &[f32]) {
    if READERS.load(Ordering::Relaxed) == 0 {
        return;
    }
    let start = WRITTEN.load(Ordering::Relaxed);
    let frames = stereo.as_chunks::<2>().0;
    for (i, [l, r]) in frames.iter().enumerate() {
        let at = (start as usize).wrapping_add(i) % SIZE;
        RING[at].store(pack(*l, *r), Ordering::Relaxed);
    }
    WRITTEN.store(start + frames.len() as u64, Ordering::Release);
}

fn pack(l: f32, r: f32) -> u64 {
    (u64::from(l.to_bits()) << 32) | u64::from(r.to_bits())
}

fn unpack(bits: u64) -> [f32; 2] {
    [
        f32::from_bits((bits >> 32) as u32),
        f32::from_bits(bits as u32),
    ]
}

/// One reader of what the engine plays. While any is open the output
/// callback copies its mix; it starts at the newest sample.
pub struct Tap {
    read: u64,
}

impl Tap {
    pub fn open() -> Self {
        READERS.fetch_add(1, Ordering::Relaxed);
        Self {
            read: WRITTEN.load(Ordering::Acquire),
        }
    }

    /// The samples' rate; 0 before the engine opened its output.
    pub fn rate(&self) -> u32 {
        RATE.load(Ordering::Relaxed)
    }

    /// Samples written and not read yet.
    pub fn available(&self) -> usize {
        let written = WRITTEN.load(Ordering::Acquire);
        written.saturating_sub(self.read).min(SIZE as u64) as usize
    }

    /// Drops what waits beyond the newest `keep` samples.
    pub fn skip_to(&mut self, keep: usize) {
        let written = WRITTEN.load(Ordering::Acquire);
        self.read = self.read.max(written.saturating_sub(keep as u64));
    }

    /// Appends up to `n` of the oldest waiting samples, as mono, to `out`;
    /// returns how many.
    pub fn read(&mut self, n: usize, out: &mut Vec<f32>) -> usize {
        self.take(n, |[l, r]| out.push((l + r) * 0.5))
    }

    /// Appends up to `n` of the oldest waiting frames, left and right, to
    /// `out`; returns how many.
    pub fn read_stereo(&mut self, n: usize, out: &mut Vec<[f32; 2]>) -> usize {
        self.take(n, |frame| out.push(frame))
    }

    fn take(&mut self, n: usize, mut each: impl FnMut([f32; 2])) -> usize {
        let written = WRITTEN.load(Ordering::Acquire);
        // Lapped: the oldest half of the ring is being written over.
        self.read = self.read.max(written.saturating_sub(SIZE as u64 / 2));
        let n = n.min(written.saturating_sub(self.read) as usize);
        let from = self.read as usize;
        for i in 0..n {
            each(unpack(
                RING[from.wrapping_add(i) % SIZE].load(Ordering::Relaxed),
            ));
        }
        self.read += n as u64;
        n
    }
}

impl Default for Tap {
    fn default() -> Self {
        Self::open()
    }
}

impl Drop for Tap {
    fn drop(&mut self) {
        READERS.fetch_sub(1, Ordering::Relaxed);
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

    /// A reader gets the mix as mono or stereo in order, nothing twice, and skips
    /// ahead when it falls a ring behind. (One test: the ring is global.)
    #[test]
    fn readers_follow_the_mix() {
        let mut tap = Tap::open();
        write(&[1.0, 0.0, 0.5, 0.5, -1.0, -1.0]);
        let mut out = Vec::new();
        assert_eq!(tap.read(16, &mut out), 3);
        assert_eq!(out, [0.5, 0.5, -1.0]);
        assert_eq!(tap.read(16, &mut out), 0);
        write(&[0.25, -0.75]);
        let mut frames = Vec::new();
        assert_eq!(tap.read_stereo(16, &mut frames), 1);
        assert_eq!(frames, [[0.25, -0.75]]);

        let burst: Vec<f32> = (0..SIZE * 2).map(|i| (i / 2) as f32).collect();
        write(&burst);
        out.clear();
        tap.read(SIZE, &mut out);
        assert_eq!(out.len(), SIZE / 2);
        assert_eq!(out[0], (SIZE / 2) as f32);
        assert_eq!(*out.last().expect("samples"), (SIZE - 1) as f32);
    }
}
