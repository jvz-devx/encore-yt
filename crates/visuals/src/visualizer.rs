//! The audio visualiser (M21): `shaders/visualizer.wgsl` draws bars,
//! mirrored bars, a ring around the cover, a line spectrum or a particle
//! field from the spectrum, or an oscilloscope from the samples (M22),
//! offscreen with an alpha channel, read back like the other effects and
//! painted over the backdrop.
//!
//! [`Bars`] turns the analysis's 32 bands into the bars the settings ask
//! for (count, frequency range and spacing, sensitivity, rise smoothing,
//! fall speed, peak caps), [`Scope`] the samples into traces;
//! [`Visualizer`] draws them.

use anyhow::Result;

use crate::gpu::Gpu;
use crate::renderer::Look;
use crate::scope::{Channels, MAX_POINTS, Scope, XY_POINTS};
use crate::spectrum::{BANDS, band_at};
use crate::target::{Frame, Target};
use crate::uniform::Uniform;

/// The most bars the shader holds (four to a vec4).
pub const MAX_BARS: usize = 128;
/// The scope's values: one trace, two, or X/Y pairs.
const WAVE: usize = 2 * MAX_POINTS;
/// The X/Y figure's segments go in this many runs, each with its bounds,
/// so the shader skips the runs far from a pixel.
const RUNS: usize = 16;
/// Seven vec4s, four colour stops, the bars and the peaks, the scope's
/// values and the X/Y runs' bounds.
const PARAMS_SIZE: u64 = (7 + 4 + 2 * MAX_BARS as u64 / 4 + WAVE as u64 / 4 + RUNS as u64) * 16;
/// Levels under this are drawn as nothing (about -39 dB from the loudest).
const FLOOR: f32 = 0.18;
/// A peak cap stays this long before it falls, in seconds.
const PEAK_HOLD: f32 = 0.25;

/// How the bands become bars.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BarSettings {
    pub count: usize,
    /// Gain on the levels.
    pub sensitivity: f32,
    /// How much of the last value a rising bar keeps per 1/60 s, 0..0.95.
    pub smoothing: f32,
    /// How fast a bar falls, in heights a second.
    pub decay: f32,
    pub low_hz: f32,
    pub high_hz: f32,
    /// Bars evenly spaced in Hz rather than in octaves.
    pub linear: bool,
    /// How fast a peak cap falls, in heights a second.
    pub peak_fall: f32,
}

/// The bars' heights and peak caps, 0..1, moving with the music.
#[derive(Clone, Debug, Default)]
pub struct Bars {
    pub values: Vec<f32>,
    pub peaks: Vec<f32>,
    hold: Vec<f32>,
}

impl Bars {
    /// Moves the bars `dt` seconds towards `levels` (the analysis's bands).
    pub fn update(&mut self, levels: &[f32; BANDS], s: &BarSettings, dt: f32) {
        let n = s.count.clamp(1, MAX_BARS);
        if self.values.len() != n {
            self.values = vec![0.0; n];
            self.peaks = vec![0.0; n];
            self.hold = vec![0.0; n];
        }
        let keep = s.smoothing.clamp(0.0, 0.95).powf(dt * 60.0);
        for j in 0..n {
            let at = (j as f32 + 0.5) / n as f32;
            let hz = if s.linear {
                s.low_hz + (s.high_hz - s.low_hz) * at
            } else {
                s.low_hz * (s.high_hz / s.low_hz).powf(at)
            };
            let band = band_at(hz);
            let (lo, f) = (band.floor() as usize, band.fract());
            let hi = (lo + 1).min(BANDS - 1);
            let level = levels[lo] + (levels[hi] - levels[lo]) * f;
            // The analysis keeps 48 dB; the quietest part of it (the
            // noise floor between notes) stays flat, so loud bars don't
            // all sit at the top.
            let gated = ((level - FLOOR) / (1.0 - FLOOR)).max(0.0).powf(1.2);
            let target = (gated * s.sensitivity).clamp(0.0, 1.0);
            let v = &mut self.values[j];
            *v = if target > *v {
                target + (*v - target) * keep
            } else {
                (*v - s.decay * dt).max(target)
            };
            let (peak, hold) = (&mut self.peaks[j], &mut self.hold[j]);
            if *v >= *peak {
                *peak = *v;
                *hold = PEAK_HOLD;
            } else if *hold > 0.0 {
                *hold -= dt;
            } else {
                *peak = (*peak - s.peak_fall * dt).max(*v);
            }
        }
    }

