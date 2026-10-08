//! Painting and slot geometry for the effects layer. The scheduler hands
//! this module an owned frame snapshot; painting does not advance effects.

use std::cell::Cell;
use std::rc::Rc;

use encore_visuals::BANDS;
use gpui_kit::*;

use super::Effects;
use crate::theme::{self, radius, size};
use crate::visuals::ambient::Field;
use crate::visuals::frames::PaintLayer;
use crate::visuals::slots::{self, Slot, Slots};
use crate::visuals::{backdrop, bar, dissolve, waveform};

/// What the layer paints, captured for the canvas.
pub(super) struct Paint {
    pub(super) backdrop: Option<std::sync::Arc<RenderImage>>,
    /// The sparkles over the backdrop.
    pub(super) sparkles: Option<Field>,
    pub(super) fallback: Background,
    pub(super) showing: bool,
    /// The backdrop fills a scene (Stage, the full-window visualiser).
    pub(super) scene: bool,
    /// A 3D scene fills the backdrop's place (painted as the visualiser).
    pub(super) fills: bool,
    /// The visualiser's frame, where it goes and its corners.
    pub(super) vis: Option<(std::sync::Arc<RenderImage>, Bounds<Pixels>, Corners<Pixels>)>,
    pub(super) bar: Option<std::sync::Arc<RenderImage>>,
    /// Whether to draw the spectrum.
    pub(super) spectrum: bool,
    pub(super) levels: [f32; BANDS],
    /// Now Playing's waveform: its outline (once decoded) and progress.
    pub(super) waveform: Option<Waveform>,
    pub(super) color: Hsla,
    pub(super) flag: Rc<Cell<bool>>,
    pub(super) hover: Rc<Cell<bool>>,
    pub(super) this: WeakEntity<Effects>,
}

impl Paint {
    /// Paints where the views laid their slots out in this frame (they lay
    /// out after this layer renders, before it paints).
    pub(super) fn paint(self, window: &mut Window, cx: &mut App) {
        listen_for_input(&self.flag, window);
        if let Some(image) = self.bar
            && let Some(bar) = Slots::get(cx, Slot::Bar)
        {
            PaintLayer::Bar.report(window.paint_image(
                bar,
                bar,
                Corners::default(),
                image,
                0,
                false,
            ));
            follow_hover(self.hover.clone(), self.this.clone(), window);
        }
        let area =
            backdrop_area(self.showing, cx).filter(|_| (self.showing || self.scene) && !self.fills);
        if let Some((panel, corners)) = area {
            // The gradient shows until the first frame and instead of it
            // when there is no GPU device. Not under the frame: each layer
            // over the whole panel costs GPU time in every window frame.
            match self.backdrop {
                Some(image) => {
                    let fitted = dissolve::cover_fit(panel, &image);
                    PaintLayer::Backdrop
                        .report(window.paint_image(panel, fitted, corners, image, 0, false));
                }
                None => window.paint_quad(fill(panel, self.fallback).corner_radii(corners)),
            }
            if let Some(sparkles) = &self.sparkles {
                sparkles.paint(panel, window);
            }
        }
        if let Some((image, bounds, corners)) = self.vis {
            PaintLayer::Visualizer
                .report(window.paint_image(bounds, bounds, corners, image, 0, false));
        }
        if !self.showing {
            return;
        }
        if let Some(strip) = Slots::get(cx, Slot::Spectrum).filter(|_| self.spectrum) {
            paint_spectrum(strip, &self.levels, self.color, window);
        }
        if let Some((w, bounds)) = self.waveform.zip(Slots::get(cx, Slot::Waveform)) {
            waveform::paint(bounds, w.values.as_deref(), w.progress, w.colors, window);
        }
    }
}

/// What Now Playing's waveform shows.
pub(super) struct Waveform {
    pub(super) values: Option<Vec<f32>>,
    pub(super) progress: f32,
    pub(super) colors: (Hsla, Hsla),
}

impl Waveform {
    pub(super) fn new(bar: Option<&bar::Input>, c: &theme::Colors, cx: &App) -> Self {
        let values = bar
            .and_then(|b| b.video_id.as_deref())
            .and_then(|id| waveform::outline(id, cx));
        Self {
            values,
            progress: bar.map_or(0.0, |b| b.progress),
            colors: waveform::colors(c),
        }
    }
}

