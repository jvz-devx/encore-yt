//! The audio visualiser (M21): `shaders/visualizer.wgsl` draws bars,
//! mirrored bars, a ring around the cover, a line spectrum or a particle
//! field from the spectrum, offscreen with an alpha channel, read back like
//! the other effects and painted over the backdrop.
//!
//! [`Bars`] turns the analysis's 32 bands into the bars the settings ask
//! for (count, frequency range and spacing, sensitivity, rise smoothing,
//! fall speed, peak caps); [`Visualizer`] draws them.

use anyhow::Result;

use crate::gpu::{Gpu, bytes};
use crate::renderer::Look;
use crate::spectrum::{BANDS, band_at};
use crate::target::{Frame, Target};

/// The most bars the shader holds (four to a vec4).
pub const MAX_BARS: usize = 128;
/// Ten vec4s, four colour stops, then the bars and the peaks.
const PARAMS_SIZE: u64 = (10 + 4 + 2 * MAX_BARS as u64 / 4) * 16;
/// The style that draws the backdrop's ambient layer (wave and sparkles).
pub const AMBIENT: u32 = 5;
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
    /// 0 bars, 1 mirrored, 2 ring, 3 line, 4 particles, [`AMBIENT`].
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
    pub peaks: bool,
    /// Output pixels per point.
    pub scale: f32,
    /// The cover (the ring goes round it): left, top, right, bottom, and
    /// its corner radius.
    pub cover: Option<([f32; 4], f32)>,
    /// The ring's longest bar.
    pub reach: f32,
    /// The gradient along the spectrum, linear RGB, low to high.
    pub stops: [[f32; 3]; 4],
    pub bars: &'a Bars,
    /// The ambient layer's settings (only [`AMBIENT`] reads them).
    pub ambient: Ambient,
}

/// The backdrop's ambient layer: sparkles and the wave. The clocks are
/// kept by the caller, so a speed can change without a jump.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Ambient {
    /// How many sparkles, 0..2 (0: none).
    pub amount: f32,
    /// The far and the near sparkles' radius, in device pixels.
    pub size_min: f32,
    pub size_max: f32,
    /// 0 a crisp dot, 1 a soft glow.
    pub softness: f32,
    pub brightness: f32,
    /// The drift's clock: seconds at the drift speed.
    pub drift: f32,
    /// How much size, brightness and speed differ with depth, 0..1.
    pub depth: f32,
    /// How much the music lifts their brightness, 0..1.
    pub reaction: f32,
    /// How much they fade in and out, 0..1, and the twinkle's clock.
    pub twinkle: f32,
    pub twinkle_clock: f32,
    /// Where they drift, in radians (0 rightwards, counter-clockwise).
    pub direction: f32,
    /// How much of the colour stops tints them, 0..1 (0 white).
    pub tint: f32,
    /// The wave's strength, 0..2 (0: none), its clock, its ribbons (1..3)
    /// and the height of its middle (0 top, 1 bottom).
    pub wave: f32,
    pub wave_clock: f32,
    pub ribbons: u32,
    pub wave_height: f32,
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
    pub fn frame(&mut self, params: &VisualizerParams) -> Result<Option<Frame>> {
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
    pub fn frame_now(&mut self, params: &VisualizerParams) -> Result<Frame> {
        let uniforms = self.params_bytes(params);
        self.gpu.queue.write_buffer(&self.uniforms, 0, &uniforms);
        let (pipeline, group) = (&self.pipeline, &self.bind_group);
        self.target.frame_now(&self.gpu, |pass| {
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, group, &[]);
            pass.draw(0..3, 0..1);
        })
    }

    fn params_bytes(&self, p: &VisualizerParams) -> Vec<u8> {
        let flag = |on: bool| if on { 1.0 } else { 0.0 };
        let (width, height) = self.target.size();
        let count = p.bars.values.len().min(MAX_BARS);
        let mut floats = vec![width as f32, height as f32, p.style as f32, count as f32];
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
        floats.extend([p.scale, flag(p.cover.is_some()), 0.0, 0.0]);
        let a = &p.ambient;
        floats.extend([a.amount, a.size_min, a.size_max, a.softness]);
        floats.extend([a.brightness, a.drift, a.depth, a.reaction]);
        floats.extend([a.twinkle, a.twinkle_clock, a.direction, a.tint]);
        floats.extend([a.wave, a.wave_clock, a.ribbons as f32, a.wave_height]);
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
        bytes(&floats)
    }
}

