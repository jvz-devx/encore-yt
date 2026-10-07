//! Sample-exact rate conversion for interleaved stereo: rubato's FFT
//! resampler with its start-up delay cut off and its tail flushed, so a
//! track of N input frames gives exactly `round(N * out / in)` output
//! frames. That keeps gapless joins exact when a 44.1 kHz AAC track plays on
//! a 48 kHz device.

use anyhow::Result;
use rubato::audioadapter_buffers::direct::InterleavedSlice;
use rubato::{Fft, FixedSync, Indexing, Resampler as _};

const CHUNK: usize = 1024;

pub struct Resampler {
    inner: Fft<f32>,
    ratio: f64,
    pending: Vec<f32>,
    out: Vec<f32>,
    /// Output frames still to drop (the filter's delay).
    skip: usize,
    frames_in: u64,
    frames_out: u64,
}

impl Resampler {
    pub fn new(from: u32, to: u32) -> Result<Self> {
        let inner = Fft::<f32>::new(from as usize, to as usize, CHUNK, 2, FixedSync::Input)?;
        let out = vec![0.0; inner.output_frames_max() * 2];
        Ok(Self {
            skip: inner.output_delay(),
            ratio: to as f64 / from as f64,
            inner,
            pending: Vec::with_capacity(CHUNK * 4),
            out,
            frames_in: 0,
            frames_out: 0,
        })
    }

    /// Feeds interleaved stereo and calls `emit` with what is ready.
    pub fn push(&mut self, input: &[f32], emit: &mut dyn FnMut(&[f32])) -> Result<()> {
        self.pending.extend_from_slice(input);
        self.frames_in += (input.len() / 2) as u64;
        while self.pending.len() >= CHUNK * 2 {
            self.run(CHUNK, None, emit)?;
        }
        Ok(())
    }

    /// Flushes the rest at the end of the track.
    pub fn finish(&mut self, emit: &mut dyn FnMut(&[f32])) -> Result<()> {
        let target = (self.frames_in as f64 * self.ratio).round() as u64;
        while self.frames_out < target {
            let have = self.pending.len() / 2;
            self.pending.resize(CHUNK * 2, 0.0);
            let limit = target - self.frames_out;
            self.run(CHUNK, Some((have.min(CHUNK), limit)), emit)?;
        }
        Ok(())
    }

    fn run(
        &mut self,
        frames: usize,
        partial: Option<(usize, u64)>,
        emit: &mut dyn FnMut(&[f32]),
    ) -> Result<()> {
        let input = InterleavedSlice::new(&self.pending[..frames * 2], 2, frames)?;
        let out_frames = self.out.len() / 2;
        let mut output = InterleavedSlice::new_mut(&mut self.out, 2, out_frames)?;
        let indexing = partial.map(|(len, _)| Indexing {
            input_offset: 0,
            output_offset: 0,
            partial_len: Some(len),
            active_channels_mask: None,
        });
        let (used, made) =
            self.inner
                .process_into_buffer(&input, &mut output, indexing.as_ref())?;
        self.pending.drain(..used.min(frames) * 2);
        if partial.is_some() {
            self.pending.clear();
        }
        let mut ready = &self.out[..made * 2];
        let skip = self.skip.min(made);
        self.skip -= skip;
        ready = &ready[skip * 2..];
        if let Some((_, limit)) = partial {
            let keep = (ready.len() / 2).min(limit as usize);
            ready = &ready[..keep * 2];
        }
        self.frames_out += (ready.len() / 2) as u64;
        emit(ready);
        Ok(())
    }
}
