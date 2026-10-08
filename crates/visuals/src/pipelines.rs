//! Every effect's pipeline, compiled once with the device ([`Gpu::new`]),
//! so an effect's first frame doesn't wait for a shader compile.
//!
//! On Vulkan the compiled pipelines go through a wgpu `PipelineCache` kept
//! on disk: a file per GPU and driver in the app's cache directory, loaded
//! before the compile and written back when it grew. Elsewhere (the GL
//! fallback; wgpu has pipeline caches only on Vulkan) the driver's own
//! cache is all there is.
//!
//! [`Gpu::new`]: crate::Gpu::new

use std::time::Instant;

use crate::gpu::{FORMAT, ms};

/// A full-screen-triangle effect: its pipeline and the layout of its bind
/// group.
#[derive(Clone)]
pub(crate) struct Effect {
    pub layout: wgpu::BindGroupLayout,
    pub pipeline: wgpu::RenderPipeline,
}

#[derive(Clone)]
pub(crate) struct Pipelines {
    pub backdrop: Effect,
    pub strip: Effect,
    pub dissolve: Effect,
    pub visualizer: Effect,
    pub scenes: Scenes,
}

/// The 3D scenes (M30, [`crate::Scene`]): one bind group layout (the
/// uniforms, read by vertex and fragment stages), and each scene's draws in
/// order.
#[derive(Clone)]
pub(crate) struct Scenes {
    pub layout: wgpu::BindGroupLayout,
    /// The gradient, the wave mesh (alpha blended), the sparkles (added).
    pub xmb: [wgpu::RenderPipeline; 3],
    pub ridges: wgpu::RenderPipeline,
    pub aurora: wgpu::RenderPipeline,
}

impl Pipelines {
    /// Compiles every effect on `device`, through `cache` when there is
    /// one.
    pub fn new(device: &wgpu::Device, cache: Option<&wgpu::PipelineCache>) -> Self {
        let effect = |label, source, textures| {
            let module = shader(device, label, source);
            let layout = layout(device, label, textures);
            let pipeline = pipeline(device, label, &module, &layout, cache);
            Effect { layout, pipeline }
        };
        Self {
            backdrop: effect("backdrop", include_str!("../shaders/backdrop.wgsl"), 2),
            strip: effect("strip", include_str!("../shaders/strip.wgsl"), 2),
            dissolve: effect("dissolve", include_str!("../shaders/dissolve.wgsl"), 2),
            // The spectrum is in its uniforms: no textures.
            visualizer: effect("visualizer", include_str!("../shaders/visualizer.wgsl"), 0),
            scenes: Scenes::new(device, cache),
        }
    }
}

impl Scenes {
    fn new(device: &wgpu::Device, cache: Option<&wgpu::PipelineCache>) -> Self {
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("scenes"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        // Each scene follows the shared part (parameters, noise, tone
        // mapping) in one module.
        let common = include_str!("../shaders/scene_common.wgsl");
        let module = |label, scene: &str| shader(device, label, format!("{common}\n{scene}"));
        let xmb = module("scene xmb", include_str!("../shaders/scene_xmb.wgsl"));
        let ridges = module("scene ridges", include_str!("../shaders/scene_ridges.wgsl"));
        let aurora = module("scene aurora", include_str!("../shaders/scene_aurora.wgsl"));
        let draw = |label, module, vs, fs, blend| {
            pass(device, label, module, &layout, cache, vs, fs, blend)
        };
        let alpha = Some(wgpu::BlendState::ALPHA_BLENDING);
        let add = Some(wgpu::BlendState {
            color: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::One,
                dst_factor: wgpu::BlendFactor::One,
                operation: wgpu::BlendOperation::Add,
            },
            alpha: wgpu::BlendComponent::OVER,
        });
        Self {
            xmb: [
                draw("scene xmb", &xmb, "vs_main", "fs_main", None),
                draw("scene xmb wave", &xmb, "vs_wave", "fs_wave", alpha),
                draw("scene xmb sparkles", &xmb, "vs_sparkle", "fs_sparkle", add),
            ],
            ridges: draw("scene ridges", &ridges, "vs_main", "fs_main", None),
            aurora: draw("scene aurora", &aurora, "vs_main", "fs_main", None),
            layout,
        }
    }
}

fn shader(
    device: &wgpu::Device,
    label: &str,
    source: impl Into<std::borrow::Cow<'static, str>>,
) -> wgpu::ShaderModule {
    let started = Instant::now();
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some(label),
        source: wgpu::ShaderSource::Wgsl(source.into()),
    });
    log::info!("visuals: {label} shader {:.1} ms", ms(started));
    module
}

/// Binding 0 the uniforms, 1..=`textures` the textures, then the sampler.
fn layout(device: &wgpu::Device, label: &str, textures: u32) -> wgpu::BindGroupLayout {
    let fragment = wgpu::ShaderStages::FRAGMENT;
    let mut entries = vec![wgpu::BindGroupLayoutEntry {
        binding: 0,
        visibility: fragment,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Uniform,
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    }];
    entries.extend((1..=textures).map(|binding| wgpu::BindGroupLayoutEntry {
        binding,
        visibility: fragment,
        ty: wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Float { filterable: true },
            view_dimension: wgpu::TextureViewDimension::D2,
            multisampled: false,
        },
        count: None,
    }));
    entries.push(wgpu::BindGroupLayoutEntry {
        binding: textures + 1,
        visibility: fragment,
        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
        count: None,
    });
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some(label),
        entries: &entries,
    })
}

/// A pipeline drawing `vs_main`'s full-screen triangle with `fs_main`.
fn pipeline(
    device: &wgpu::Device,
    label: &str,
    module: &wgpu::ShaderModule,
    layout: &wgpu::BindGroupLayout,
    cache: Option<&wgpu::PipelineCache>,
) -> wgpu::RenderPipeline {
    pass(
        device, label, module, layout, cache, "vs_main", "fs_main", None,
    )
}

/// A pipeline drawing with `vertex` and `fragment` (vertices from the
/// vertex index alone, no buffers), blended by `blend`.
#[allow(
    clippy::too_many_arguments,
    reason = "one pipeline descriptor combines device resources, shader entry points and blending"
)]
fn pass(
    device: &wgpu::Device,
    label: &str,
    module: &wgpu::ShaderModule,
    layout: &wgpu::BindGroupLayout,
    cache: Option<&wgpu::PipelineCache>,
    vertex: &str,
    fragment: &str,
    blend: Option<wgpu::BlendState>,
) -> wgpu::RenderPipeline {
    let started = Instant::now();
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some(label),
        bind_group_layouts: &[Some(layout)],
        immediate_size: 0,
    });
    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(label),
        layout: Some(&pipeline_layout),
        vertex: wgpu::VertexState {
            module,
            entry_point: Some(vertex),
            buffers: &[],
            compilation_options: wgpu::PipelineCompilationOptions::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module,
            entry_point: Some(fragment),
            targets: &[Some(wgpu::ColorTargetState {
                format: FORMAT,
                blend,
                write_mask: wgpu::ColorWrites::ALL,
            })],
            compilation_options: wgpu::PipelineCompilationOptions::default(),
        }),
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        multiview_mask: None,
        cache,
    });
    log::info!("visuals: {label} pipeline {:.1} ms", ms(started));
    pipeline
}
