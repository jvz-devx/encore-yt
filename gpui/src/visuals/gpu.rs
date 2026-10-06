//! Our own wgpu device, rendering the backdrop shader offscreen and reading
//! each frame back as a GPUI `RenderImage` (BGRA, the atlas format).
//!
//! GPUI (gpui-pre 0.3.8) has no public way to paint a texture of ours or to
//! share its device (see NOTES-visuals.md), so the frame crosses through
//! memory: render, copy to a mapped buffer, hand GPUI the bytes, which it
//! uploads into its sprite atlas. Two slots are in flight: a frame shows
//! one frame after it was rendered, and the CPU never waits for the GPU.

use std::sync::Arc;
use std::sync::mpsc;
use std::time::Instant;

use anyhow::{Context as _, Result, anyhow};
use gpui_kit::RenderImage;
use image::{Frame, RgbaImage};
use wgpu::util::DeviceExt as _;

/// Pixel format of the frames: what GPUI's atlas stores on Vulkan.
const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Bgra8Unorm;
/// The cover is reduced to this many pixels a side before upload: the
/// shader blurs it anyway.
pub const COVER_SIZE: u32 = 48;

/// Inputs for one frame.
pub struct FrameParams {
    pub seconds: f32,
    pub bass: f32,
    pub level: f32,
    pub palette: [[f32; 4]; 4],
}

/// Where the time of one frame went, in milliseconds.
#[derive(Clone, Copy, Default)]
pub struct FrameCost {
    /// Encoding and submitting the render and the copy.
    pub submit: f32,
    /// Waiting for the previous frame's buffer to map.
    pub wait: f32,
    /// Copying the mapped bytes into a new `RenderImage`.
    pub copy: f32,
}

struct Slot {
    texture: wgpu::Texture,
    buffer: wgpu::Buffer,
    /// Set while a copy into `buffer` is in flight; receives the map result.
    pending: Option<(
        wgpu::SubmissionIndex,
        mpsc::Receiver<Result<(), wgpu::BufferAsyncError>>,
    )>,
}

pub struct Backdrop {
    device: wgpu::Device,
    queue: wgpu::Queue,
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    uniforms: wgpu::Buffer,
    sampler: wgpu::Sampler,
    bind_group: wgpu::BindGroup,
    has_cover: bool,
    width: u32,
    height: u32,
    padded_row: u32,
    slots: [Slot; 2],
    next: usize,
    pub adapter: String,
}