    /// Everything has fallen to nothing.
    pub fn quiet(&self) -> bool {
        self.values.iter().chain(&self.peaks).all(|&v| v < 0.004)
    }
}

/// Inputs for one frame. Positions and sizes are in output pixels.
#[derive(Clone, Copy, Debug)]
pub struct VisualizerParams<'a> {
    /// 0 bars, 1 mirrored, 2 ring, 3 line, 4 particles, 5 scope.
    pub style: u32,
    pub seconds: f32,
    /// The particles' clock: runs faster when the music is loud.
    pub travel: f32,
    pub bass: f32,
    pub kick: f32,
    pub level: f32,
    pub treble: f32,
    pub look: Look,
    pub opacity: f32,
    /// 0..2, 1 a soft halo.
    pub glow: f32,
    /// The line's and the scope's stroke, in output pixels.
    pub thickness: f32,
    pub peaks: bool,
    /// Output pixels per point.
    pub scale: f32,
    /// The cover (the ring goes round it): left, top, right, bottom, and
    /// its corner radius.
    pub cover: Option<([f32; 4], f32)>,
    /// The ring's longest bar.
    pub reach: f32,
    /// The margin bars, mirrored bars and the line keep at each side.
    pub margin: f32,
    /// The gradient along the spectrum, linear RGB, low to high.
    pub stops: [[f32; 3]; 4],
    pub bars: &'a Bars,
    pub scope: &'a Scope,
}

pub struct Visualizer {
    gpu: Gpu,
    pipeline: wgpu::RenderPipeline,
    uniforms: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    target: Target,
}

impl Visualizer {
    pub fn new(gpu: &Gpu, width: u32, height: u32) -> Self {
        let effect = gpu.pipelines.visualizer.clone();
        let uniforms = gpu.uniforms("visualizer params", PARAMS_SIZE);
        let sampler = gpu.sampler(wgpu::AddressMode::ClampToEdge);
        let bind_group = gpu.bind_group(&effect.layout, &uniforms, &[], &sampler);
        Self {
            gpu: gpu.clone(),
            pipeline: effect.pipeline,
            uniforms,
            bind_group,
            target: Target::new(gpu, width, height),
        }
    }

    pub fn size(&self) -> (u32, u32) {
        self.target.size()
    }

    /// Draws `width`×`height` frames from now on; the next frame shows
    /// nothing yet.
    pub fn resize(&mut self, width: u32, height: u32) {
        self.target.resize(&self.gpu, width, height);
    }

    /// Forgets the frame in flight (the visualiser was hidden).
    pub fn discard(&mut self) {
        self.target.discard();
    }

