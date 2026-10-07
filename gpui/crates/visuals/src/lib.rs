//! The effects of ytfast-gpui, without GPUI.
//!
//! - [`Gpu`]: our own offscreen wgpu device with every effect's pipeline
//!   compiled (through a pipeline cache on disk), shared by the renderers below.
//!   Each draws offscreen and reads every frame back as BGRA bytes for the
//!   app to paint.
//! - [`Renderer`]: the animated cover backdrop behind Now Playing (flowing
//!   blurred cover, palette gradient, bloom, a light wave).
//! - [`Strip`]: the player bar's background in one pass (palette glow, the
//!   seek bar with the song's waveform, beat halos).
//! - [`Dissolve`]: a cover burning into the next one on a track change.
//! - [`Visualizer`]: the audio visualiser (bars, mirrored bars, a ring
//!   round the cover, a line spectrum, a particle field), with alpha, from
//!   [`Bars`] made of the spectrum, and an oscilloscope from [`Scope`]'s
//!   traces of the samples.
//! - [`Scene`]: the 3D scenes (M30: the PS3's XMB wave, ridges made of
//!   the spectrum, an aurora), opaque, moved by the music through [`Pace`].
//! - [`AudioTap`]: what the audio engine plays (`ytfast_audio::Tap`, in
//!   process), analysed into spectrum bands plus bass and beat levels.
//! - [`waveform`]: a whole song's loudness outline, decoded with the audio
//!   engine's decoder and cached per video id.
//!
//! Findings and numbers: `gpui/NOTES-visuals.md`.

pub mod color;
mod cover;
mod dissolve;
mod gpu;
mod pipelines;
mod renderer;
mod scene;
mod scope;
mod spectrum;
mod strip;
mod target;
mod visualizer;
pub mod waveform;

pub use cover::{COVER_SIZE, Cover};
pub use dissolve::Dissolve;
pub use gpu::Gpu;
pub use renderer::{CoverShadow, FrameParams, Look, Renderer, Tune};
pub use scene::{Pace, Scene, SceneKind, SceneParams, seed};
pub use scope::{Channels, SPAN, Scope};
pub use spectrum::{AudioTap, BANDS, Bands, RECENT, band_at};
pub use strip::{Seek, Strip, StripColors, StripParams};
pub use target::{Frame, FrameCost};
pub use visualizer::{BarSettings, Bars, MAX_BARS, Visualizer, VisualizerParams};

/// Tests that make a GPU device take turns: Mesa's Vulkan driver crashes
/// now and then when several devices are made at once in one process.
#[cfg(test)]
pub(crate) fn gpu_test_lock() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    LOCK.lock().unwrap_or_else(|e| e.into_inner())
}
