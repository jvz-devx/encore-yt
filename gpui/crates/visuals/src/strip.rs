//! The player bar strip: `shaders/strip.wgsl` draws the bar's whole
//! background in one pass (palette glow, seek bar with the song's
//! waveform, beat halos) and reads it back like the backdrop.

use anyhow::Result;

use crate::gpu::{Gpu, bytes};
use crate::renderer::Look;
use crate::target::{Frame, Target};

/// The waveform texture's width: the song's outline resampled to this.
const WAVE: u32 = 512;
/// Fifteen vec4s, as `strip.wgsl`'s `Params`.
const PARAMS_SIZE: u64 = 15 * 16;

/// Inputs for one strip frame. Positions and sizes are in output pixels
/// (points times `scale`).
#[derive(Clone, Copy, Debug, Default)]
pub struct StripParams {
    /// Animation time in seconds; it stands still while paused.
    pub seconds: f32,
    /// A slow swell with the beat, 0..1 (the glow breathes with it).
    pub breath: f32,
    /// A short pulse on each beat, 0..1 (rings and playhead).
    pub kick: f32,
    /// How much of the palette's glow shows: 0 without a cover.
    pub glow: f32,
    pub look: Look,
    /// Output pixels per point.
    pub scale: f32,
    pub seek: Option<Seek>,
    /// The play button: centre x, y and radius.
    pub play: Option<[f32; 3]>,
    /// The cover thumbnail: left, top, right, bottom, and its corner radius.
    pub cover: Option<([f32; 4], f32)>,
    pub colors: StripColors,
    /// The cover's four colours (display space), already cross-faded.
    pub palette: [[f32; 4]; 4],
}

/// The seek slider's box and state.
#[derive(Clone, Copy, Debug, Default)]
pub struct Seek {
    pub left: f32,
    /// The top of the slider's box (24 points tall, track in the middle).
    pub top: f32,
    pub right: f32,
    /// 0..1.
    pub progress: f32,
    /// The song's length is known (else a bare track, no playhead).
    pub known: bool,
    /// 0..1: the pointer is over the bar.
    pub hover: f32,
}

/// The theme colours the strip paints with, display space RGB.
#[derive(Clone, Copy, Debug, Default)]
pub struct StripColors {
    /// The window's base colour, under the glow.
    pub base: [f32; 3],
    /// The played fill.
    pub signal: [f32; 3],
    /// The playhead and, at `track` opacity, the unplayed track.
    pub ink: [f32; 3],
    pub track: f32,
    /// The rings' hue (the cover's most colourful palette entry).
    pub accent: [f32; 3],
}

pub struct Strip {
    gpu: Gpu,
    pipeline: wgpu::RenderPipeline,
    uniforms: wgpu::Buffer,
    wave: wgpu::Texture,
    bind_group: wgpu::BindGroup,
    has_wave: bool,
    target: Target,
}

impl Strip {
    pub fn new(gpu: &Gpu, width: u32, height: u32) -> Self {
        let module = gpu.shader("strip", include_str!("../shaders/strip.wgsl"));
        let layout = gpu.layout("strip", 1);
        let pipeline = gpu.pipeline("strip", &module, &layout);
        let uniforms = gpu.uniforms("strip params", PARAMS_SIZE);
        let sampler = gpu.sampler(wgpu::AddressMode::ClampToEdge);
        let wave = gpu.texture("waveform", (WAVE, 1), wgpu::TextureFormat::R8Unorm);
        let view = wave.create_view(&wgpu::TextureViewDescriptor::default());
        let bind_group = gpu.bind_group(&layout, &uniforms, &[&view], &sampler);
        Self {
            gpu: gpu.clone(),
            pipeline,
            uniforms,
            wave,
            bind_group,
            has_wave: false,
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

    /// The song's outline (0..1 values, any count), or `None` for a plain
    /// track.
    pub fn set_waveform(&mut self, values: Option<&[f32]>) {
        self.has_wave = false;
        let Some(values) = values.filter(|v| !v.is_empty()) else {
            return;
        };
        let texels: Vec<u8> = (0..WAVE)
            .map(|i| {
                let at = (i as f32 + 0.5) / WAVE as f32 * values.len() as f32 - 0.5;
                let lo = at.floor().clamp(0.0, (values.len() - 1) as f32) as usize;
                let hi = (lo + 1).min(values.len() - 1);
                let f = (at - lo as f32).clamp(0.0, 1.0);
                let v = values[lo] + (values[hi] - values[lo]) * f;
                (v.clamp(0.0, 1.0) * 255.0).round() as u8
            })
            .collect();
        self.gpu.upload(&self.wave, 1, &texels);
        self.has_wave = true;
    }

    /// Renders a frame and returns the one rendered on the previous call.
    pub fn frame(&mut self, params: &StripParams) -> Result<Option<Frame>> {
        let uniforms = self.params_bytes(params);
        self.gpu.queue.write_buffer(&self.uniforms, 0, &uniforms);
        let (pipeline, group) = (&self.pipeline, &self.bind_group);
        self.target.frame(&self.gpu, |pass| {
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, group, &[]);
            pass.draw(0..3, 0..1);
        })
    }

    /// Renders a frame and waits for it (a still picture: paused, reduced
    /// motion, a seek).
    pub fn frame_now(&mut self, params: &StripParams) -> Result<Frame> {
        let uniforms = self.params_bytes(params);
        self.gpu.queue.write_buffer(&self.uniforms, 0, &uniforms);
        let (pipeline, group) = (&self.pipeline, &self.bind_group);
        self.target.frame_now(&self.gpu, |pass| {
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, group, &[]);
            pass.draw(0..3, 0..1);
        })
    }