/// Where the backdrop goes: Now Playing's panel (rounded), or the whole
/// scene (Stage, the full-window visualiser).
pub(super) fn backdrop_area(
    now_playing: bool,
    cx: &App,
) -> Option<(Bounds<Pixels>, Corners<Pixels>)> {
    if now_playing {
        slots::panel(cx).map(|p| (p, Corners::all(radius::LG)))
    } else {
        Slots::get(cx, Slot::Stage).map(|s| (s, Corners::default()))
    }
}

/// The large cover (Now Playing's, or the scene's), whose shadow the
/// backdrop draws, unless the cover is flying (the flight draws its own).
pub(super) fn cover_shadow(now_playing: bool, cx: &App) -> Option<backdrop::Shadow> {
    if now_playing && crate::visuals::cover_in_flight(cx) {
        return None;
    }
    let slot = if now_playing {
        Slot::Cover
    } else {
        Slot::StageCover
    };
    let cover = Slots::get(cx, slot)?;
    // As `widgets::cover` and Stage round it.
    let radius = if !now_playing {
        crate::visuals::stage_cover_radius(cover.size.width)
    } else if cover.size.width >= size::HEADER_COVER {
        radius::LG
    } else {
        radius::MD
    };
    Some(backdrop::Shadow {
        cover,
        radius,
        opacity: theme::colors(cx).shadow.a,
    })
}

/// The spectrum as bars mirrored around the strip's centre (lows in the
/// middle), growing up and down from its centre line.
fn paint_spectrum(strip: Bounds<Pixels>, levels: &[f32; BANDS], color: Hsla, window: &mut Window) {
    let count = BANDS * 2;
    let step = strip.size.width / count as f32;
    let width = (step * 0.5).max(px(1.));
    let centre_y = strip.center().y;
    // Like the waveform, the spectrum needs one position in the scene's
    // order, not one bounds-tree insertion per bar.
    window.paint_layer(strip, |window| {
        for i in 0..count {
            let band = if i < BANDS { BANDS - 1 - i } else { i - BANDS };
            let level = levels[band];
            let height = (strip.size.height * level.max(0.06)).max(width);
            let x = strip.left() + step * i as f32 + (step - width) / 2.;
            let bar = Bounds::new(point(x, centre_y - height / 2.), size(width, height));
            let color = color.opacity(0.28 + 0.6 * level);
            window.paint_quad(fill(bar, color).corner_radii(Corners::all(width / 2.)));
        }
    });
}

/// The seek bar's playhead grows under the pointer: a move across its edge
/// redraws the strip.
fn follow_hover(hover: Rc<Cell<bool>>, this: WeakEntity<Effects>, window: &mut Window) {
    window.on_mouse_event(move |e: &MouseMoveEvent, phase, _, cx| {
        if phase != DispatchPhase::Capture {
            return;
        }
        let over = Slots::get(cx, Slot::Seek).is_some_and(|b| b.contains(&e.position));
        if over != hover.get() {
            hover.set(over);
            let _ = this.update(cx, |_, cx| cx.notify());
        }
    });
}

/// Mouse presses, drags and scrolling may change models the cached app
/// view doesn't watch (a slider's state): the next frame renders it afresh.
fn listen_for_input(flag: &Rc<Cell<bool>>, window: &mut Window) {
    let set = |flag: &Rc<Cell<bool>>| {
        let flag = flag.clone();
        move || flag.set(true)
    };
    let down = set(flag);
    window.on_mouse_event(move |_: &MouseDownEvent, phase, _, _| {
        if phase == DispatchPhase::Capture {
            down();
        }
    });
    let up = set(flag);
    window.on_mouse_event(move |_: &MouseUpEvent, phase, _, _| {
        if phase == DispatchPhase::Capture {
            up();
        }
    });
    let drag = set(flag);
    window.on_mouse_event(move |e: &MouseMoveEvent, phase, _, _| {
        if phase == DispatchPhase::Capture && e.pressed_button.is_some() {
            drag();
        }
    });
    let scroll = set(flag);
    window.on_mouse_event(move |_: &ScrollWheelEvent, phase, _, _| {
        if phase == DispatchPhase::Capture {
            scroll();
        }
    });
}
