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

use std::path::{Path, PathBuf};
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
}

impl Pipelines {
    /// Compiles the four effects on `device`, through `cache` when there
    /// is one.
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
        }
    }
}

fn shader(device: &wgpu::Device, label: &str, source: &'static str) -> wgpu::ShaderModule {
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
        cache,
    });
    log::info!("visuals: {label} pipeline {:.1} ms", ms(started));
    pipeline
}

/// The persistent pipeline cache of one device.
pub(crate) struct DiskCache {
    file: PathBuf,
    loaded: Option<Vec<u8>>,
    pub cache: wgpu::PipelineCache,
}

impl DiskCache {
    /// Loads the cache for `info`'s GPU and driver from `dir`, or starts an
    /// empty one. `None` where wgpu has no pipeline cache for the backend.
    pub fn open(device: &wgpu::Device, info: &wgpu::AdapterInfo, dir: &Path) -> Option<Self> {
        let key = wgpu::util::pipeline_cache_key(info)?;
        let file = dir.join(format!(
            "{key}_{:016x}.bin",
            fnv1a(&[&info.driver, &info.driver_info])
        ));
        let loaded = std::fs::read(&file).ok();
        // SAFETY: the data is what `get_data` returned for this GPU (the
        // file name holds its vendor and device ids, so the header's adapter
        // key matches); wgpu checks the header, the driver's cache UUID and
        // the length, and starts empty (`fallback`) when any of them is off.
        let cache = unsafe {
            device.create_pipeline_cache(&wgpu::PipelineCacheDescriptor {
                label: Some("ytfast visuals"),
                data: loaded.as_deref(),
                fallback: true,
            })
        };
        Some(Self {
            file,
            loaded,
            cache,
        })
    }

    /// How much was loaded, for the log.
    pub fn loaded(&self) -> usize {
        self.loaded.as_ref().map_or(0, Vec::len)
    }

    /// Writes the cache back unless it holds what was loaded (to a
    /// temporary file renamed over the old one), and drops other drivers'
    /// caches for this GPU.
    pub fn save(&self) {
        let started = Instant::now();
        let Some(data) = self.cache.get_data() else {
            return;
        };
        if self
            .loaded
            .as_deref()
            .is_some_and(|loaded| same_cache(loaded, &data))
        {
            return;
        }
        let Some(dir) = self.file.parent() else {
            return;
        };
        let temp = self.file.with_extension("tmp");
        let written = std::fs::create_dir_all(dir)
            .and_then(|()| std::fs::write(&temp, &data))
            .and_then(|()| std::fs::rename(&temp, &self.file));
        match written {
            Ok(()) => {
                log::info!(
                    "visuals: pipeline cache saved, {} KB in {:.1} ms",
                    data.len() / 1024,
                    ms(started)
                );
                self.drop_stale(dir);
            }
            Err(e) => log::warn!("visuals: pipeline cache not saved: {e}"),
        }
    }

    /// Removes this GPU's caches from other drivers.
    fn drop_stale(&self, dir: &Path) {
        let Some(name) = self.file.file_name().and_then(|n| n.to_str()) else {
            return;
        };
        let Some((prefix, _)) = name.rsplit_once('_') else {
            return;
        };
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let other = entry.file_name();
            let other = other.to_string_lossy();
            if other != name && other.starts_with(&format!("{prefix}_")) {
                let _ = std::fs::remove_file(entry.path());
            }
        }
    }
}

/// Whether a cache read back holds what was loaded. The driver writes its
/// entries in a different order from run to run, so the bytes differ even
/// when nothing new was compiled; the length and wgpu's 64-byte header
/// (format, GPU, the driver's cache UUID, data size) are compared instead.
/// A driver update changes the UUID, and a new pipeline the length.
fn same_cache(loaded: &[u8], data: &[u8]) -> bool {
    const HEADER: usize = 64;
    loaded.len() == data.len()
        && loaded[..HEADER.min(loaded.len())] == data[..HEADER.min(data.len())]
}

/// A stable 64-bit hash (FNV-1a) of `parts`, for file names.
fn fnv1a(parts: &[&str]) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for byte in parts.iter().flat_map(|p| p.bytes().chain([0])) {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Gpu;

    /// A device with a cache directory writes one cache file, and the next
    /// device on the same GPU reads it and leaves one file.
    #[test]
    fn the_cache_is_kept_on_disk() {
        let _one = crate::gpu_test_lock();
        let dir = std::env::temp_dir().join(format!("ytfast-pipelines-{}", std::process::id()));
        let files = || {
            std::fs::read_dir(&dir)
                .map(|entries| entries.flatten().map(|e| e.path()).collect::<Vec<_>>())
                .unwrap_or_default()
        };
        let gpu = match Gpu::with_pipeline_cache(&dir) {
            Ok(gpu) => gpu,
            Err(e) => {
                eprintln!("skipped, no GPU: {e:#}");
                return;
            }
        };
        if !gpu
            .device
            .features()
            .contains(wgpu::Features::PIPELINE_CACHE)
        {
            eprintln!("skipped, no pipeline cache on {}", gpu.adapter());
            return;
        }
        drop(gpu);
        let saved = files();
        assert_eq!(saved.len(), 1, "{saved:?}");
        assert!(std::fs::metadata(&saved[0]).map(|m| m.len()).unwrap_or(0) > 0);
        Gpu::with_pipeline_cache(&dir).expect("a second device");
        assert_eq!(files(), saved);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_driver_hash_is_stable() {
        assert_eq!(fnv1a(&[]), 0xcbf2_9ce4_8422_2325);
        assert_eq!(fnv1a(&["a"]), fnv1a(&["a"]));
        assert_ne!(fnv1a(&["ab", "c"]), fnv1a(&["a", "bc"]));
    }
}