    fn params_bytes(&self, p: &StripParams) -> Vec<u8> {
        let flag = |on: bool| if on { 1.0 } else { 0.0 };
        let (width, height) = self.target.size();
        let c = &p.colors;
        let seek = p.seek.unwrap_or_default();
        let play = p.play.unwrap_or_default();
        let (cover, corner) = p.cover.unwrap_or_default();
        let mut floats = vec![p.seconds, p.breath, p.kick, p.glow];
        floats.extend([
            width as f32,
            height as f32,
            flag(p.look == Look::Light),
            p.scale,
        ]);
        floats.extend([seek.left, seek.top, seek.right, seek.progress]);
        floats.extend([
            flag(seek.known),
            flag(self.has_wave),
            seek.hover,
            flag(p.seek.is_some()),
        ]);
        floats.extend([play[0], play[1], play[2], flag(p.play.is_some())]);
        floats.extend(cover);
        floats.extend([corner, flag(p.cover.is_some()), 0.0, 0.0]);
        for (rgb, w) in [
            (c.base, 1.0),
            (c.signal, 1.0),
            (c.ink, c.track),
            (c.accent, 1.0),
        ] {
            floats.extend(rgb);
            floats.push(w);
        }
        for colour in &p.palette {
            floats.extend(colour);
        }
        bytes(&floats)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Relative luminance of a BGRA pixel (sRGB transfer).
    fn luminance(px: &[u8]) -> f32 {
        let lin = |b: u8| {
            let c = f32::from(b) / 255.0;
            if c <= 0.04045 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        };
        0.0722 * lin(px[0]) + 0.7152 * lin(px[1]) + 0.2126 * lin(px[2])
    }

    fn params(look: Look, palette: [f32; 3]) -> StripParams {
        let base = match look {
            Look::Dark => [10.0 / 255.0, 10.0 / 255.0, 13.0 / 255.0],
            Look::Light => [239.0 / 255.0, 239.0 / 255.0, 242.0 / 255.0],
        };
        StripParams {
            seconds: 4.0,
            breath: 1.0,
            kick: 1.0,
            glow: 1.0,
            look,
            scale: 1.0,
            colors: StripColors {
                base,
                ..Default::default()
            },
            palette: [[palette[0], palette[1], palette[2], 1.0]; 4],
            ..Default::default()
        }
    }

    /// The glow alone, at full breath, under a vivid red, blue and white
    /// cover: the dark look stays dim enough for `text_faint` (luminance
    /// 0.216) to keep 4.5:1, and the light look never gets darker than the
    /// base, so the bar's text keeps the contrast it has without the glow.
    #[test]
    fn the_glow_keeps_the_bar_text_legible() {
        let gpu = match Gpu::new() {
            Ok(gpu) => gpu,
            Err(e) => {
                eprintln!("skipped, no GPU: {e:#}");
                return;
            }
        };
        let mut strip = Strip::new(&gpu, 320, 88);
        for colour in [[0.9, 0.1, 0.1], [0.1, 0.2, 0.95], [1.0, 1.0, 1.0]] {
            for look in [Look::Dark, Look::Light] {
                let frame = strip.frame_now(&params(look, colour)).expect("frame");
                let (lo, hi) = frame
                    .bgra
                    .chunks(4)
                    .map(luminance)
                    .fold((f32::MAX, 0.0f32), |(lo, hi), l| (lo.min(l), hi.max(l)));
                match look {
                    Look::Dark => assert!(hi <= 0.0091, "dark glow too bright: {hi}"),
                    Look::Light => assert!(lo >= 0.855, "light glow too dark: {lo}"),
                }
            }
        }
    }
}
