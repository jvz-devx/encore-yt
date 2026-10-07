//! The player bar's background, drawn by the effects layer under the app:
//! a glow of the cover's palette rising from the window's bottom edge, the
//! seek bar (glowing fill, the song's waveform, a playhead that pulses on
//! the kick) and soft rings around the play button and the cover. One
//! `ytfast_visuals::Strip` frame holds all of it.
//!
//! The bar leaves its background and slider see-through while a frame
//! shows ([`super::paints_bar`]); the slider still takes the clicks and
//! drags, and the most-replayed ridge draws over the seek bar.

use std::cell::Cell;
use std::rc::Rc;
use std::time::{Duration, Instant};

use gpui_kit::*;
use ytfast_visuals::{Look, Seek, Strip, StripColors, StripParams};

use super::effects::Tick;
use super::frames::Frames;
use super::slots::{Slot, Slots};
use crate::theme::{self, radius};

/// How long a new cover's colours take to fade in.
const FADE: Duration = Duration::from_millis(1200);

/// What the app tells the bar on each render.
#[derive(Clone, Debug, Default)]
pub struct Input {
    /// A song is loaded.
    pub track: bool,
    /// How far into the song, 0..1 (the slider's value while it is held).
    pub progress: f32,
    /// The song's length is known.
    pub known: bool,
    /// The song's length in seconds (0 while unknown).
    pub duration: f64,
    /// The song whose outline to show.
    pub video_id: Option<String>,
    /// The song's replay heat, for the most-replayed ridge.
    pub heat: Option<ytfast::heat::Heat>,
}

/// What changes the strip's still picture: when it differs from the last
/// one drawn, a new frame is drawn even while nothing animates.
#[derive(Clone, Debug, Default, PartialEq)]
struct Still {
    size: (u32, u32),
    /// The playhead, in output pixels.
    head: i32,
    known: bool,
    track: bool,
    hover: bool,
    look: Option<Look>,
    reduce: bool,
    wave: bool,
    heat: bool,
    cover: bool,
    /// Device pixel positions of the bar's controls.
    layout: [i32; 6],
}

/// The palette cross-fading to a new cover's.
#[derive(Clone, Copy)]
struct Palette {
    from: [[f32; 4]; 4],
    to: [[f32; 4]; 4],
    glow_from: f32,
    glow_to: f32,
    at: Option<Instant>,
}

impl Palette {
    fn mix(&self) -> f32 {
        let t = self.at.map_or(1.0, |at| {
            (at.elapsed().as_secs_f32() / FADE.as_secs_f32()).min(1.0)
        });
        t * t * (3.0 - 2.0 * t)
    }

    fn colours(&self) -> ([[f32; 4]; 4], f32) {
        let m = self.mix();
        let mut out = self.to;
        for (o, f) in out.iter_mut().zip(&self.from) {
            for i in 0..4 {
                o[i] = f[i] + (o[i] - f[i]) * m;
            }
        }
        (out, self.glow_from + (self.glow_to - self.glow_from) * m)
    }

    fn fading(&self) -> bool {
        self.at.is_some_and(|at| at.elapsed() < FADE)
    }
}

pub struct Bar {
    strip: Option<Strip>,
    frames: Frames,
    palette: Palette,
    /// The waveform uploaded, by video id.
    wave: Option<String>,
    /// The heat uploaded: video id and song length.
    heat: Option<(String, u64)>,
    /// A slow swell following the kicks, for the glow.
    breath: f32,
    /// The pointer is over the seek bar (set from the paint's listener).
    pub hover: Rc<Cell<bool>>,
    drawn: Option<Still>,
    pending: u8,
    /// When the last paced frame was drawn (the breath decays over time).
    paced_at: Option<Instant>,
}

impl Bar {
    pub fn new() -> Self {
        let neutral = [[0.5, 0.5, 0.5, 1.0]; 4];
        Self {
            strip: None,
            frames: Frames::default(),
            palette: Palette {
                from: neutral,
                to: neutral,
                glow_from: 0.0,
                glow_to: 0.0,
                at: None,
            },
            wave: None,
            heat: None,
            breath: 0.4,
            hover: Rc::new(Cell::new(false)),
            drawn: None,
            pending: 0,
            paced_at: None,
        }
    }

    /// The frame on screen.
    pub fn image(&self) -> Option<std::sync::Arc<RenderImage>> {
        self.frames.image()
    }

