//! The Now Playing backdrop: `shaders/backdrop.wgsl` drawn offscreen on the
//! shared [`Gpu`] and read back as BGRA bytes (see [`crate::target`]).
//!
//! Covers cross-fade: two cover textures are bound, and a new cover goes
//! into the older one while the shader mixes towards it over `FADE`.

use std::time::{Duration, Instant};

use anyhow::Result;

use crate::cover::{COVER_SIZE, Cover};
use crate::gpu::{Gpu, bytes};
use crate::target::{Frame, Target};

/// How long a new cover takes to fade in.
const FADE: Duration = Duration::from_millis(1200);

/// Which look an effect is toned for: dark and dim under light text, or
/// light and pastel under dark text.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Look {
    #[default]
    Dark,
    Light,
}

/// Inputs for one frame.
#[derive(Clone, Copy, Debug, Default)]
pub struct FrameParams {
    /// Animation time in seconds; it stands still while paused.
    pub seconds: f32,
    /// Low-frequency level, 0..1.
    pub bass: f32,
    /// A short pulse on each beat, 0..1.
    pub kick: f32,
    /// Overall level, 0..1.
    pub level: f32,
    pub look: Look,
    /// Floating motes; off under reduced motion.
    pub particles: bool,
}

pub struct Renderer {
    gpu: Gpu,
    pipeline: wgpu::RenderPipeline,
    uniforms: wgpu::Buffer,
    covers: [wgpu::Texture; 2],
    /// Bind groups with cover `i` as the new one and the other as the old.
    bind_groups: [wgpu::BindGroup; 2],
    /// The cover texture shown (or fading in).
    front: usize,
    palettes: [[[f32; 4]; 4]; 2],
    has_cover: bool,
    faded_at: Option<Instant>,
    target: Target,
}

impl Renderer {
    /// A pipeline on `gpu` drawing `width`×`height` frames.
    pub fn new(gpu: &Gpu, width: u32, height: u32) -> Self {
        let module = gpu.shader("backdrop", include_str!("../shaders/backdrop.wgsl"));
        let layout = gpu.layout("backdrop", 2);
        let pipeline = gpu.pipeline("backdrop", &module, &layout);
        let uniforms = gpu.uniforms("backdrop params", PARAMS_SIZE);
        let sampler = gpu.sampler(wgpu::AddressMode::MirrorRepeat);
        let covers = [0, 1].map(|_| {
            gpu.texture(
                "cover",
                (COVER_SIZE, COVER_SIZE),
                wgpu::TextureFormat::Rgba8Unorm,
            )
        });
        let views = covers
            .each_ref()
            .map(|t| t.create_view(&wgpu::TextureViewDescriptor::default()));
        let bind_groups = [0, 1].map(|front| {
            gpu.bind_group(
                &layout,
                &uniforms,
                &[&views[front], &views[1 - front]],
                &sampler,
            )
        });
        let blank = Cover::blank();
        for texture in &covers {
            gpu.upload(texture, 4, &blank.rgba);
        }
        Self {
            gpu: gpu.clone(),
            pipeline,
            uniforms,
            covers,
            bind_groups,
            front: 0,
            palettes: [blank.palette; 2],
            has_cover: false,
            faded_at: None,
            target: Target::new(gpu, width, height),
        }
    }

    pub fn size(&self) -> (u32, u32) {
        self.target.size()
    }

    /// Draws `width`×`height` frames from now on. Frames in flight are
    /// dropped, so the next [`Self::frame`] shows nothing yet.
    pub fn resize(&mut self, width: u32, height: u32) {
        self.target.resize(&self.gpu, width, height);
    }

    /// Shows a new cover: cross-faded over `FADE` when `fade`, at once
    /// otherwise (the first cover, reduced motion).
    pub fn set_cover(&mut self, cover: &Cover, fade: bool) {
        let back = 1 - self.front;
        self.gpu.upload(&self.covers[back], 4, &cover.rgba);
        self.palettes[back] = cover.palette;
        if self.has_cover && fade {
            self.front = back;
            self.faded_at = Some(Instant::now());
        } else {
            self.gpu.upload(&self.covers[self.front], 4, &cover.rgba);
            self.palettes[self.front] = cover.palette;
            self.faded_at = None;
            self.has_cover = true;
        }
    }

