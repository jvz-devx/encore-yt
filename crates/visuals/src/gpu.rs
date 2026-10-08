//! The one wgpu device every effect draws with, its compiled pipelines
//! ([`crate::pipelines`]), and the pieces the effects share: uniforms,
//! textures, a sampler and their bind group.

use std::path::Path;
use std::sync::Arc;
use std::time::Instant;

use anyhow::{Context as _, Result, anyhow};

use crate::pipeline_cache::DiskCache;
use crate::pipelines::Pipelines;

/// Pixel format of the frames: what GPUI's atlas stores on Vulkan.
pub(crate) const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Bgra8Unorm;

/// Our own offscreen device (GPUI's isn't reachable, see NOTES-visuals.md)
/// with every effect's pipeline compiled. Making one blocks for tens to
/// hundreds of milliseconds, so the app makes it on a background thread
/// (it is `Send`). Cheap to clone: the renderers made from it share it,
/// and it goes away with the last of them.
#[derive(Clone)]
pub struct Gpu {
    pub(crate) device: wgpu::Device,
    pub(crate) queue: wgpu::Queue,
    pub(crate) pipelines: Arc<Pipelines>,
    adapter: Arc<str>,
    /// A GPU that gets less work by default: Adreno (Snapdragon laptops),
    /// or anything on the GL fallback.
    modest: bool,
}

/// The scenes' default detail on a modest GPU (1 elsewhere).
const MODEST_DETAIL: f32 = 0.7;

impl Gpu {
    /// A low-power Vulkan (or GL) device without a surface, its pipelines
    /// compiled without a persistent cache.
    pub fn new() -> Result<Self> {
        Self::create(None)
    }

    /// Like [`Self::new`], with the pipelines going through a pipeline
    /// cache kept in `dir` (Vulkan only).
    pub fn with_pipeline_cache(dir: &Path) -> Result<Self> {
        Self::create(Some(dir))
    }

    fn create(cache_dir: Option<&Path>) -> Result<Self> {
        // Vulkan on Linux, Metal on macOS, DX12 or Vulkan on Windows; GL
        // where none of them is there. Vulkan and GL alone left the
        // effects off on every Mac. `WGPU_BACKEND` (e.g. `gl`) picks one,
        // for checks.
        let backends =
            wgpu::Backends::from_env().unwrap_or(wgpu::Backends::PRIMARY | wgpu::Backends::GL);
        Self::with_backends(backends, cache_dir)
    }

