//! Track changes on a cover (the player bar's and Now Playing's): the old
//! cover stays over the slot until the new one has loaded, then burns into
//! it along a noise front (`ytfast_visuals::Dissolve`). Painted over the app.
//! Under reduced motion the new cover just replaces the old one.

use std::sync::Arc;
use std::time::{Duration, Instant};

use gpui_kit::*;
use ytfast_visuals::{Dissolve, Gpu, Look};

use super::frames::Frames;
use super::slots::{Slot, Slots};
use crate::theme::{radius, size};

/// How long the burn takes.
/// How long a dissolve takes (Settings → Visuals, 900 ms by default).
fn duration() -> Duration {
    Duration::from_millis(u64::from(super::config::get().dissolve.ms))
}

pub struct Change {
    slot: Slot,
    /// The cover the slot shows, and its decoded image once loaded.
    url: Option<SharedString>,
    image: Option<Arc<RenderImage>>,
    /// The cover being replaced, while the change runs.
    old: Option<Arc<RenderImage>>,
    started: Option<Instant>,
    renderer: Option<Dissolve>,
    frames: Frames,
}

impl Change {
    pub fn new(slot: Slot) -> Self {
        Self {
            slot,
            url: None,
            image: None,
            old: None,
            started: None,
            renderer: None,
            frames: Frames::default(),
        }
    }

    /// A change is on screen: frames should keep coming.
    pub fn active(&self) -> bool {
        self.old.is_some()
    }

    /// Follows the cover `want` in the slot. `on`: the slot shows and may
    /// animate. Draws the next frame when `due`.
    #[allow(clippy::too_many_arguments, reason = "the effects layer's frame state")]
    pub fn update(
        &mut self,
        want: Option<SharedString>,
        on: bool,
        due: bool,
        look: Look,
        accent: [f32; 3],
        gpu: Option<&Gpu>,
        window: &mut Window,
        cx: &mut App,
    ) {
        if want != self.url {
            let old = self.image.take();
            self.url = want;
            self.finish(cx);
            self.old = old.filter(|_| on && gpu.is_some());
        }
        if !on {
            self.finish(cx);
        }
        if self.image.is_none()
            && let Some(url) = &self.url
        {
            let resource = Resource::Uri(SharedUri::from(url.clone()));
            if let Some(Ok(image)) = window.use_asset::<ImgResourceLoader>(&resource, cx) {
                self.image = Some(image);
            }
        }
        let (Some(gpu), Some(new), Some(old)) = (gpu, &self.image, &self.old) else {
            return;
        };
        let first = self.started.is_none();
        if first {
            let Some(bounds) = Slots::get(cx, self.slot) else {
                self.finish(cx);
                return;
            };
            let scale = window.scale_factor();
            let side = |p: Pixels| (f32::from(p) * scale).round().max(1.) as u32;
            let size = (side(bounds.size.width), side(bounds.size.height));
            let renderer = self.renderer.get_or_insert_with(|| {
                let started = std::time::Instant::now();
                let dissolve = Dissolve::new(gpu, size.0, size.1);
                super::timing::setup(started);
                dissolve
            });
            renderer.resize(size.0, size.1);
            renderer.set_covers(pixels(old), pixels(new));
            self.started = Some(Instant::now());
        }
        let t = self.started.map_or(0.0, |at| {
            at.elapsed().as_secs_f32() / duration().as_secs_f32()
        });
        if t >= 1.0 {
            self.finish(cx);
            return;
        }
        if !(due || first) {
            return;
        }
        let edge = match look {
            Look::Dark => accent.map(|c| c + (1.0 - c) * 0.3),
            Look::Light => accent,
        };
        let eased = t * t * (3.0 - 2.0 * t);
        if let Some(renderer) = &mut self.renderer {
            match renderer.frame(eased, look, edge) {
                Ok(Some(frame)) => self.frames.push(frame, window),
                Ok(None) => {}
                Err(e) => log::warn!("visuals: dissolve frame: {e:#}"),
            }
        }
    }

    /// Ends the change: the app's own cover shows again.
    fn finish(&mut self, cx: &mut App) {
        self.old = None;
        self.started = None;
        self.frames.clear(cx);
    }

    /// Drops the renderer (the GPU goes with it).
    pub fn release(&mut self, cx: &mut App) {
        self.finish(cx);
        self.renderer = None;
    }

    pub fn forget(&mut self) {
        self.frames.forget();
        self.old = None;
        self.started = None;
    }

    /// Paints the old cover, or the dissolve's frame, over the slot.
    pub fn paint(&self, window: &mut Window, cx: &App) {
        let (Some(old), Some(bounds)) = (&self.old, Slots::get(cx, self.slot)) else {
            return;
        };
        let corners = Corners::all(corner(bounds.size.width));
        let (image, fitted) = match self.frames.image().filter(|_| self.started.is_some()) {
            Some(frame) => (frame, bounds),
            None => (old.clone(), cover_fit(bounds, old)),
        };
        let _ = window.paint_image(bounds, fitted, corners, image, 0, false);
    }
}

fn pixels(image: &RenderImage) -> (u32, u32, &[u8]) {
    let size = image.size(0);
    (
        size.width.0 as u32,
        size.height.0 as u32,
        image.as_bytes(0).unwrap_or_default(),
    )
}

/// A cover's corner radius at `side`, as `widgets::cover` rounds it.
fn corner(side: Pixels) -> Pixels {
    if side >= size::HEADER_COVER {
        radius::LG
    } else if side >= size::CARD {
        radius::MD
    } else if side >= size::PLAYER_COVER {
        radius::SM
    } else {
        radius::XS
    }
}

/// Fills `bounds` with the image, cropped to keep its aspect ratio.
pub fn cover_fit(bounds: Bounds<Pixels>, image: &RenderImage) -> Bounds<Pixels> {
    let size = image.size(0);
    let (iw, ih) = (size.width.0 as f32, size.height.0 as f32);
    let (bw, bh) = (f32::from(bounds.size.width), f32::from(bounds.size.height));
    let scale = (bw / iw.max(1.)).max(bh / ih.max(1.));
    let fitted = gpui_kit::size(px(iw * scale), px(ih * scale));
    Bounds::new(
        bounds.center() - point(fitted.width / 2., fitted.height / 2.),
        fitted,
    )
}
