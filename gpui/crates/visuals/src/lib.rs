//! The effects of ytfast-gpui, without GPUI.
//!
//! - [`Gpu`]: our own offscreen wgpu device with every effect's pipeline
//!   compiled (through a pipeline cache on disk), shared by the renderers below.
//!   Each draws offscreen and reads every frame back as BGRA bytes for the
//!   app to paint.
//! - [`Renderer`]: the animated cover backdrop behind Now Playing (flowing
//!   blurred cover, palette gradient, bloom, particles).
//! - [`Strip`]: the player bar's background in one pass (palette glow, the
//!   seek bar with the song's waveform, beat halos).
//! - [`Dissolve`]: a cover burning into the next one on a track change.
//! - [`Visualizer`]: the audio visualiser (bars, mirrored bars, a ring
//!   round the cover, a line spectrum, a particle field), with alpha, from
//!   [`Bars`] made of the spectrum.
//! - [`AudioTap`]: a PipeWire capture of mpv's stream, analysed into
//!   spectrum bands plus bass and beat levels.
//! - [`waveform`]: a whole song's loudness outline, decoded with ffmpeg and
//!   cached per video id.
//!
//! Findings and numbers: `gpui/NOTES-visuals.md`.

pub mod color;
mod cover;
mod dissolve;
mod gpu;
mod mpv;
mod pipelines;
mod pipewire;
mod renderer;
mod spectrum;
mod strip;
mod target;
mod visualizer;
pub mod waveform;

pub use cover::{COVER_SIZE, Cover};
pub use dissolve::Dissolve;
pub use gpu::Gpu;
pub use renderer::{CoverShadow, FrameParams, Look, Renderer, Tune};
pub use spectrum::{AudioTap, BANDS, Bands, band_at};
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
