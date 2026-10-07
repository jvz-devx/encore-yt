//! The layer under the app: the animated cover backdrop over the page panel
//! and the spectrum, while Now Playing shows.
//!
//! It re-renders on its own timer (`fps()`, 30 by default) and never
//! notifies `MusicApp`. Frames stop when Now Playing is hidden, the window
//! isn't visible (minimised, covered), playback is paused, or motion is
//! reduced (then one still frame is drawn per cover).

use std::cell::Cell;
use std::collections::VecDeque;
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use gpui_kit::component::Colorize as _;
use gpui_kit::*;
use ytfast_visuals::{AudioTap, BANDS, Bands, Cover, FrameCost, FrameParams, Look, Renderer};

use super::slots::{self, Slot, Slots};
use crate::theme::{self, Colors, radius};

/// The backdrop renders at most this wide; GPUI scales it to the panel
/// (a blurred image looks the same at half size).
const MAX_WIDTH: f32 = 640.;
/// The renderer (a second Vulkan device, ~50-90 MB) is dropped this long
/// after Now Playing closes.
const KEEP: Duration = Duration::from_secs(30);

/// What the app tells the layer on each render.
#[derive(Clone, Debug, Default)]
pub struct Input {
    /// Now Playing fills the panel (and effects are on).
    pub showing: bool,
    pub playing: bool,
}

pub struct Effects {
    pub input: Input,
    /// Set on input while the app view is cached (see `super::Layers`).
    input_flag: Rc<Cell<bool>>,
    renderer: Option<Result<Renderer, String>>,
    tap: Option<AudioTap>,
    bands: Bands,
    /// The frame on screen, and the one before it (dropped from GPUI's atlas
    /// a frame later, so its atlas texture isn't freed and made again).
    shown: Option<Arc<RenderImage>>,
    retired: VecDeque<Arc<RenderImage>>,
    /// The cover URL uploaded, and its palette for the fallback gradient.
    cover: Option<SharedString>,
    palette: Option<[[f32; 4]; 4]>,
    look: Look,
    reduce: bool,
    /// Frames still to render although nothing moves (a new cover or size
    /// under reduced motion or while paused; the first frame shows a frame
    /// late).
    pending: u8,
    /// Animation time: moves only while animating, so pause freezes it.
    clock: f32,
    last: Option<Instant>,
    ticker: Option<Task<()>>,
    reaper: Option<Task<()>>,
    stats: Stats,
    window: Option<AnyWindowHandle>,
    _visibility: Option<Subscription>,
}

impl Effects {
    pub fn new(input_flag: Rc<Cell<bool>>) -> Self {
        Self {
            input: Input::default(),
            input_flag,
            renderer: None,
            tap: None,
            bands: Bands::default(),
            shown: None,
            retired: VecDeque::new(),
            cover: None,
            palette: None,
            look: Look::Dark,
            reduce: false,
            pending: 0,
            clock: 0.0,
            last: None,
            ticker: None,
            reaper: None,
            stats: Stats::default(),
            window: None,
            _visibility: None,
        }
    }

    /// A new window has its own atlas: forget frames painted into the old
    /// one, and follow the new one's visibility.
    fn follow_window(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let handle = window.window_handle();
        if self.window == Some(handle) {
            return;
        }
        self.window = Some(handle);
        self.shown = None;
        self.retired.clear();
        self.pending = 2;
        let this = cx.entity().downgrade();
        self._visibility = Some(window.observe_window_visibility(move |visibility, _, cx| {
            log::info!("visuals: window {visibility:?}");
            let _ = this.update(cx, |_, cx| cx.notify());
        }));
    }

