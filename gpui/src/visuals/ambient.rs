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

use super::config::{self, Palette, ParticleColour};
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
    /// The sparkles' drift and twinkle clocks and the wave's, each at its
    /// own speed, and the animation and flow clocks they last followed.
    drift: f32,
    twinkle: f32,
    wave: f32,
    clocks: Option<(f32, f32)>,
}

impl Ambient {
    /// The frame to paint over the backdrop, while one fits `area`.
    pub fn image(&self) -> Option<Arc<RenderImage>> {
        self.frames.image()
    }

    /// Whether the layer draws at all.
    pub fn wanted() -> bool {
        let config = config::get();
        let (p, w) = (&config.particles, &config.wave);
        (p.on && p.amount > 0.) || (w.on && w.strength > 0.)
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
        let (p, w) = (&config.particles, &config.wave);
        let (passed, flowed) = self.clocks.map_or((0., 0.), |(seconds, last)| {
            ((tick.seconds - seconds).max(0.), (flow - last).max(0.))
        });
        self.clocks = Some((tick.seconds, flow));
        self.drift += passed * p.speed;
        self.twinkle += passed * p.twinkle_speed;
        self.wave += flowed * w.speed;
        let palette_kind = match p.colour {
            ParticleColour::Accent => Palette::Accent,
            ParticleColour::White | ParticleColour::Cover => Palette::Cover,
        };
        let params = VisualizerParams {
            style: AMBIENT,
            seconds: tick.seconds,
            travel: 0.,
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
                palette_kind,
                &config.visualizer.custom,
                palette,
                tick.look,
                cx,
            ),
            bars: &self.bars,
            ambient: Settings {
                amount: if p.on { p.amount } else { 0. },
                size_min: p.size_min,
                size_max: p.size_max,
                softness: p.softness,
                brightness: p.brightness,
                drift: self.drift,
                depth: p.depth,
                reaction: p.reaction,
                twinkle: p.twinkle,
                twinkle_clock: self.twinkle,
                direction: p.direction.to_radians(),
                tint: if p.colour == ParticleColour::White {
                    0.
                } else {
                    1.
                },
                wave: if w.on { w.strength } else { 0. },
                wave_clock: self.wave,
                ribbons: w.ribbons,
                wave_height: w.height,
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