    /// A cover is still fading in: frames should keep coming even while
    /// paused.
    pub fn fading(&self) -> bool {
        self.faded_at.is_some_and(|at| at.elapsed() < FADE)
    }

    /// Renders a frame and returns the one rendered on the previous call.
    /// The first call after `new` or `resize` has nothing to show yet.
    pub fn frame(&mut self, params: &FrameParams) -> Result<Option<Frame>> {
        let uniforms = self.params_bytes(params);
        self.gpu.queue.write_buffer(&self.uniforms, 0, &uniforms);
        let (pipeline, group) = (&self.pipeline, &self.bind_groups[self.front]);
        self.target.frame(&self.gpu, |pass| {
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, group, &[]);
            pass.draw(0..3, 0..1);
        })
    }

    /// The uniform block, as `backdrop.wgsl`'s `Params` lays it out.
    fn params_bytes(&self, p: &FrameParams) -> Vec<u8> {
        let mix = self.faded_at.map_or(1.0, |at| {
            (at.elapsed().as_secs_f32() / FADE.as_secs_f32()).min(1.0)
        });
        let mix = mix * mix * (3.0 - 2.0 * mix);
        let light = if p.look == Look::Light { 1.0 } else { 0.0 };
        let particles = if p.particles { 1.0 } else { 0.0 };
        let has_cover = if self.has_cover { 1.0 } else { 0.0 };
        let (width, height) = self.target.size();
        let mut floats = vec![p.seconds, p.bass, p.kick, p.level];
        floats.extend([width as f32, height as f32, light, particles]);
        floats.extend([mix, has_cover, 0.0, 0.0]);
        let (new, old) = (self.palettes[self.front], self.palettes[1 - self.front]);
        for (n, o) in new.iter().zip(&old) {
            floats.extend((0..4).map(|i| o[i] + (n[i] - o[i]) * mix));
        }
        bytes(&floats)
    }
}

/// Three vec4s and four palette colours.
const PARAMS_SIZE: u64 = 7 * 16;

#[cfg(test)]
mod tests {
    use super::*;

    /// Relative luminance of a BGRA pixel (sRGB, approximate gamma 2.2).
    fn luminance(px: &[u8]) -> f32 {
        let lin = |b: u8| (f32::from(b) / 255.0).powf(2.2);
        0.0722 * lin(px[0]) + 0.7152 * lin(px[1]) + 0.2126 * lin(px[2])
    }

    /// Renders a white and a deep blue cover in both looks and checks the
    /// promise the app makes to its text: the dark look stays dim enough
    /// and the light look light enough for `text_muted` on top (4.5:1).
    #[test]
    fn frames_stay_in_the_text_safe_range() {
        let gpu = match Gpu::new() {
            Ok(gpu) => gpu,
            Err(e) => {
                eprintln!("skipped, no GPU: {e:#}");
                return;
            }
        };
        let mut renderer = Renderer::new(&gpu, 64, 36);
        for bgra in [[255u8, 255, 255, 255], [120, 20, 10, 255]] {
            let cover = bgra.repeat(32 * 32);
            renderer.set_cover(&Cover::from_bgra(32, 32, &cover), false);
            for look in [Look::Dark, Look::Light] {
                let (min, max) = luminance_range(&mut renderer, look);
                match look {
                    Look::Dark => assert!(max < 0.05, "dark look too bright: {max}"),
                    Look::Light => assert!(min > 0.63, "light look too dark: {min}"),
                }
            }
        }
    }

    /// The darkest and brightest pixel of a loud frame. Particles are left
    /// out: a mote is a few pixels that drift past, brighter on purpose.
    fn luminance_range(renderer: &mut Renderer, look: Look) -> (f32, f32) {
        let params = FrameParams {
            seconds: 3.0,
            bass: 1.0,
            kick: 1.0,
            level: 1.0,
            look,
            particles: false,
        };
        renderer.frame(&params).expect("first frame");
        let frame = renderer.frame(&params).expect("frame").expect("a frame");
        assert_eq!(frame.bgra.len(), 64 * 36 * 4);
        frame
            .bgra
            .chunks(4)
            .map(luminance)
            .fold((f32::MAX, 0.0f32), |(lo, hi), l| (lo.min(l), hi.max(l)))
    }
}
