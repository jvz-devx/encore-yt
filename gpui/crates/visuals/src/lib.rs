//! The effects of ytfast-gpui, without GPUI.
//!
//! - [`Gpu`]: our own offscreen wgpu device, shared by the renderers below.
//!   Each draws offscreen and reads every frame back as BGRA bytes for the
//!   app to paint.
//! - [`Renderer`]: the animated cover backdrop behind Now Playing (flowing
//!   blurred cover, palette gradient, bloom, particles).
//! - [`Strip`]: the player bar's background in one pass (palette glow, the
//!   seek bar with the song's waveform, beat halos).
//! - [`Dissolve`]: a cover burning into the next one on a track change.
//! - [`AudioTap`]: a PipeWire capture of mpv's stream, analysed into
//!   spectrum bands plus bass and beat levels.
//! - [`waveform`]: a whole song's loudness outline, decoded with ffmpeg and
//!   cached per video id.
//!
//! Findings and numbers: `gpui/NOTES-visuals.md`.

mod cover;
mod dissolve;
mod gpu;
mod mpv;
mod pipewire;
mod renderer;
mod spectrum;
mod strip;
mod target;
pub mod waveform;

pub use cover::{COVER_SIZE, Cover};
pub use dissolve::Dissolve;
pub use gpu::Gpu;
pub use renderer::{CoverShadow, FrameParams, Look, Renderer};
pub use spectrum::{AudioTap, BANDS, Bands};
pub use strip::{Seek, Strip, StripColors, StripParams};
pub use target::{Frame, FrameCost};