    /// A new cover's palette (or none): cross-faded unless `at_once`.
    pub fn set_palette(&mut self, palette: Option<[[f32; 4]; 4]>, at_once: bool) {
        let (now, glow) = self.palette.colours();
        let (to, glow_to) = match palette {
            Some(p) => (p, 1.0),
            None => (self.palette.to, 0.0),
        };
        self.palette = Palette {
            from: now,
            to,
            glow_from: glow,
            glow_to,
            at: (!at_once).then(Instant::now),
        };
        if at_once {
            self.palette.from = to;
            self.palette.glow_from = glow_to;
        }
        self.pending = self.pending.max(2);
    }

    /// The palette is still cross-fading: frames should keep coming.
    pub fn fading(&self) -> bool {
        self.palette.fading()
    }

    /// Frames are wanted although nothing animates.
    pub fn pending(&self) -> bool {
        self.pending > 0
    }

    /// The most colourful palette entry: the rings' and the dissolve
    /// edge's hue.
    pub fn accent(&self) -> [f32; 3] {
        accent(&self.palette.to)
    }

    /// Drops the strip (the GPU goes with it); the last frame stays on
    /// screen until the next one.
    pub fn release(&mut self) {
        self.strip = None;
        self.drawn = None;
        self.wave = None;
        self.heat = None;
    }

    pub fn forget(&mut self) {
        self.frames.forget();
        self.drawn = None;
    }

    /// Draws a frame when one is due or the still picture changed.
    pub fn update(&mut self, tick: &Tick, input: &Input, window: &mut Window, cx: &App) {
        let Some(bar) = Slots::get(cx, Slot::Bar) else {
            return;
        };
        let scale = window.scale_factor();
        let size = (
            (f32::from(bar.size.width) * scale).round().max(1.) as u32,
            (f32::from(bar.size.height) * scale).round().max(1.) as u32,
        );
        let params = self.params(tick, input, bar, scale, cx);
        let still = self.still(tick, input, size, &params, cx);
        if self.drawn.as_ref() != Some(&still) {
            self.pending = self.pending.max(2);
        }
        if tick.due {
            self.breathe(tick);
        }
        if !tick.due && self.pending == 0 {
            return;
        }
        let strip = self
            .strip
            .get_or_insert_with(|| Strip::new(tick.gpu, size.0, size.1));
        if strip.size() != size {
            strip.resize(size.0, size.1);
            self.pending = self.pending.max(2);
        }
        if self.wave != input.video_id {
            let values = input
                .video_id
                .as_deref()
                .and_then(|id| super::waveform::outline(id, cx));
            strip.set_waveform(values.as_deref());
            if values.is_some() || input.video_id.is_none() {
                self.wave = input.video_id.clone();
            }
        }
        let heat = input
            .video_id
            .clone()
            .zip(input.heat.as_ref())
            .map(|(id, _)| (id, input.duration.to_bits()));
        if self.heat != heat {
            strip.set_heat(heat_values(input).as_deref());
            self.heat = heat;
        }
        // Animating, frames are pipelined (one frame late, no waiting);
        // a still picture is waited for, so it shows in this render.
        let frame = if tick.due {
            strip.frame(&params)
        } else {
            strip.frame_now(&params).map(Some)
        };
        match frame {
            Ok(Some(frame)) => {
                self.frames.push(frame, window);
                self.pending = if tick.due {
                    self.pending.saturating_sub(1)
                } else {
                    0
                };
            }
            Ok(None) => {}
            Err(e) => log::warn!("visuals: strip frame: {e:#}"),
        }
        self.drawn = Some(still);
    }

    /// Follows the beat: quick to swell, slow to settle. Paced frames come
    /// unevenly, so the decay uses the time since the last one.
    fn breathe(&mut self, tick: &Tick) {
        let now = Instant::now();
        let dt = self
            .paced_at
            .map_or(tick.dt, |at| (now - at).as_secs_f32().min(0.3));
        self.paced_at = Some(now);
        let target = tick.kick.max(tick.bass * 0.6);
        self.breath = if target > self.breath {
            self.breath + (target - self.breath) * 0.3
        } else {
            (self.breath - dt * 0.9).max(target)
        };
    }

