//! The effects' GPU device: a `encore_visuals::Gpu` with every effect's
//! pipeline compiled, made on a background thread so no frame waits for
//! the Vulkan instance, the device or a shader compile (together 70-300 ms
//! on an Intel UHD 630, see NOTES-visuals.md).
//!
//! It is made once after the first frame (the warm-up), and again after
//! the effects layer released it for idling as soon as an effect needs it.
//! Until it is ready the effects just don't draw: the views keep their
//! plain backgrounds. The pipelines go through a persistent pipeline cache
//! in the app's cache directory (`gpu/`).

use std::path::{Path, PathBuf};
use std::time::Instant;

use encore_visuals::Gpu;
use gpui_kit::*;

use super::effects::Effects;

pub struct Device {
    state: State,
    /// Where the pipeline cache lives.
    cache: PathBuf,
    /// The start-up warm-up was asked for.
    warmed: bool,
}

enum State {
    Off,
    /// To be made after this frame.
    Scheduled,
    Making {
        _task: Task<()>,
    },
    Ready(Gpu),
    /// No usable GPU: the effects stay off.
    Failed,
}

impl Device {
    /// A device whose pipeline cache lives under the app's `cache`.
    pub fn new(cache: &Path) -> Self {
        Self {
            state: State::Off,
            cache: cache.join("gpu"),
            warmed: false,
        }
    }

    pub fn gpu(&self) -> Option<&Gpu> {
        match &self.state {
            State::Ready(gpu) => Some(gpu),
            _ => None,
        }
    }

    /// A device is held or on its way.
    pub fn held(&self) -> bool {
        matches!(
            self.state,
            State::Scheduled | State::Making { .. } | State::Ready(_)
        )
    }

    /// Drops the device (idle); it is made again when next needed.
    pub fn release(&mut self) {
        if matches!(self.state, State::Ready(_)) {
            self.state = State::Off;
        }
    }

    /// Asks for the device after this frame: an effect wants it, or
    /// (`warm_up`) the window has shown its first frame. Whether it was off
    /// and should be started ([`Self::start`]) after this frame.
    pub fn schedule(&mut self, warm_up: bool) -> bool {
        if warm_up && std::mem::replace(&mut self.warmed, true) {
            return false;
        }
        if !matches!(self.state, State::Off) {
            return false;
        }
        self.warmed = true;
        self.state = State::Scheduled;
        true
    }

    /// Starts making the device on a background thread; the effects layer
    /// gets it with [`Effects::device_made`].
    pub fn start(&mut self, cx: &mut Context<Effects>) {
        if !matches!(self.state, State::Scheduled) {
            return;
        }
        let dir = self.cache.clone();
        self.state = State::Making {
            _task: cx.spawn(async move |this, cx| {
                let started = Instant::now();
                let made = cx
                    .background_spawn(async move { Gpu::with_pipeline_cache(&dir) })
                    .await;
                let _ = this.update(cx, |effects, cx| effects.device_made(made, started, cx));
            }),
        };
    }

    /// Takes the device made in the background.
    pub fn finish(&mut self, made: anyhow::Result<Gpu>, started: Instant) {
        let ms = started.elapsed().as_secs_f64() * 1000.0;
        self.state = match made {
            Ok(gpu) => {
                log::info!(
                    "visuals: GPU {} ready in {ms:.0} ms (made in the background)",
                    gpu.adapter()
                );
                State::Ready(gpu)
            }
            Err(e) => {
                log::warn!("visuals: no GPU, effects fall back: {e:#}");
                State::Failed
            }
        };
    }
}
