//! The audio visualiser (M21) on the effects layer: `ytfast_visuals`'s
//! `Visualizer` frames painted over the backdrop, in Now Playing (when
//! Settings → Visuals puts it there), in Stage (when switched on) and in
//! the full-window visualiser (V).
//!
//! Where it draws depends on the style: bars, mirrored bars and the line
//! spectrum take a band (Now Playing's strip above the title; in Stage and
//! the full window a band along the bottom, or through the cover for
//! mirrored bars), the ring goes round the cover, and particles fill the
//! space round the cover (Now Playing) or the whole scene. Frames come only
//! with the paced frames while music plays; paused, hidden or under reduced
//! motion it draws nothing.

use std::sync::Arc;

use gpui_kit::*;
use ytfast_visuals::{BANDS, BarSettings, Bars, Look, Visualizer, VisualizerParams, color};

use super::config::{self, Palette, Spacing, Style};
use super::effects::Tick;
use super::frames::Frames;
use super::slots::{Slot, Slots};
use crate::theme;

/// Where the visualiser shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Place {
    NowPlaying,
    Stage,
    /// The full-window visualiser (V).
    Full,
}

#[derive(Default)]
pub struct Vis {
    renderer: Option<Visualizer>,
    frames: Frames,
    bars: Bars,
    /// The particles' clock.
    travel: f32,
    /// Where the frame on screen goes, in window coordinates.
    region: Option<Bounds<Pixels>>,
    /// The region the frame in flight was rendered for.
    pending_region: Option<Bounds<Pixels>>,
}

impl Vis {
    /// The frame to paint and where, while one shows.
    pub fn image(&self) -> Option<(Arc<RenderImage>, Bounds<Pixels>)> {
        self.frames.image().zip(self.region)
    }

    /// Stops showing (hidden, paused): the next start begins from silence.
    pub fn hide(&mut self, cx: &mut App) {
        if self.region.is_some() || self.pending_region.is_some() {
            self.region = None;
            self.pending_region = None;
            self.bars = Bars::default();
            if let Some(r) = &mut self.renderer {
                r.discard();
            }
            self.frames.clear(cx);
        }
    }

    pub fn forget(&mut self) {
        self.frames.forget();
        self.region = None;
        self.pending_region = None;
    }

    /// Drops the renderer (the GPU is going).
    pub fn release(&mut self, cx: &mut App) {
        self.renderer = None;
        self.hide(cx);
    }

    /// Draws the next frame at `place` when one is due.
    pub fn update(
        &mut self,
        tick: &Tick,
        place: Place,
        palette: &[[f32; 4]; 4],
        window: &mut Window,
        cx: &mut App,
    ) {
        if !tick.due {
            return;
        }
        let config = config::get();
        let v = &config.visualizer;
        let Some(region) = region(place, v.style, cx) else {
            return;
        };
        let region = whole_pixels(region, window.scale_factor());
        let scale = window.scale_factor() * render_scale(v.style);
        let size = (
            (f32::from(region.size.width) * scale)
                .round()
                .clamp(1., 2048.) as u32,
            (f32::from(region.size.height) * scale)
                .round()
                .clamp(1., 2048.) as u32,
        );
        let settings = BarSettings {
            count: v.bars as usize,
            sensitivity: v.sensitivity,
            smoothing: v.smoothing,
            decay: v.decay,
            low_hz: v.low_hz,
            high_hz: v.high_hz,
            linear: v.spacing == Spacing::Linear,
            peak_fall: v.peak_fall,
        };
        self.bars
            .update(&tick.levels, &settings, tick.dt.max(1. / 120.));
        self.travel += tick.dt * (0.35 + 1.5 * tick.level);
        let renderer = self.renderer.get_or_insert_with(|| {
            let started = std::time::Instant::now();
            let r = Visualizer::new(tick.gpu, size.0, size.1);
            super::timing::setup(started);
            r
        });
        if renderer.size() != size {
            renderer.resize(size.0, size.1);
        }
        let cover = cover_slot(place)
            .and_then(|slot| Slots::get(cx, slot))
            .map(|c| {
                let o = c.origin - region.origin;
                let r = [
                    f32::from(o.x) * scale,
                    f32::from(o.y) * scale,
                    f32::from(o.x + c.size.width) * scale,
                    f32::from(o.y + c.size.height) * scale,
                ];
                (r, f32::from(cover_radius(place, c.size.width)) * scale)
            });
        let treble = tick.levels[BANDS - 8..].iter().sum::<f32>() / 8.;
        let params = VisualizerParams {
            style: v.style.index(),
            seconds: tick.seconds,
            travel: self.travel,
            bass: tick.bass,
            kick: tick.kick,
            level: tick.level,
            treble,
            look: tick.look,
            opacity: v.opacity,
            glow: v.glow,
            peaks: v.peaks,
            scale,
            cover,
            reach: f32::from(reach(place, cx)) * scale,
            stops: stops(v.palette, &v.custom, palette, tick.look, cx),
            bars: &self.bars,
        };
        match renderer.frame(&params) {
            Ok(Some(frame)) => {
                self.frames.push(frame, window);
                self.region = self.pending_region;
            }
            Ok(None) => {}
            Err(e) => log::warn!("visuals: visualiser frame: {e:#}"),
        }
        self.pending_region = Some(region);
    }
}

