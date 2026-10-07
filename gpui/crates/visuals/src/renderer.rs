//! Our own wgpu device, rendering the backdrop shader offscreen and reading
//! each frame back as BGRA bytes (the format of GPUI's sprite atlas).
//!
//! GPUI (gpui-pre 0.3.8) has no public way to paint a texture of ours or to
//! share its device (see NOTES-visuals.md), so the frame crosses through
//! memory: render, copy to a mapped buffer, hand the app the bytes. Two
//! slots are in flight: a frame shows one frame after it was rendered, and
//! the CPU never waits for the GPU.
//!
//! Covers cross-fade: two cover textures are bound, and a new cover goes
//! into the older one while the shader mixes towards it over `FADE`.

use std::sync::mpsc;
use std::time::{Duration, Instant};

use anyhow::{Context as _, Result, anyhow};

use crate::cover::{COVER_SIZE, Cover};

/// Pixel format of the frames: what GPUI's atlas stores on Vulkan.
const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Bgra8Unorm;
/// How long a new cover takes to fade in.
const FADE: Duration = Duration::from_millis(1200);

/// Which look the backdrop is toned for: dark and dim under light text, or
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

/// One rendered frame, BGRA, `width * height * 4` bytes.
pub struct Frame {
    pub width: u32,
    pub height: u32,
    pub bgra: Vec<u8>,
    pub cost: FrameCost,
}

/// Where the time of one frame went, in milliseconds.
#[derive(Clone, Copy, Debug, Default)]
pub struct FrameCost {
    /// Encoding and submitting the render and the copy.
    pub submit: f32,
    /// Waiting for the previous frame's buffer to map.
    pub wait: f32,
    /// Copying the mapped bytes out.
    pub copy: f32,
}

type MapResult = mpsc::Receiver<Result<(), wgpu::BufferAsyncError>>;

struct Slot {
    texture: wgpu::Texture,
    buffer: wgpu::Buffer,
    /// Set while a copy into `buffer` is in flight; receives the map result.
    pending: Option<(wgpu::SubmissionIndex, MapResult)>,
}

pub struct Renderer {
    device: wgpu::Device,
    queue: wgpu::Queue,
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
    width: u32,
    height: u32,
    padded_row: u32,
    slots: [Slot; 2],
    next: usize,
    adapter: String,
}