    /// Renders a frame and returns the one rendered on the previous call.
    pub fn frame(&mut self, params: &VisualizerParams<'_>) -> Result<Option<Frame>> {
        let uniforms = self.params_bytes(params);
        self.gpu.queue.write_buffer(&self.uniforms, 0, &uniforms);
        let (pipeline, group) = (&self.pipeline, &self.bind_group);
        self.target.frame(&self.gpu, |pass| {
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, group, &[]);
            pass.draw(0..3, 0..1);
        })
    }

    /// Renders a frame and waits for it.
    pub fn frame_now(&mut self, params: &VisualizerParams<'_>) -> Result<Frame> {
        let uniforms = self.params_bytes(params);
        self.gpu.queue.write_buffer(&self.uniforms, 0, &uniforms);
        let (pipeline, group) = (&self.pipeline, &self.bind_group);
        self.target.frame_now(&self.gpu, |pass| {
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, group, &[]);
            pass.draw(0..3, 0..1);
        })
    }

    fn params_bytes(&self, p: &VisualizerParams<'_>) -> [u8; PARAMS_SIZE as usize] {
        let flag = |on: bool| if on { 1.0 } else { 0.0 };
        let (width, height) = self.target.size();
        let count = p.bars.values.len().min(MAX_BARS);
        let mut floats = Uniform::new();
        floats.extend([width as f32, height as f32, p.style as f32, count as f32]);
        floats.extend([p.seconds, p.bass, p.kick, p.level]);
        floats.extend([
            p.opacity,
            p.glow,
            flag(p.peaks),
            flag(p.look == Look::Light),
        ]);
        let (cover, corner) = p.cover.unwrap_or_default();
        floats.extend(cover);
        floats.extend([corner, p.reach, p.travel, p.treble]);
        floats.extend([
            p.scale,
            flag(p.cover.is_some()),
            p.margin,
            p.thickness * 0.5,
        ]);
        let channels = p.scope.channels;
        floats.extend([p.scope.points as f32, channels.index() as f32, 0.0, 0.0]);
        for stop in &p.stops {
            floats.extend(*stop);
            floats.push(1.0);
        }
        for list in [&p.bars.values, &p.bars.peaks] {
            let mut values = [0.0f32; MAX_BARS];
            for (v, s) in values.iter_mut().zip(list.iter()) {
                *v = *s;
            }
            floats.extend(values);
        }
        let mut wave = [0.0f32; WAVE];
        for (v, s) in wave.iter_mut().zip(&p.scope.values) {
            *v = *s;
        }
        floats.extend(wave);
        floats.extend(runs(&p.scope.values, channels).into_iter().flatten());
        floats.finish()
    }
}

/// The bounds (least x, least y, most x, most y) of each run of the X/Y
/// figure's segments; nothing for the traces.
fn runs(values: &[f32], channels: Channels) -> [[f32; 4]; RUNS] {
    let mut out = [[0.0; 4]; RUNS];
    if channels != Channels::XY || values.len() < XY_POINTS * 2 {
        return out;
    }
    let per = XY_POINTS.div_ceil(RUNS);
    for (r, bounds) in out.iter_mut().enumerate() {
        let first = r * per;
        let last = ((r + 1) * per).min(XY_POINTS - 1);
        let mut b = [f32::MAX, f32::MAX, f32::MIN, f32::MIN];
        for k in first..=last {
            let (x, y) = (values[2 * k], values[2 * k + 1]);
            b = [b[0].min(x), b[1].min(y), b[2].max(x), b[3].max(y)];
        }
        *bounds = b;
    }
    out
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    reason = "GPU regression fixtures require successful frames"
)]
mod tests {
    use super::*;

    fn settings(count: usize) -> BarSettings {
        BarSettings {
            count,
            sensitivity: 1.0,
            smoothing: 0.5,
            decay: 2.0,
            low_hz: 50.0,
            high_hz: 16_000.0,
            linear: false,
            peak_fall: 1.0,
        }
    }

    /// Bars rise towards loud bands, fall at the decay speed once they
    /// go quiet, and leave their peak caps above them for a moment.
    #[test]
    fn bars_rise_fall_and_keep_their_peaks() {
        let mut bars = Bars::default();
        let s = settings(48);
        let loud = [0.9; BANDS];
        for _ in 0..30 {
            bars.update(&loud, &s, 1.0 / 60.0);
        }
        assert_eq!(bars.values.len(), 48);
        let full = ((0.9 - FLOOR) / (1.0 - FLOOR)).powf(1.2);
        assert!(bars.values.iter().all(|&v| (v - full).abs() < 0.01));
        bars.update(&[0.0; BANDS], &s, 0.1);
        assert!(bars.values.iter().all(|&v| (v - (full - 0.2)).abs() < 0.01));
        assert!(bars.peaks.iter().all(|&p| p > full - 0.05));
        for _ in 0..120 {
            bars.update(&[0.0; BANDS], &s, 1.0 / 60.0);
        }
        assert!(bars.quiet());
    }