/// The scale the style renders at, of device pixels: soft particles look
/// the same at less. The ring's frame edge runs through the scene, where a
/// scaled frame's last texels would show as a faint line, so it renders at
/// full size.
fn render_scale(style: Style) -> f32 {
    match style {
        Style::Particles => 0.5,
        _ => 1.,
    }
}

/// `b` on whole device pixels, so the frame's texels meet the screen's.
fn whole_pixels(b: Bounds<Pixels>, scale: f32) -> Bounds<Pixels> {
    let snap = |v: Pixels| px((f32::from(v) * scale).round() / scale);
    Bounds::from_corners(
        point(snap(b.left()), snap(b.top())),
        point(snap(b.right()), snap(b.bottom())),
    )
}

/// The cover the ring goes round.
fn cover_slot(place: Place) -> Option<Slot> {
    match place {
        Place::NowPlaying => Some(Slot::Cover),
        Place::Stage | Place::Full => Some(Slot::StageCover),
    }
}

/// The cover's corner radius, as the views round it.
fn cover_radius(place: Place, side: Pixels) -> Pixels {
    match place {
        Place::NowPlaying if side >= theme::size::HEADER_COVER => theme::radius::LG,
        Place::NowPlaying => theme::radius::MD,
        Place::Stage | Place::Full => super::stage_cover_radius(side),
    }
}

/// The ring's longest bar, in points.
fn reach(place: Place, cx: &App) -> Pixels {
    let side = cover_slot(place)
        .and_then(|slot| Slots::get(cx, slot))
        .map_or(px(200.), |c| c.size.width);
    match place {
        // Clear of the strip and the title below the cover.
        Place::NowPlaying => super::now_playing_ring_reach(side),
        Place::Stage => super::stage_ring_reach(side),
        Place::Full => super::ring_reach(side),
    }
}