impl Renderer {
    /// A device and pipeline drawing `width`×`height` frames.
    pub fn new(width: u32, height: u32) -> Result<Self> {
        let (device, queue, adapter) = device()?;
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("backdrop"),
            source: wgpu::ShaderSource::Wgsl(include_str!("../shaders/backdrop.wgsl").into()),
        });
        let layout = bind_group_layout(&device);
        let pipeline = pipeline(&device, &module, &layout);
        let uniforms = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("backdrop params"),
            size: PARAMS_SIZE,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("cover"),
            address_mode_u: wgpu::AddressMode::MirrorRepeat,
            address_mode_v: wgpu::AddressMode::MirrorRepeat,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let covers = [0, 1].map(|_| cover_texture(&device));
        let views = covers
            .each_ref()
            .map(|t| t.create_view(&wgpu::TextureViewDescriptor::default()));
        let bind_groups = [0, 1].map(|front| {
            bind_group(
                &device,
                &layout,
                &uniforms,
                &sampler,
                [&views[front], &views[1 - front]],
            )
        });
        let blank = Cover::blank();
        for texture in &covers {
            upload(&queue, texture, &blank.rgba);
        }
        let padded_row = padded_row(width);
        let slots = [0, 1].map(|_| slot(&device, width, height, padded_row));
        Ok(Self {
            device,
            queue,
            pipeline,
            uniforms,
            covers,
            bind_groups,
            front: 0,
            palettes: [blank.palette; 2],
            has_cover: false,
            faded_at: None,
            width,
            height,
            padded_row,
            slots,
            next: 0,
            adapter,
        })
    }

    /// The GPU and API in use, for the log.
    pub fn adapter(&self) -> &str {
        &self.adapter
    }

    pub fn size(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    /// Draws `width`×`height` frames from now on. Frames in flight are
    /// dropped, so the next [`Self::frame`] shows nothing yet.
    pub fn resize(&mut self, width: u32, height: u32) {
        if (width, height) == (self.width, self.height) {
            return;
        }
        self.width = width;
        self.height = height;
        self.padded_row = padded_row(width);
        self.slots = [0, 1].map(|_| slot(&self.device, width, height, self.padded_row));
        self.next = 0;
    }

    /// Shows a new cover: cross-faded over `FADE` when `fade`, at once
    /// otherwise (the first cover, reduced motion).
    pub fn set_cover(&mut self, cover: &Cover, fade: bool) {
        let back = 1 - self.front;
        upload(&self.queue, &self.covers[back], &cover.rgba);
        self.palettes[back] = cover.palette;
        if self.has_cover && fade {
            self.front = back;
            self.faded_at = Some(Instant::now());
        } else {
            upload(&self.queue, &self.covers[self.front], &cover.rgba);
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
        let mut cost = FrameCost::default();
        let started = Instant::now();
        let current = self.next;
        let previous = 1 - current;
        self.next = previous;
        self.submit(current, params);
        cost.submit = ms(started);

        let Some((index, done)) = self.slots[previous].pending.take() else {
            return Ok(None);
        };
        let waited = Instant::now();
        self.device
            .poll(wgpu::PollType::Wait {
                submission_index: Some(index),
                timeout: Some(Duration::from_secs(2)),
            })
            .map_err(|e| anyhow!("waiting for the visuals frame: {e}"))?;
        done.recv()
            .map_err(|_| anyhow!("map callback dropped"))?
            .map_err(|e| anyhow!("mapping the visuals frame: {e}"))?;
        cost.wait = ms(waited);

        let copied = Instant::now();
        let bgra = self.take_bytes(previous);
        cost.copy = ms(copied);
        Ok(Some(Frame {
            width: self.width,
            height: self.height,
            bgra,
            cost,
        }))
    }

    fn submit(&mut self, slot: usize, params: &FrameParams) {
        let bytes = self.params_bytes(params);
        self.queue.write_buffer(&self.uniforms, 0, &bytes);
        let view = self.slots[slot]
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("backdrop"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &self.bind_groups[self.front], &[]);
            pass.draw(0..3, 0..1);
        }
        let target = &self.slots[slot];
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &target.texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &target.buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(self.padded_row),
                    rows_per_image: Some(self.height),
                },
            },
            extent(self.width, self.height),
        );
        let index = self.queue.submit([encoder.finish()]);
        let (tx, rx) = mpsc::channel();
        target
            .buffer
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |result| {
                let _ = tx.send(result);
            });
        self.slots[slot].pending = Some((index, rx));
    }

    fn take_bytes(&self, slot: usize) -> Vec<u8> {
        let buffer = &self.slots[slot].buffer;
        let row = (self.width * 4) as usize;
        let mut bytes = Vec::with_capacity(row * self.height as usize);
        {
            let mapped = buffer.slice(..).get_mapped_range();
            for line in mapped.chunks(self.padded_row as usize) {
                bytes.extend_from_slice(&line[..row]);
            }
        }
        buffer.unmap();
        bytes
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
        let mut floats = vec![p.seconds, p.bass, p.kick, p.level];
        floats.extend([self.width as f32, self.height as f32, light, particles]);
        floats.extend([mix, has_cover, 0.0, 0.0]);
        let (new, old) = (self.palettes[self.front], self.palettes[1 - self.front]);
        for (n, o) in new.iter().zip(&old) {
            floats.extend((0..4).map(|i| o[i] + (n[i] - o[i]) * mix));
        }
        floats.iter().flat_map(|f| f.to_ne_bytes()).collect()
    }
}

/// Three vec4s and four palette colours.
const PARAMS_SIZE: u64 = 7 * 16;

