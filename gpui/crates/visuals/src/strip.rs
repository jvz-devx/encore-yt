//! The player bar strip: `shaders/strip.wgsl` draws the bar's whole
//! background in one pass (palette glow, seek bar with the song's
//! waveform, beat halos) and reads it back like the backdrop.

use anyhow::Result;

use crate::gpu::{Gpu, bytes};
use crate::renderer::Look;
use crate::target::{Frame, Target};

/// The waveform and heat textures' width: the song's outline and replay
/// heat resampled to this.
const WAVE: u32 = 512;
/// Seventeen vec4s, as `strip.wgsl`'s `Params`.
const PARAMS_SIZE: u64 = 17 * 16;

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
    /// The most-replayed ridge's height in points where the heat is
    /// greatest, while the song has heat ([`Strip::set_heat`]).
    pub ridge: Option<f32>,
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
    /// The ridge ahead of the playhead.
    pub muted: [f32; 3],
}

pub struct Strip {
    gpu: Gpu,
    pipeline: wgpu::RenderPipeline,
    uniforms: wgpu::Buffer,
    wave: wgpu::Texture,
    heat: wgpu::Texture,
    bind_group: wgpu::BindGroup,
    has_wave: bool,
    has_heat: bool,
    target: Target,
}

impl Strip {
    pub fn new(gpu: &Gpu, width: u32, height: u32) -> Self {
        let module = gpu.shader("strip", include_str!("../shaders/strip.wgsl"));
        let layout = gpu.layout("strip", 2);
        let pipeline = gpu.pipeline("strip", &module, &layout);
        let uniforms = gpu.uniforms("strip params", PARAMS_SIZE);
        let sampler = gpu.sampler(wgpu::AddressMode::ClampToEdge);
        let wave = gpu.texture("waveform", (WAVE, 1), wgpu::TextureFormat::R8Unorm);
        let heat = gpu.texture("heat", (WAVE, 1), wgpu::TextureFormat::R8Unorm);
        let views = [&wave, &heat].map(|t| t.create_view(&wgpu::TextureViewDescriptor::default()));
        let bind_group = gpu.bind_group(&layout, &uniforms, &[&views[0], &views[1]], &sampler);
        Self {
            gpu: gpu.clone(),
            pipeline,
            uniforms,
            wave,
            heat,
            bind_group,
            has_wave: false,
            has_heat: false,
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
        self.has_wave = values.is_some_and(|v| !v.is_empty());
        if let Some(values) = values.filter(|_| self.has_wave) {
            self.gpu.upload(&self.wave, 1, &texels(values));
        }
    }

    /// The song's replay heat (0..1 values evenly over the song, any
    /// count), or `None` for no ridge.
    pub fn set_heat(&mut self, values: Option<&[f32]>) {
        self.has_heat = values.is_some_and(|v| !v.is_empty());
        if let Some(values) = values.filter(|_| self.has_heat) {
            self.gpu.upload(&self.heat, 1, &texels(values));
        }
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
        let ridge = p.ridge.filter(|_| self.has_heat);
        floats.extend([flag(ridge.is_some()), ridge.unwrap_or(0.0), 0.0, 0.0]);
        let base = oklab(to_linear(c.base));
        floats.extend(base);
        floats.push(luminance(oklab_to_linear(base)));
        let halo = halo(c.accent, p.look == Look::Light);
        for (rgb, w) in [
            (c.signal, 1.0),
            (c.ink, c.track),
            (halo, 1.0),
            (c.muted, 1.0),
        ] {
            floats.extend(rgb);
            floats.push(w);
        }
        for colour in &p.palette {
            let lab = oklab(to_linear([colour[0], colour[1], colour[2]]));
            floats.extend([lab[1], lab[2], 0.0, 0.0]);
        }
        bytes(&floats)
    }
}

/// The accent made into the halos' colour (linear RGB): vivid and light
/// in the dark look, a deeper tone in the light one.
fn halo(accent: [f32; 3], light: bool) -> [f32; 3] {
    let lab = oklab(to_linear(accent));
    let c = lab[1].hypot(lab[2]);
    let hue = if c > 1e-4 {
        [lab[1] / c, lab[2] / c]
    } else {
        [1.0, 0.0]
    };
    let (l, c) = if light {
        (0.66, c.max(0.1))
    } else {
        (0.74, c.clamp(0.09, 0.16))
    };
    oklab_to_linear([l, hue[0] * c, hue[1] * c])
}

fn to_linear(c: [f32; 3]) -> [f32; 3] {
    c.map(|v| {
        if v <= 0.04045 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    })
}

fn luminance(c: [f32; 3]) -> f32 {
    0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2]
}

/// Linear sRGB to OKLab, as `strip.wgsl` converts.
fn oklab(c: [f32; 3]) -> [f32; 3] {
    let l = 0.412_221_46 * c[0] + 0.536_332_55 * c[1] + 0.051_445_995 * c[2];
    let m = 0.211_903_5 * c[0] + 0.680_699_5 * c[1] + 0.107_396_96 * c[2];
    let s = 0.088_302_46 * c[0] + 0.281_718_85 * c[1] + 0.629_978_7 * c[2];
    let [l, m, s] = [l, m, s].map(|v| v.max(0.0).cbrt());
    [
        0.210_454_26 * l + 0.793_617_8 * m - 0.004_072_047 * s,
        1.977_998_5 * l - 2.428_592_2 * m + 0.450_593_7 * s,
        0.025_904_037 * l + 0.782_771_77 * m - 0.808_675_77 * s,
    ]
}

fn oklab_to_linear(lab: [f32; 3]) -> [f32; 3] {
    let l = lab[0] + 0.396_337_78 * lab[1] + 0.215_803_76 * lab[2];
    let m = lab[0] - 0.105_561_346 * lab[1] - 0.063_854_17 * lab[2];
    let s = lab[0] - 0.089_484_18 * lab[1] - 1.291_485_5 * lab[2];
    let [l, m, s] = [l, m, s].map(|v| v * v * v);
    [
        4.076_741_7 * l - 3.307_711_6 * m + 0.230_969_94 * s,
        -1.268_438 * l + 2.609_757_4 * m - 0.341_319_38 * s,
        -0.004_196_086_3 * l - 0.703_418_6 * m + 1.707_614_7 * s,
    ]
}

/// `values` resampled linearly to `WAVE` bytes.
fn texels(values: &[f32]) -> Vec<u8> {
    (0..WAVE)
        .map(|i| {
            let at = (i as f32 + 0.5) / WAVE as f32 * values.len() as f32 - 0.5;
            let lo = at.floor().clamp(0.0, (values.len() - 1) as f32) as usize;
            let hi = (lo + 1).min(values.len() - 1);
            let f = (at - lo as f32).clamp(0.0, 1.0);
            let v = values[lo] + (values[hi] - values[lo]) * f;
            (v.clamp(0.0, 1.0) * 255.0).round() as u8
        })
        .collect()
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