#[cfg(test)]
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

    /// Each style draws something from a loud spectrum, and leaves most of
    /// the frame see-through when the music is silent.
    #[test]
    fn every_style_draws() {
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
        for _ in 0..20 {
            bars.update(&levels, &settings(32), 1.0 / 60.0);
        }
        for style in 0..5 {
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
                peaks: true,
                scale: 1.0,
                cover: Some(([50.0, 30.0, 110.0, 90.0], 8.0)),
                reach: 24.0,
                stops: [
                    [1.0, 0.3, 0.3],
                    [0.9, 0.6, 0.2],
                    [0.3, 0.8, 0.5],
                    [0.3, 0.4, 1.0],
                ],
                bars: &bars,
                ambient: Ambient::default(),
            };
            let frame = vis.frame_now(&p).expect("frame");
            let lit = frame.bgra.chunks(4).filter(|px| px[3] > 128).count();
            assert!(lit > 40, "style {style}: {lit} opaque pixels");
            p.bars = &quiet;
            p.bass = 0.0;
            p.kick = 0.0;
            p.level = 0.0;
            let frame = vis.frame_now(&p).expect("frame");
            let lit = frame.bgra.chunks(4).filter(|px| px[3] > 128).count();
            assert!(
                lit < 160 * 120 / 10,
                "style {style} silent: {lit} opaque pixels"
            );
        }
    }

    /// The ambient layer stays in the background: its sparkles and wave
    /// light many pixels a little and none of them strongly, also on the
    /// beat, in either look.
    #[test]
    fn ambient_layer_stays_faint() {
        let gpu = match Gpu::new() {
            Ok(gpu) => gpu,
            Err(e) => {
                eprintln!("skipped, no GPU: {e:#}");
                return;
            }
        };
        let mut vis = Visualizer::new(&gpu, 320, 240);
        let bars = Bars::default();
        for look in [Look::Dark, Look::Light] {
            let p = VisualizerParams {
                style: AMBIENT,
                seconds: 3.0,
                travel: 3.0,
                bass: 1.0,
                kick: 1.0,
                level: 1.0,
                treble: 0.5,
                look,
                opacity: 1.0,
                glow: 0.0,
                peaks: false,
                scale: 1.0,
                cover: None,
                reach: 0.0,
                stops: [[0.9, 0.4, 0.5]; 4],
                bars: &bars,
                ambient: Ambient {
                    amount: 1.0,
                    size_min: 0.7,
                    size_max: 1.4,
                    softness: 0.8,
                    brightness: 1.0,
                    drift: 3.0,
                    depth: 1.0,
                    reaction: 0.5,
                    twinkle: 0.6,
                    twinkle_clock: 3.0,
                    direction: 0.0,
                    tint: 1.0,
                    wave: 1.0,
                    wave_clock: 3.0,
                    ribbons: 3,
                    wave_height: 0.56,
                },
            };
            let frame = vis.frame_now(&p).expect("frame");
            let alphas: Vec<u8> = frame.bgra.chunks(4).map(|px| px[3]).collect();
            let lit = alphas.iter().filter(|a| **a > 4).count();
            let strongest = alphas.iter().copied().max().unwrap_or(0);
            assert!(lit > 320 * 240 / 20, "{look:?}: only {lit} pixels lit");
            assert!(strongest < 230, "{look:?}: a pixel at alpha {strongest}");
        }
    }
}