    /// Now Playing is hidden: no frames, no tap; the renderer goes later.
    fn rest(&mut self, cx: &mut Context<Self>) {
        self.ticker = None;
        self.tap = None;
        self.last = None;
        if self.renderer.is_none() || self.reaper.is_some() {
            return;
        }
        self.reaper = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(KEEP).await;
            let _ = this.update(cx, |this, cx| this.release(cx));
        }));
    }

    fn release(&mut self, cx: &mut Context<Self>) {
        self.reaper = None;
        if self.input.showing {
            return;
        }
        log::info!("visuals: releasing the backdrop renderer");
        self.renderer = None;
        self.cover = None;
        for image in self.shown.take().into_iter().chain(self.retired.drain(..)) {
            cx.drop_image(image, None);
        }
    }

    fn renderer(&mut self, size: (u32, u32)) -> Option<&mut Renderer> {
        let made = self.renderer.get_or_insert_with(|| {
            let started = Instant::now();
            let made = Renderer::new(size.0, size.1).map_err(|e| format!("{e:#}"));
            match &made {
                Ok(r) => log::info!(
                    "visuals: backdrop {}x{} on {} (set up in {:.0} ms)",
                    size.0,
                    size.1,
                    r.adapter(),
                    started.elapsed().as_secs_f64() * 1000.0
                ),
                Err(e) => log::warn!("visuals: no backdrop, showing a gradient: {e}"),
            }
            made
        });
        let renderer = made.as_mut().ok()?;
        if renderer.size() != size {
            renderer.resize(size.0, size.1);
            self.pending = self.pending.max(2);
        }
        Some(renderer)
    }

    /// Uploads the cover once GPUI has loaded it (the player bar's size,
    /// which is loaded whenever a song plays).
    fn sync_cover(&mut self, size: (u32, u32), reduce: bool, window: &mut Window, cx: &mut App) {
        let Some(url) = slots::Slots::covers(cx).small else {
            return;
        };
        if self.cover.as_ref() == Some(&url) {
            return;
        }
        let resource = Resource::Uri(SharedUri::from(url.clone()));
        let Some(Ok(image)) = window.use_asset::<ImgResourceLoader>(&resource, cx) else {
            return;
        };
        let px = image.size(0);
        let bytes = image.as_bytes(0).unwrap_or_default();
        let cover = Cover::from_bgra(px.width.0 as u32, px.height.0 as u32, bytes);
        self.palette = Some(cover.palette);
        let fade = self.cover.is_some() && !reduce;
        if let Some(renderer) = self.renderer(size) {
            renderer.set_cover(&cover, fade);
        }
        self.cover = Some(url);
        self.pending = self.pending.max(2);
    }

    /// Renders the next frame if something moves or changed.
    fn advance(&mut self, animate: bool, size: (u32, u32), window: &mut Window) {
        let now = Instant::now();
        if !animate {
            self.last = None;
            if self.pending == 0 {
                return;
            }
        }
        // Other redraws (the app's clock, input) don't add frames.
        let since = self.last.map_or(1.0, |l| (now - l).as_secs_f32());
        if animate && self.pending == 0 && since < 0.8 / fps() as f32 {
            return;
        }
        let dt = if self.last.is_some() {
            since.min(0.1)
        } else {
            0.0
        };
        if animate {
            self.last = Some(now);
            self.clock += dt;
        }
        let params = FrameParams {
            seconds: self.clock,
            bass: self.bands.bass,
            kick: self.bands.kick,
            level: self.bands.level,
            look: self.look,
            particles: !self.reduce,
        };
        let Some(renderer) = self.renderer(size) else {
            return;
        };
        match renderer.frame(&params) {
            Ok(Some(frame)) => {
                self.pending = self.pending.saturating_sub(1);
                let cost = frame.cost;
                let image = to_image(frame);
                if let Some(old) = self.shown.replace(image) {
                    self.retired.push_back(old);
                }
                while self.retired.len() > 1 {
                    if let Some(old) = self.retired.pop_front() {
                        let _ = window.drop_image(old);
                    }
                }
                self.stats.record(cost, size);
            }
            Ok(None) => {}
            Err(e) => log::warn!("visuals: frame: {e:#}"),
        }
        if self.pending > 0 && self.ticker.is_none() {
            window.request_animation_frame();
        }
    }

    /// While animating, a timer notifies this view at `fps()`; GPUI would
    /// otherwise draw at the display's rate (120 Hz here) through
    /// `request_animation_frame`. Dropping the task stops it.
    fn pace(&mut self, animate: bool, cx: &mut Context<Self>) {
        if !animate {
            self.ticker = None;
            return;
        }
        if self.ticker.is_some() {
            return;
        }
        let period = Duration::from_secs_f32(1.0 / fps() as f32);
        self.ticker = Some(cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(period).await;
                if this.update(cx, |_, cx| cx.notify()).is_err() {
                    break;
                }
            }
        }));
    }
}