    pub(crate) fn with_backends(
        backends: wgpu::Backends,
        cache_dir: Option<&Path>,
    ) -> Result<Self> {
        let started = Instant::now();
        let dx12 = dx12_compiler();
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends,
            flags: wgpu::InstanceFlags::default(),
            backend_options: wgpu::BackendOptions {
                dx12: wgpu::Dx12BackendOptions {
                    shader_compiler: dx12.clone(),
                    ..Default::default()
                },
                ..Default::default()
            },
            memory_budget_thresholds: wgpu::MemoryBudgetThresholds::default(),
            display: None,
        });
        let instance_ms = ms(started);
        let started = Instant::now();
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::LowPower,
            compatible_surface: None,
            force_fallback_adapter: false,
        }))
        .map_err(|e| anyhow!("no GPU adapter: {e}"))?;
        let info = adapter.get_info();
        if info.backend == wgpu::Backend::Dx12 {
            log::info!("visuals: DX12 shaders through {dx12:?}");
        }
        let adapter_ms = ms(started);
        let started = Instant::now();
        let cache_dir =
            cache_dir.filter(|_| adapter.features().contains(wgpu::Features::PIPELINE_CACHE));
        let required_features = match cache_dir {
            Some(_) => wgpu::Features::PIPELINE_CACHE,
            None => wgpu::Features::empty(),
        };
        let (device, queue) = pollster::block_on(
            adapter.request_device(&wgpu::DeviceDescriptor {
                label: Some("encore visuals"),
                required_features,
                required_limits: wgpu::Limits::downlevel_defaults()
                    .using_resolution(adapter.limits())
                    .using_alignment(adapter.limits()),
                memory_hints: wgpu::MemoryHints::MemoryUsage,
                trace: wgpu::Trace::Off,
                experimental_features: wgpu::ExperimentalFeatures::disabled(),
            }),
        )
        .context("creating the visuals device")?;
        log::info!(
            "visuals: instance {instance_ms:.1} ms, adapter {adapter_ms:.1} ms, device {:.1} ms",
            ms(started)
        );
        let started = Instant::now();
        let disk = cache_dir.and_then(|dir| DiskCache::open(&device, &info, dir));
        let pipelines = Pipelines::new(&device, disk.as_ref().map(|d| &d.cache));
        log::info!(
            "visuals: pipelines {:.1} ms ({})",
            ms(started),
            match &disk {
                Some(d) if d.loaded() > 0 => format!("cache of {} KB", d.loaded() / 1024),
                Some(_) => "empty cache".into(),
                None => "no pipeline cache".into(),
            }
        );
        if let Some(disk) = &disk {
            disk.save();
        }
        let modest = info.name.contains("Adreno") || info.backend == wgpu::Backend::Gl;
        if modest {
            log::info!("visuals: {} gets the scenes' lower detail", info.name);
        }
        Ok(Self {
            device,
            queue,
            pipelines: Arc::new(pipelines),
            adapter: format!("{} ({:?})", info.name, info.backend).into(),
            modest,
        })
    }

    /// The GPU and API in use, for the log.
    pub fn adapter(&self) -> &str {
        &self.adapter
    }

    /// Adreno, or the GL fallback: the scenes draw less by default
    /// ([`Self::scene_detail`]) and Aurora's lake mirrors fewer curtains.
    pub fn modest(&self) -> bool {
        self.modest
    }

    /// What the scenes' detail setting is relative to: their default step
    /// count here.
    pub fn scene_detail(&self) -> f32 {
        if self.modest { MODEST_DETAIL } else { 1.0 }
    }

    pub(crate) fn uniforms(&self, label: &str, size: u64) -> wgpu::Buffer {
        self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(label),
            size,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        })
    }

    pub(crate) fn sampler(&self, address: wgpu::AddressMode) -> wgpu::Sampler {
        self.device.create_sampler(&wgpu::SamplerDescriptor {
            label: None,
            address_mode_u: address,
            address_mode_v: address,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        })
    }

    /// A sampled 2D texture we upload into.
    pub(crate) fn texture(
        &self,
        label: &str,
        (width, height): (u32, u32),
        format: wgpu::TextureFormat,
    ) -> wgpu::Texture {
        self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size: extent(width, height),
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        })
    }

    /// Writes tightly packed pixels (`bytes_per_pixel` each) into `texture`.
    pub(crate) fn upload(&self, texture: &wgpu::Texture, bytes_per_pixel: u32, data: &[u8]) {
        let size = texture.size();
        self.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            data,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(size.width * bytes_per_pixel),
                rows_per_image: Some(size.height),
            },
            size,
        );
    }

    /// A bind group for [`Self::layout`]: the uniforms, `textures`, `sampler`.
    pub(crate) fn bind_group(
        &self,
        layout: &wgpu::BindGroupLayout,
        uniforms: &wgpu::Buffer,
        textures: &[&wgpu::TextureView],
        sampler: &wgpu::Sampler,
    ) -> wgpu::BindGroup {
        let mut entries = vec![wgpu::BindGroupEntry {
            binding: 0,
            resource: uniforms.as_entire_binding(),
        }];
        for (i, view) in textures.iter().enumerate() {
            entries.push(wgpu::BindGroupEntry {
                binding: i as u32 + 1,
                resource: wgpu::BindingResource::TextureView(view),
            });
        }
        entries.push(wgpu::BindGroupEntry {
            binding: textures.len() as u32 + 1,
            resource: wgpu::BindingResource::Sampler(sampler),
        });
        self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout,
            entries: &entries,
        })
    }
}

/// DX12's shader compiler: DXC from a `dxcompiler.dll` next to the app
/// when there is one, else FXC (old and slow, but part of Windows). Named
/// here instead of wgpu's default (`Auto`), which also takes a
/// `dxcompiler.dll` from anywhere on the PATH, whatever its version. DXC
/// linked in (`static-dxc`) would add 22.5 MB to the app. `WGPU_DX12_COMPILER`
/// (`fxc`, `dxc`) overrides the choice, for comparing.
fn dx12_compiler() -> wgpu::Dx12Compiler {
    let beside_app = std::env::current_exe()
        .ok()
        .and_then(|exe| Some(exe.parent()?.join("dxcompiler.dll")))
        .filter(|dll| cfg!(windows) && dll.is_file());
    let compiler = match beside_app {
        Some(dll) => wgpu::Dx12Compiler::DynamicDxc {
            dxc_path: dll.to_string_lossy().into_owned(),
        },
        None => wgpu::Dx12Compiler::Fxc,
    };
    compiler.with_env()
}

pub(crate) fn extent(width: u32, height: u32) -> wgpu::Extent3d {
    wgpu::Extent3d {
        width,
        height,
        depth_or_array_layers: 1,
    }
}

/// Milliseconds since `started`, for the log.
pub(crate) fn ms(started: Instant) -> f64 {
    started.elapsed().as_secs_f64() * 1000.0
}