/// Where the visualiser draws, in window coordinates.
fn region(place: Place, style: Style, cx: &App) -> Option<Bounds<Pixels>> {
    let cover = cover_slot(place).and_then(|slot| Slots::get(cx, slot));
    let around = |c: Bounds<Pixels>, by: Pixels| c.dilate(by);
    match place {
        Place::NowPlaying => match style {
            Style::Ring => {
                let page = Slots::get(cx, Slot::Page)?;
                cover.map(|c| around(c, reach(place, cx) + px(24.)).intersect(&page))
            }
            Style::Particles => {
                let page = Slots::get(cx, Slot::Page)?;
                let c = cover?;
                let grown = around(c, c.size.width * 0.3);
                Some(grown.intersect(&page))
            }
            _ => Slots::get(cx, Slot::Spectrum).map(super::visualizer_strip),
        },
        Place::Stage | Place::Full => {
            let body = Slots::get(cx, Slot::StageBody)?;
            let band = if place == Place::Full { 0.26 } else { 0.22 };
            // Stage keeps a band free under its cover and lyrics.
            let kept = super::stage_band(f32::from(body.size.height));
            match style {
                Style::Ring => cover.map(|c| around(c, reach(place, cx) + px(32.))),
                Style::Particles => Slots::get(cx, Slot::Stage),
                Style::Bars | Style::Line | Style::Mirrored => {
                    // Mirrored bars stand on a floor with their reflection
                    // under it: a taller band.
                    let band = if style == Style::Mirrored {
                        band * 1.15
                    } else {
                        band
                    };
                    let h = if place == Place::Stage && kept > px(0.) {
                        kept
                    } else {
                        body.size.height * band
                    };
                    Some(Bounds::new(
                        point(body.left(), body.bottom() - h),
                        size(body.size.width, h),
                    ))
                }
            }
        }
    }
}

/// The gradient's four stops (linear RGB), toned for the look: light and
/// vivid over the dark backdrop, deeper over the light one.
fn stops(
    palette: Palette,
    custom: &[config::Swatch; 2],
    cover: &[[f32; 4]; 4],
    look: Look,
    cx: &App,
) -> [[f32; 3]; 4] {
    let c = theme::colors(cx);
    let rgb = |h: Hsla| {
        let c = h.to_rgb();
        [c.r, c.g, c.b]
    };
    let lab = |display: [f32; 3]| color::oklab(color::to_linear(display));
    let labs: [[f32; 3]; 4] = match palette {
        Palette::Cover => {
            let mut labs = cover.map(|c| lab([c[0], c[1], c[2]]));
            // Lows to highs by hue, so neighbours blend rather than jump.
            labs.sort_by(|a, b| a[2].atan2(a[1]).total_cmp(&b[2].atan2(b[1])));
            labs.map(|l| vivid(l, look, 0.12))
        }
        Palette::Accent => {
            let base = lab(rgb(c.signal));
            let mut out = [base; 4];
            for (i, o) in out.iter_mut().enumerate() {
                *o = turn(base, i as f32 * 14.);
            }
            out.map(|l| vivid(l, look, 0.12))
        }
        Palette::Theme => [lab(rgb(c.text)); 4],
        Palette::Custom => {
            let (a, b) = (lab(custom[0].rgb()), lab(custom[1].rgb()));
            let mix = |t: f32| [0, 1, 2].map(|i| a[i] + (b[i] - a[i]) * t);
            [mix(0.), mix(1. / 3.), mix(2. / 3.), mix(1.)].map(|l| vivid(l, look, 0.))
        }
    };
    labs.map(|l| {
        let lin = color::oklab_to_linear(l);
        lin.map(|v| v.clamp(0., 1.))
    })
}

/// An OKLab colour brought into the look's lightness range, with at least
/// `chroma` of colour (greys stay grey).
fn vivid(lab: [f32; 3], look: Look, chroma: f32) -> [f32; 3] {
    let (lo, hi) = match look {
        Look::Dark => (0.7, 0.9),
        Look::Light => (0.45, 0.6),
    };
    let c = lab[1].hypot(lab[2]);
    let scale = if c > 0.02 { c.max(chroma) / c } else { 1. };
    [lab[0].clamp(lo, hi), lab[1] * scale, lab[2] * scale]
}

/// `lab` with its hue turned by `degrees`.
fn turn(lab: [f32; 3], degrees: f32) -> [f32; 3] {
    let (s, c) = degrees.to_radians().sin_cos();
    [lab[0], lab[1] * c - lab[2] * s, lab[1] * s + lab[2] * c]
}
