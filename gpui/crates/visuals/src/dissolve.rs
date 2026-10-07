//! A cover changing on a track change: `shaders/dissolve.wgsl` burns the
//! old cover into the new one along a noise front, drawn at the size the
//! cover shows and read back like the other effects.

use anyhow::Result;

use crate::gpu::{Gpu, bytes};
use crate::renderer::Look;
use crate::target::{Frame, Target};

/// A decoded image as GPUI keeps it: width, height and BGRA bytes.
pub type Pixels<'a> = (u32, u32, &'a [u8]);

pub struct Dissolve {
    gpu: Gpu,
    layout: wgpu::BindGroupLayout,
    pipeline: wgpu::RenderPipeline,
    uniforms: wgpu::Buffer,
    sampler: wgpu::Sampler,
    bind_group: Option<wgpu::BindGroup>,
    /// The share of each cover's width and height that shows.
    crop: [f32; 4],
    target: Target,
}

impl Dissolve {
    pub fn new(gpu: &Gpu, width: u32, height: u32) -> Self {
        let module = gpu.shader("dissolve", include_str!("../shaders/dissolve.wgsl"));
        let layout = gpu.layout("dissolve", 2);
        let pipeline = gpu.pipeline("dissolve", &module, &layout);
        Self {
            gpu: gpu.clone(),
            uniforms: gpu.uniforms("dissolve params", 48),
            sampler: gpu.sampler(wgpu::AddressMode::ClampToEdge),
            layout,
            pipeline,
            bind_group: None,
            crop: [1.0; 4],
            target: Target::new(gpu, width, height),
        }
    }

    pub fn size(&self) -> (u32, u32) {
        self.target.size()
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        self.target.resize(&self.gpu, width, height);
    }

    /// Starts a dissolve between two covers. Any frame still in flight is
    /// dropped, so the first [`Self::frame`] after this shows nothing yet.
    pub fn set_covers(&mut self, old: Pixels<'_>, new: Pixels<'_>) {
        let (width, height) = self.target.size();
        let aspect = width as f32 / height.max(1) as f32;
        let crop = |(w, h, _): Pixels<'_>| {
            let image = w.max(1) as f32 / h.max(1) as f32;
            if image > aspect {
                [aspect / image, 1.0]
            } else {
                [1.0, image / aspect]
            }
        };
        let (o, n) = (crop(old), crop(new));
        self.crop = [o[0], o[1], n[0], n[1]];
        let views = [old, new].map(|(width, height, bgra)| {
            let size = (width.max(1), height.max(1));
            let texture = self
                .gpu
                .texture("dissolve cover", size, wgpu::TextureFormat::Bgra8Unorm);
            if bgra.len() >= (size.0 * size.1 * 4) as usize {
                self.gpu
                    .upload(&texture, 4, &bgra[..(size.0 * size.1 * 4) as usize]);
            }
            texture.create_view(&wgpu::TextureViewDescriptor::default())
        });
        self.bind_group = Some(self.gpu.bind_group(
            &self.layout,
            &self.uniforms,
            &[&views[0], &views[1]],
            &self.sampler,
        ));
        self.target.discard();
    }

    /// Renders the dissolve at `progress` (0..1) and returns the frame
    /// rendered on the previous call. `accent` colours the burning edge.
    pub fn frame(&mut self, progress: f32, look: Look, accent: [f32; 3]) -> Result<Option<Frame>> {
        let Some(group) = &self.bind_group else {
            return Ok(None);
        };
        let (width, height) = self.target.size();
        let light = if look == Look::Light { 1.0 } else { 0.0 };
        let floats = [
            progress,
            light,
            width as f32,
            height as f32,
            accent[0],
            accent[1],
            accent[2],
            1.0,
            self.crop[0],
            self.crop[1],
            self.crop[2],
            self.crop[3],
        ];
        self.gpu
            .queue
            .write_buffer(&self.uniforms, 0, &bytes(&floats));
        let pipeline = &self.pipeline;
        self.target.frame(&self.gpu, |pass| {
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, group, &[]);
            pass.draw(0..3, 0..1);
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Black to white: nothing has burnt at 0, everything at 1, and half
    /// way part of the cover is new, part old.
    #[test]
    fn the_front_runs_from_old_to_new() {
        let gpu = match Gpu::new() {
            Ok(gpu) => gpu,
            Err(e) => {
                eprintln!("skipped, no GPU: {e:#}");
                return;
            }
        };
        let mut dissolve = Dissolve::new(&gpu, 32, 32);
        let black = [0u8, 0, 0, 255].repeat(16 * 16);
        let white = [255u8; 4].repeat(16 * 16);
        dissolve.set_covers((16, 16, &black), (16, 16, &white));
        let mean = |dissolve: &mut Dissolve, t: f32| {
            dissolve
                .frame(t, Look::Dark, [0.0, 0.0, 0.0])
                .expect("frame");
            let frame = dissolve
                .frame(t, Look::Dark, [0.0, 0.0, 0.0])
                .expect("frame")
                .expect("a frame");
            frame.bgra.iter().map(|&b| f32::from(b)).sum::<f32>() / frame.bgra.len() as f32
        };
        // Alpha is 255 everywhere: a black frame averages 63.75.
        assert!(mean(&mut dissolve, 0.0) < 64.5);
        assert!(mean(&mut dissolve, 1.0) > 254.0);
        let half = mean(&mut dissolve, 0.5);
        assert!(half > 90.0 && half < 230.0, "half way: {half}");
    }
}