    fn params(
        &self,
        tick: &Tick,
        input: &Input,
        bar: Bounds<Pixels>,
        scale: f32,
        cx: &App,
    ) -> StripParams {
        let local = |b: Bounds<Pixels>| {
            let o = b.origin - bar.origin;
            [
                f32::from(o.x) * scale,
                f32::from(o.y) * scale,
                f32::from(o.x + b.size.width) * scale,
                f32::from(o.y + b.size.height) * scale,
            ]
        };
        let seek = Slots::get(cx, Slot::Seek).map(|b| {
            let r = local(b);
            Seek {
                left: r[0],
                top: r[1],
                right: r[2],
                progress: input.progress,
                known: input.known,
                hover: if self.hover.get() && input.known {
                    1.0
                } else {
                    0.0
                },
            }
        });
        let play = Slots::get(cx, Slot::Play).filter(|_| input.track).map(|b| {
            let r = local(b);
            [(r[0] + r[2]) / 2., (r[1] + r[3]) / 2., (r[2] - r[0]) / 2.]
        });
        let cover = Slots::get(cx, Slot::BarCover)
            .filter(|_| input.track)
            .map(|b| (local(b), f32::from(cover_radius()) * scale));
        let (palette, glow) = self.palette.colours();
        let still = tick.reduce;
        StripParams {
            seconds: tick.seconds,
            breath: if still { 0.4 } else { self.breath },
            kick: if still { 0.0 } else { tick.kick },
            glow: if input.track { glow } else { 0.0 },
            look: tick.look,
            scale,
            seek,
            play,
            cover,
            ridge: input.heat.as_ref().map(|_| RIDGE),
            colors: colours(cx, accent(&palette)),
            palette,
        }
    }

    fn still(
        &self,
        tick: &Tick,
        input: &Input,
        size: (u32, u32),
        p: &StripParams,
        cx: &App,
    ) -> Still {
        let seek = p.seek.unwrap_or_default();
        let head = seek.left + (seek.right - seek.left) * input.progress;
        let play = p.play.unwrap_or_default();
        let cover = p.cover.map(|c| c.0).unwrap_or_default();
        Still {
            size,
            head: head.round() as i32,
            known: input.known,
            track: input.track,
            hover: seek.hover > 0.5,
            look: Some(tick.look),
            reduce: tick.reduce,
            wave: input
                .video_id
                .as_deref()
                .is_some_and(|id| super::waveform::has_outline(id, cx)),
            heat: p.ridge.is_some(),
            cover: p.glow > 0.0,
            layout: [
                seek.left as i32,
                seek.top as i32,
                seek.right as i32,
                play[0] as i32,
                cover[0] as i32,
                cover[1] as i32,
            ],
        }
    }
}

/// The theme colours, display space.
fn colours(cx: &App, accent: [f32; 3]) -> StripColors {
    let c = theme::colors(cx);
    let rgb = |h: Hsla| {
        let c = h.to_rgb();
        [c.r, c.g, c.b]
    };
    StripColors {
        base: rgb(c.base),
        signal: rgb(c.signal),
        ink: rgb(c.text),
        track: match theme::mode(cx) {
            theme::Mode::Dark => 0.2,
            theme::Mode::Light => 0.16,
        },
        accent,
        muted: rgb(c.text_muted),
    }
}

/// The ridge's height where the heat is greatest (as `extras::ridge`
/// draws it without effects).
const RIDGE: f32 = 11.;

/// The song's replay heat evenly over its length, for the strip.
fn heat_values(input: &Input) -> Option<Vec<f32>> {
    let heat = input.heat.as_ref().filter(|_| input.duration > 0.0)?;
    let n = 512;
    Some(
        (0..n)
            .map(|i| heat.value_at((i as f64 + 0.5) / n as f64 * input.duration))
            .collect(),
    )
}

/// The palette entry farthest from grey.
fn accent(palette: &[[f32; 4]; 4]) -> [f32; 3] {
    let chroma = |c: &[f32; 4]| c[0].max(c[1]).max(c[2]) - c[0].min(c[1]).min(c[2]);
    let best = palette
        .iter()
        .max_by(|a, b| chroma(a).total_cmp(&chroma(b)))
        .copied()
        .unwrap_or([0.5; 4]);
    [best[0], best[1], best[2]]
}

/// The player bar cover's corner radius (`widgets::cover` at
/// `size::PLAYER_COVER`).
fn cover_radius() -> Pixels {
    radius::SM
}
