//! The backdrop's ambient layer: a soft light wave in the cover's colours
//! and fine sparkles drifting along it, like the PS3's XrossMediaBar. The
//! visualiser shader draws it (`ytfast_visuals::AMBIENT`) at the panel's
//! full size, since the sparkles are a pixel or two across and the
//! backdrop under it renders at 0.4. It paints over the backdrop in the
//! same area; frames come with the backdrop's paced frames, and under
//! reduced motion or while paused the last one stays.

use std::sync::Arc;

use gpui_kit::*;
use ytfast_visuals::{AMBIENT, Ambient as Settings, Bars, Visualizer, VisualizerParams};

use super::config::{self, Palette};
use super::effects::Tick;
use super::frames::Frames;

#[derive(Default)]
pub struct Ambient {
    renderer: Option<Visualizer>,
    frames: Frames,
    bars: Bars,
    /// The size the frame on screen was drawn at, and the one in flight.
    shown: Option<(u32, u32)>,
    pending: Option<(u32, u32)>,
}

impl Ambient {
    /// The frame to paint over the backdrop, while one fits `area`.
    pub fn image(&self) -> Option<Arc<RenderImage>> {
        self.frames.image()
    }

    /// Whether the layer draws at all.
    pub fn wanted() -> bool {
        let config = config::get();
        let b = &config.backdrop;
        (b.motes && b.motes_amount > 0.) || (b.wave && b.wave_strength > 0.)
    }

    /// Stops showing (the backdrop went, or the layer was switched off).
    pub fn hide(&mut self, cx: &mut App) {
        if self.shown.is_some() || self.pending.is_some() {
            self.shown = None;
            self.pending = None;
            if let Some(r) = &mut self.renderer {
                r.discard();
            }
            self.frames.clear(cx);
        }
    }

    /// Drops the renderer (the GPU is going).
    pub fn release(&mut self, cx: &mut App) {
        self.renderer = None;
        self.hide(cx);
    }

    /// Draws the next frame over `area` when one is due (or when none fits
    /// it yet). `flow` is the backdrop's clock, `palette` the cover's.
    pub fn update(
        &mut self,
        tick: &Tick,
        area: Bounds<Pixels>,
        flow: f32,
        palette: &[[f32; 4]; 4],
        window: &mut Window,
        cx: &mut App,
    ) {
        let scale = window.scale_factor();
        let size = (
            (f32::from(area.size.width) * scale)
                .round()
                .clamp(1., 4096.) as u32,
            (f32::from(area.size.height) * scale)
                .round()
                .clamp(1., 4096.) as u32,
        );
        let fits = self.shown == Some(size);
        if !tick.due && fits {
            return;
        }
        let renderer = self.renderer.get_or_insert_with(|| {
            let started = std::time::Instant::now();
            let r = Visualizer::new(tick.gpu, size.0, size.1);
            super::timing::setup(started);
            r
        });
        if renderer.size() != size {
            renderer.resize(size.0, size.1);
        }
        let config = config::get();
        let b = &config.backdrop;
        let params = VisualizerParams {
            style: AMBIENT,
            seconds: tick.seconds,
            travel: flow,
            bass: 0.,
            kick: 0.,
            // The beat only lifts the sparkles a little, from the smoothed
            // level; still frames don't move with it.
            level: if tick.reduce { 0. } else { tick.level },
            treble: 0.,
            look: tick.look,
            opacity: 1.,
            glow: 0.,
            peaks: false,
            scale,
            cover: None,
            reach: 0.,
            stops: super::visualizer::stops(
                Palette::Cover,
                &config.visualizer.custom,
                palette,
                tick.look,
                cx,
            ),
            bars: &self.bars,
            ambient: Settings {
                amount: if b.motes { b.motes_amount } else { 0. },
                size: b.mote_size,
                twinkle: b.twinkle,
                brightness: b.mote_brightness,
                wave: if b.wave { b.wave_strength } else { 0. },
            },
        };
        match renderer.frame(&params) {
            Ok(Some(frame)) => {
                self.frames.push(frame, window);
                self.shown = self.pending;
            }
            Ok(None) => {}
            Err(e) => log::warn!("visuals: ambient frame: {e:#}"),
        }
        self.pending = Some(size);
        if !tick.due {
            // The frame just rendered shows on the next one.
            window.request_animation_frame();
        }
    }
}
