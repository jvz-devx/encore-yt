//! The effects behind ytfast-gpui's Now Playing, without GPUI.
//!
//! - [`Renderer`]: our own wgpu device draws the animated cover backdrop
//!   (flowing blurred cover, palette gradient, bloom, particles) offscreen
//!   and reads each frame back as BGRA bytes for the app to paint.
//! - [`AudioTap`]: a PipeWire capture of mpv's stream, analysed into
//!   spectrum bands plus bass and beat levels.
//! - [`waveform`]: a whole song's loudness outline, decoded with ffmpeg and
//!   cached per video id.
//!
//! Findings and numbers: `gpui/NOTES-visuals.md`.

mod cover;
mod mpv;
mod pipewire;
mod renderer;
mod spectrum;
pub mod waveform;

pub use cover::{COVER_SIZE, Cover};
pub use renderer::{Frame, FrameCost, FrameParams, Look, Renderer};
pub use spectrum::{AudioTap, BANDS, Bands};