    /// Slider extremes still respond at each paced frame rate. The maximum
    /// frequency handles clamp to 2..16 kHz, so bass-only audio correctly
    /// leaves that view flat; energy in its selected bands must still rise.
    #[test]
    fn slider_extremes_keep_following_the_selected_bands() {
        let min = BarSettings {
            count: 16,
            sensitivity: 0.5,
            smoothing: 0.,
            decay: 0.3,
            low_hz: 50.,
            high_hz: 100.,
            peak_fall: 0.1,
            ..settings(16)
        };
        let max = BarSettings {
            count: 128,
            sensitivity: 3.,
            smoothing: 0.95,
            decay: 8.,
            low_hz: 2_000.,
            high_hz: 16_000.,
            peak_fall: 4.,
            ..settings(128)
        };
        for s in [min, max] {
            for fps in [15, 20, 30, 60, 120] {
                let dt = 1. / fps as f32;
                let mut bars = Bars::default();
                for _ in 0..fps {
                    bars.update(&[1.; BANDS], &s, dt);
                }
                assert_eq!(bars.values.len(), s.count);
                assert!(
                    bars.values
                        .iter()
                        .all(|v| v.is_finite() && *v > 0.4 && *v <= 1.)
                );
                // Even the slowest peak fall reaches zero in this interval.
                for _ in 0..fps * 12 {
                    bars.update(&[0.; BANDS], &s, dt);
                }
                assert!(bars.quiet());
            }
        }
        let mut bass_only = [0.; BANDS];
        bass_only[0] = 1.;
        let mut bars = Bars::default();
        bars.update(&bass_only, &max, 1. / 20.);
        assert!(bars.quiet());
    }

    /// Each style draws something from a loud spectrum, and leaves most of
    /// the frame see-through when the music is silent.
    #[test]
    fn every_style_draws() {
        let _one = crate::gpu_test_lock();
        let gpu = match Gpu::new() {
            Ok(gpu) => gpu,
            Err(e) => {
                eprintln!("skipped, no GPU: {e:#}");
                return;
            }
        };
        let mut vis = Visualizer::new(&gpu, 160, 120);
        let mut bars = Bars::default();
        let mut levels = [0.0; BANDS];
        for (i, l) in levels.iter_mut().enumerate() {
            *l = 0.3 + 0.6 * (i as f32 * 0.7).sin().abs();
        }
        let quiet = Bars::default();
        let silent = Scope::default();
        for _ in 0..20 {
            bars.update(&levels, &settings(32), 1.0 / 60.0);
        }
        let rate = 48_000;
        let sound: Vec<[f32; 2]> = (0..crate::scope::frames_needed(rate))
            .map(|i| {
                let t = i as f32 / rate as f32;
                let s = (std::f32::consts::TAU * 180.0 * t).sin();
                [s * 0.5, s * 0.3]
            })
            .collect();
        let mut kinds: Vec<(u32, Channels)> = (0..5).map(|s| (s, Channels::Mono)).collect();
        kinds.extend([Channels::Mono, Channels::Stereo, Channels::XY].map(|c| (5, c)));
        for (style, channels) in kinds {
            let mut scope = Scope::default();
            scope.update(&sound, rate, channels, 1.0, 64, 0.05);
            let mut p = VisualizerParams {
                style,
                seconds: 2.0,
                travel: 2.0,
                bass: 0.8,
                kick: 0.6,
                level: 0.6,
                treble: 0.4,
                look: Look::Dark,
                opacity: 1.0,
                glow: 1.0,
                thickness: 2.5,
                peaks: true,
                scale: 1.0,
                cover: Some(([50.0, 30.0, 110.0, 90.0], 8.0)),
                reach: 24.0,
                margin: 0.0,
                stops: [
                    [1.0, 0.3, 0.3],
                    [0.9, 0.6, 0.2],
                    [0.3, 0.8, 0.5],
                    [0.3, 0.4, 1.0],
                ],
                bars: &bars,
                scope: &scope,
            };
            let frame = vis.frame_now(&p).expect("frame");
            let lit = frame.bgra.chunks(4).filter(|px| px[3] > 128).count();
            assert!(lit > 40, "style {style} {channels:?}: {lit} opaque pixels");
            p.bars = &quiet;
            p.scope = &silent;
            p.bass = 0.0;
            p.kick = 0.0;
            p.level = 0.0;
            let frame = vis.frame_now(&p).expect("frame");
            let lit = frame.bgra.chunks(4).filter(|px| px[3] > 128).count();
            assert!(
                lit < 160 * 120 / 10,
                "style {style} {channels:?} silent: {lit} opaque pixels"
            );
        }
    }
}