fn device() -> Result<(wgpu::Device, wgpu::Queue, String)> {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends: wgpu::Backends::VULKAN | wgpu::Backends::GL,
        flags: wgpu::InstanceFlags::default(),
        backend_options: wgpu::BackendOptions::default(),
        memory_budget_thresholds: wgpu::MemoryBudgetThresholds::default(),
        display: None,
    });
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::LowPower,
        compatible_surface: None,
        force_fallback_adapter: false,
    }))
    .map_err(|e| anyhow!("no GPU adapter: {e}"))?;
    let info = adapter.get_info();
    let (device, queue) = pollster::block_on(
        adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("ytfast visuals"),
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits::downlevel_defaults()
                .using_resolution(adapter.limits())
                .using_alignment(adapter.limits()),
            memory_hints: wgpu::MemoryHints::MemoryUsage,
            trace: wgpu::Trace::Off,
            experimental_features: wgpu::ExperimentalFeatures::disabled(),
        }),
    )
    .context("creating the visuals device")?;
    Ok((device, queue, format!("{} ({:?})", info.name, info.backend)))
}

fn ms(since: Instant) -> f32 {
    since.elapsed().as_secs_f32() * 1000.0
}

fn padded_row(width: u32) -> u32 {
    (width * 4).next_multiple_of(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT)
}

fn extent(width: u32, height: u32) -> wgpu::Extent3d {
    wgpu::Extent3d {
        width,
        height,
        depth_or_array_layers: 1,
    }
}

fn slot(device: &wgpu::Device, width: u32, height: u32, padded_row: u32) -> Slot {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("backdrop frame"),
        size: extent(width, height),
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("backdrop readback"),
        size: u64::from(padded_row) * u64::from(height),
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    Slot {
        texture,
        buffer,
        pending: None,
    }
}

fn cover_texture(device: &wgpu::Device) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some("cover"),
        size: extent(COVER_SIZE, COVER_SIZE),
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    })
}

fn upload(queue: &wgpu::Queue, texture: &wgpu::Texture, rgba: &[u8]) {
    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        rgba,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(COVER_SIZE * 4),
            rows_per_image: Some(COVER_SIZE),
        },
        extent(COVER_SIZE, COVER_SIZE),
    );
}

fn bind_group_layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
    let fragment = wgpu::ShaderStages::FRAGMENT;
    let texture = |binding| wgpu::BindGroupLayoutEntry {
        binding,
        visibility: fragment,
        ty: wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Float { filterable: true },
            view_dimension: wgpu::TextureViewDimension::D2,
            multisampled: false,
        },
        count: None,
    };
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("backdrop"),
        entries: &[
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: fragment,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            },
            texture(1),
            texture(2),
            wgpu::BindGroupLayoutEntry {
                binding: 3,
                visibility: fragment,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                count: None,
            },
        ],
    })
}

fn pipeline(
    device: &wgpu::Device,
    module: &wgpu::ShaderModule,
    layout: &wgpu::BindGroupLayout,
) -> wgpu::RenderPipeline {
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("backdrop"),
        bind_group_layouts: &[Some(layout)],
        immediate_size: 0,
    });
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("backdrop"),
        layout: Some(&pipeline_layout),
        vertex: wgpu::VertexState {
            module,
            entry_point: Some("vs_main"),
            buffers: &[],
            compilation_options: wgpu::PipelineCompilationOptions::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module,
            entry_point: Some("fs_main"),
            targets: &[Some(wgpu::ColorTargetState {
                format: FORMAT,
                blend: None,
                write_mask: wgpu::ColorWrites::ALL,
            })],
            compilation_options: wgpu::PipelineCompilationOptions::default(),
        }),
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        multiview_mask: None,
        cache: None,
    })
}

/// `covers[0]` is the new cover, `covers[1]` the old one.
fn bind_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    uniforms: &wgpu::Buffer,
    sampler: &wgpu::Sampler,
    covers: [&wgpu::TextureView; 2],
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("backdrop"),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: uniforms.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::TextureView(covers[0]),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: wgpu::BindingResource::TextureView(covers[1]),
            },
            wgpu::BindGroupEntry {
                binding: 3,
                resource: wgpu::BindingResource::Sampler(sampler),
            },
        ],
    })
}

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
        let mut renderer = match Renderer::new(64, 36) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("skipped, no GPU: {e:#}");
                return;
            }
        };
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