impl Render for Effects {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.follow_window(window, cx);
        let layer = div().absolute().inset_0();
        if !self.input.showing {
            self.rest(cx);
            return layer;
        }
        // Now Playing lays out after this layer: its first frame comes next.
        let Some(panel) = slots::panel(cx) else {
            window.request_animation_frame();
            return layer;
        };
        self.reaper = None;
        let reduce = super::reduced_motion(cx);
        if reduce != self.reduce {
            self.reduce = reduce;
            self.pending = self.pending.max(2);
        }
        let look = match theme::mode(cx) {
            theme::Mode::Dark => Look::Dark,
            theme::Mode::Light => Look::Light,
        };
        if look != self.look {
            self.look = look;
            self.pending = self.pending.max(2);
        }
        let size = render_size(panel);
        self.sync_cover(size, reduce, window, cx);
        let visible = window.is_visible();
        let live = self.input.playing && visible && !reduce;
        if !live {
            self.tap = None;
        }
        self.bands = match &self.tap {
            Some(tap) => tap.bands(),
            None if live => {
                self.tap = Some(AudioTap::start());
                Bands::default()
            }
            None => Bands::default(),
        };
        let fading = matches!(&self.renderer, Some(Ok(r)) if r.fading());
        let animate = visible && !reduce && (self.input.playing || fading);
        self.advance(animate, size, window);
        self.pace(animate, cx);

        let c = theme::colors(cx);
        let paint = Paint {
            image: self.shown.clone(),
            fallback: fallback(self.palette, &c),
            spectrum: live,
            levels: self.bands.levels,
            bar: c.text,
            flag: self.input_flag.clone(),
        };
        layer.child(
            canvas(
                |_, _, _| (),
                move |_, (), window, cx| paint.paint(window, cx),
            )
            .size_full(),
        )
    }
}

/// What the layer paints, captured for the canvas.
struct Paint {
    image: Option<Arc<RenderImage>>,
    fallback: Background,
    /// Whether to draw the spectrum.
    spectrum: bool,
    levels: [f32; BANDS],
    bar: Hsla,
    flag: Rc<Cell<bool>>,
}

impl Paint {
    /// Paints where Now Playing laid its slots out in this frame (it lays
    /// out after this layer renders, before it paints).
    fn paint(self, window: &mut Window, cx: &App) {
        listen_for_input(&self.flag, window);
        let Some(panel) = slots::panel(cx) else {
            return;
        };
        let corners = Corners::all(radius::LG);
        // Under the frame: the gradient shows until the first frame and
        // instead of it when there is no GPU device.
        window.paint_quad(fill(panel, self.fallback).corner_radii(corners));
        if let Some(image) = self.image {
            let fitted = cover_fit(panel, &image);
            let _ = window.paint_image(panel, fitted, corners, image, 0, false);
        }
        if let Some(strip) = Slots::get(cx, Slot::Spectrum).filter(|_| self.spectrum) {
            paint_spectrum(strip, &self.levels, self.bar, window);
        }
    }
}