impl Backdrop {
    pub fn new(width: u32, height: u32) -> Result<Self> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::VULKAN | wgpu::Backends::GL,
            flags: wgpu::InstanceFlags::default(),
            backend_options: wgpu::BackendOptions::default(),
            memory_budget_thresholds: wgpu::MemoryBudgetThresholds::default(),
            display: None,
        });
        let adapter = smol::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::LowPower,
            compatible_surface: None,
            force_fallback_adapter: false,
        }))
        .map_err(|e| anyhow!("no GPU adapter: {e}"))?;
        let info = adapter.get_info();
        let (device, queue) = smol::block_on(
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
        let (pipeline, layout) = pipeline(&device);
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
        let blank = vec![0u8; (COVER_SIZE * COVER_SIZE * 4) as usize];
        let bind_group = cover_bind_group(&device, &queue, &layout, &uniforms, &sampler, &blank);
        let padded_row = (width * 4).next_multiple_of(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);
        let slots = [0, 1].map(|_| slot(&device, width, height, padded_row));
        Ok(Self {
            device,
            queue,
            pipeline,
            layout,
            uniforms,
            sampler,
            bind_group,
            has_cover: false,
            width,
            height,
            padded_row,
            slots,
            next: 0,
            adapter: format!("{} ({:?})", info.name, info.backend),
        })
    }

    /// Replaces the cover: `rgba` is `COVER_SIZE`² RGBA pixels.
    pub fn set_cover(&mut self, rgba: &[u8]) {
        self.bind_group = cover_bind_group(
            &self.device,
            &self.queue,
            &self.layout,
            &self.uniforms,
            &self.sampler,
            rgba,
        );
        self.has_cover = true;
    }

    /// Renders a frame and returns the one rendered on the previous call,
    /// with where the time went. The first call has nothing to show yet.
    pub fn frame(&mut self, params: &FrameParams) -> Result<Option<(Arc<RenderImage>, FrameCost)>> {
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
                timeout: Some(std::time::Duration::from_secs(2)),
            })
            .map_err(|e| anyhow!("waiting for the visuals frame: {e}"))?;
        done.recv()
            .map_err(|_| anyhow!("map callback dropped"))?
            .map_err(|e| anyhow!("mapping the visuals frame: {e}"))?;
        cost.wait = ms(waited);

        let copied = Instant::now();
        let image = self.take_image(previous);
        cost.copy = ms(copied);
        Ok(Some((image, cost)))
    }

    fn submit(&mut self, slot: usize, params: &FrameParams) {
        self.queue
            .write_buffer(&self.uniforms, 0, &params_bytes(params, self));
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
            pass.set_bind_group(0, &self.bind_group, &[]);
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

    fn take_image(&self, slot: usize) -> Arc<RenderImage> {
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
        let pixels = RgbaImage::from_raw(self.width, self.height, bytes).expect("sized buffer");
        Arc::new(RenderImage::new([Frame::new(pixels)]))
    }
}

fn ms(since: Instant) -> f32 {
    since.elapsed().as_secs_f32() * 1000.0
}

fn extent(width: u32, height: u32) -> wgpu::Extent3d {
    wgpu::Extent3d {
        width,
        height,
        depth_or_array_layers: 1,
    }
}

/// time, size, four palette colours: six vec4s.
const PARAMS_SIZE: u64 = 6 * 16;

fn params_bytes(params: &FrameParams, backdrop: &Backdrop) -> Vec<u8> {
    let has_cover = if backdrop.has_cover { 1.0 } else { 0.0 };
    let mut floats = vec![params.seconds, params.bass, params.level, has_cover];
    floats.extend([backdrop.width as f32, backdrop.height as f32, 0.0, 0.0]);
    for color in params.palette {
        floats.extend(color);
    }
    floats.iter().flat_map(|f| f.to_ne_bytes()).collect()
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

fn pipeline(device: &wgpu::Device) -> (wgpu::RenderPipeline, wgpu::BindGroupLayout) {
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("backdrop"),
        source: wgpu::ShaderSource::Wgsl(include_str!("backdrop.wgsl").into()),
    });
    let fragment = wgpu::ShaderStages::FRAGMENT;
    let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
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
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: fragment,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 2,
                visibility: fragment,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                count: None,
            },
        ],
    });
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("backdrop"),
        bind_group_layouts: &[Some(&layout)],
        immediate_size: 0,
    });
    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("backdrop"),
        layout: Some(&pipeline_layout),
        vertex: wgpu::VertexState {
            module: &module,
            entry_point: Some("vs_main"),
            buffers: &[],
            compilation_options: wgpu::PipelineCompilationOptions::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: &module,
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
    });
    (pipeline, layout)
}

fn cover_bind_group(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    layout: &wgpu::BindGroupLayout,
    uniforms: &wgpu::Buffer,
    sampler: &wgpu::Sampler,
    rgba: &[u8],
) -> wgpu::BindGroup {
    let texture = device.create_texture_with_data(
        queue,
        &wgpu::TextureDescriptor {
            label: Some("cover"),
            size: extent(COVER_SIZE, COVER_SIZE),
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        },
        wgpu::util::TextureDataOrder::LayerMajor,
        rgba,
    );
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
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
                resource: wgpu::BindingResource::TextureView(&view),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: wgpu::BindingResource::Sampler(sampler),
            },
        ],
    })
}