/// The spectrum as bars mirrored around the strip's centre (lows in the
/// middle), growing up and down from its centre line.
fn paint_spectrum(strip: Bounds<Pixels>, levels: &[f32; BANDS], color: Hsla, window: &mut Window) {
    let count = BANDS * 2;
    let step = strip.size.width / count as f32;
    let width = (step * 0.5).max(px(1.));
    let centre_y = strip.center().y;
    for i in 0..count {
        let band = if i < BANDS { BANDS - 1 - i } else { i - BANDS };
        let level = levels[band];
        let height = (strip.size.height * level.max(0.06)).max(width);
        let x = strip.left() + step * i as f32 + (step - width) / 2.;
        let bar = Bounds::new(point(x, centre_y - height / 2.), size(width, height));
        let color = color.opacity(0.28 + 0.6 * level);
        window.paint_quad(fill(bar, color).corner_radii(Corners::all(width / 2.)));
    }
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

/// The cover's first colour, faded into the surface: what shows before the
/// first frame, or without a GPU device.
fn fallback(palette: Option<[[f32; 4]; 4]>, c: &Colors) -> Background {
    let Some(palette) = palette else {
        return c.surface.into();
    };
    let [r, g, b, _] = palette[0];
    let tint = c.surface.mix_oklab(Rgba { r, g, b, a: 1. }.into(), 0.75);
    linear_gradient(
        160.,
        linear_color_stop(tint, 0.),
        linear_color_stop(c.surface, 1.),
    )
}

/// The render size for a panel: half its size, at most `MAX_WIDTH` wide,
/// rounded to 16 px so small resizes don't re-make the targets.
fn render_size(panel: Bounds<Pixels>) -> (u32, u32) {
    let (w, h) = (f32::from(panel.size.width), f32::from(panel.size.height));
    let scale = (MAX_WIDTH / w).min(0.5);
    let round = |v: f32| ((v * scale / 16.).round().max(1.) * 16.) as u32;
    (round(w), round(h))
}

/// Fills `bounds` with the image, cropped to keep its aspect ratio.
fn cover_fit(bounds: Bounds<Pixels>, image: &RenderImage) -> Bounds<Pixels> {
    let size = image.size(0);
    let (iw, ih) = (size.width.0 as f32, size.height.0 as f32);
    let (bw, bh) = (f32::from(bounds.size.width), f32::from(bounds.size.height));
    let scale = (bw / iw).max(bh / ih);
    let fitted = gpui_kit::size(px(iw * scale), px(ih * scale));
    Bounds::new(
        bounds.center() - point(fitted.width / 2., fitted.height / 2.),
        fitted,
    )
}

fn to_image(frame: ytfast_visuals::Frame) -> Arc<RenderImage> {
    let pixels = image::RgbaImage::from_raw(frame.width, frame.height, frame.bgra)
        .expect("a frame's size matches its bytes");
    Arc::new(RenderImage::new([image::Frame::new(pixels)]))
}

/// Backdrop frames per second: `YTFAST_GPUI_VISUALS_FPS`, 30 by default.
fn fps() -> u32 {
    std::env::var("YTFAST_GPUI_VISUALS_FPS")
        .ok()
        .and_then(|v| v.parse().ok())
        .filter(|&f| f > 0)
        .unwrap_or(30)
}

/// Frame costs, logged every five seconds while animating.
#[derive(Default)]
struct Stats {
    frames: u32,
    cost: FrameCost,
    since: Option<Instant>,
}

impl Stats {
    fn record(&mut self, cost: FrameCost, size: (u32, u32)) {
        let since = *self.since.get_or_insert_with(Instant::now);
        self.frames += 1;
        self.cost.submit += cost.submit;
        self.cost.wait += cost.wait;
        self.cost.copy += cost.copy;
        if since.elapsed() < Duration::from_secs(5) {
            return;
        }
        let n = self.frames as f32;
        log::info!(
            "visuals: {}x{}: {:.0} fps; submit {:.2} ms, wait {:.2} ms, copy {:.2} ms",
            size.0,
            size.1,
            n / since.elapsed().as_secs_f32(),
            self.cost.submit / n,
            self.cost.wait / n,
            self.cost.copy / n,
        );
        *self = Self::default();
    }
}
