//! The layer under the app: the animated cover backdrop over the page panel
//! and the spectrum while Now Playing shows, and the player bar's strip
//! (glow, seek bar, halos) whenever a song plays. It also runs the cover
//! dissolves, which the shell paints over the app ([`Effects::overlay`]).
//!
//! It re-renders on its own timer (`fps()`, 30 by default) and never
//! notifies `MusicApp`. Frames stop when the window isn't visible
//! (minimised), playback is paused, or motion is reduced (then a still
//! frame is drawn when something changes). One `ytfast_visuals::Gpu` serves
//! every effect; it is dropped once nothing has drawn for `KEEP`.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::{Duration, Instant};

use gpui_kit::*;
use ytfast_visuals::{AudioTap, BANDS, Bands, Cover, Gpu, Look};

use super::backdrop::Backdrop;
use super::bar::{self, Bar};
use super::dissolve::{self, Change};
use super::slots::{self, Slot, Slots};
use crate::theme::{self, radius};

/// The backdrop renderer is dropped this long after Now Playing closes, and
/// the GPU device (a second Vulkan device, ~50-90 MB) this long after the
/// last frame of any effect.
const KEEP: Duration = Duration::from_secs(30);
/// While only the player bar moves, its slow drift is drawn at this rate;
/// beats and the playhead bring frames up to `fps()`.
const DRIFT_FPS: f32 = 6.;
/// A change in the kick or bass that is worth a frame of the bar, and the
/// rate beats draw at most.
const BEAT_STEP: f32 = 0.06;
const BEAT_FPS: f32 = 20.;

/// What the app tells the layer on each render.
#[derive(Clone, Debug, Default)]
pub struct Input {
    /// Now Playing fills the panel (and effects are on).
    pub showing: bool,
    pub playing: bool,
    /// Now Playing is open (the bar's cover is its close button then).
    pub now_playing: bool,
    /// The player bar's state, or `None` while the bar isn't on screen or
    /// effects are off.
    pub bar: Option<bar::Input>,
}

/// One render's frame state, shared by the effects.
pub struct Tick<'a> {
    /// A paced animation frame is due.
    pub due: bool,
    /// Seconds since the previous paced frame.
    pub dt: f32,
    /// Animation time: moves only while animating.
    pub seconds: f32,
    pub bass: f32,
    pub kick: f32,
    pub level: f32,
    pub look: Look,
    pub reduce: bool,
    pub gpu: &'a Gpu,
}

/// The cover the player bar shows, decoded, and its palette.
struct Art {
    url: SharedString,
    cover: Cover,
}

pub struct Effects {
    pub input: Input,
    /// Set on input while the app view is cached (see `super::Layers`).
    input_flag: Rc<Cell<bool>>,
    gpu: Option<Result<Gpu, String>>,
    /// Waits for the device while it is being made.
    gpu_wait: Option<Task<()>>,
    tap: Option<AudioTap>,
    bands: Bands,
    art: Option<Art>,
    backdrop: Backdrop,
    bar: Bar,
    changes: Rc<RefCell<[Change; 2]>>,
    look: Look,
    reduce: bool,
    clock: f32,
    last: Option<Instant>,
    /// Only the player bar moves: frames come when it would look different.
    bar_only: bool,
    /// The kick and bass the bar's last paced frame showed, and when.
    shown: (Option<Instant>, f32, f32),
    /// How fast the playhead moves, in device pixels a second.
    head_speed: f32,
    /// The last frame any effect drew, for dropping the GPU when idle.
    drew_at: Instant,
    /// When Now Playing was last shown, for dropping the backdrop.
    shown_at: Instant,
    ticker: Option<Task<()>>,
    reaper: Option<Task<()>>,
    window: Option<AnyWindowHandle>,
    _visibility: Option<Subscription>,
}

impl Effects {
    pub fn new(input_flag: Rc<Cell<bool>>) -> Self {
        Self {
            input: Input::default(),
            input_flag,
            gpu: None,
            gpu_wait: None,
            tap: None,
            bands: Bands::default(),
            art: None,
            backdrop: Backdrop::default(),
            bar: Bar::new(),
            changes: Rc::new(RefCell::new([
                Change::new(Slot::BarCover),
                Change::new(Slot::Cover),
            ])),
            look: Look::Dark,
            reduce: false,
            clock: 0.0,
            last: None,
            bar_only: false,
            shown: (None, 0.0, 0.0),
            head_speed: 0.0,
            drew_at: Instant::now(),
            shown_at: Instant::now(),
            ticker: None,
            reaper: None,
            window: None,
            _visibility: None,
        }
    }

    /// The cover dissolves, painted by the shell over the app.
    pub fn overlay(&self) -> impl IntoElement {
        let changes = self.changes.clone();
        canvas(
            |_, _, _| (),
            move |_, (), window, cx| {
                for change in changes.borrow().iter() {
                    change.paint(window, cx);
                }
            },
        )
        .absolute()
        .inset_0()
    }

    /// A new window has its own atlas: forget frames painted into the old
    /// one, and follow the new one's visibility.
    fn follow_window(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let handle = window.window_handle();
        if self.window == Some(handle) {
            return;
        }
        self.window = Some(handle);
        self.backdrop.forget();
        self.bar.forget();
        for change in self.changes.borrow_mut().iter_mut() {
            change.forget();
        }
        let this = cx.entity().downgrade();
        self._visibility = Some(window.observe_window_visibility(move |visibility, _, cx| {
            log::info!("visuals: window {visibility:?}");
            let _ = this.update(cx, |_, cx| cx.notify());
        }));
    }

    /// The GPU device, once [`device`](super::device) has made it; until
    /// then the effects are off and the app paints its plain bar.
    fn gpu(&mut self, cx: &mut Context<Self>) -> Option<Gpu> {
        match &self.gpu {
            Some(made) => made.as_ref().ok().cloned(),
            None => {
                if self.gpu_wait.is_none() {
                    let made = super::device::start();
                    self.gpu_wait = Some(cx.spawn(async move |this, cx| {
                        let made = made
                            .recv()
                            .await
                            .unwrap_or_else(|_| Err("the GPU thread ended".into()));
                        let _ = this.update(cx, |this, cx| {
                            this.gpu = Some(made);
                            cx.notify();
                        });
                    }));
                }
                None
            }
        }
    }

    /// Decodes the player bar's cover once GPUI has loaded it, and hands
    /// its palette to the bar (and the backdrop's fallback).
    fn sync_art(&mut self, window: &mut Window, cx: &mut App) {
        let Some(url) = Slots::covers(cx).small else {
            if self.art.take().is_some() {
                self.bar.set_palette(None, self.reduce);
            }
            return;
        };
        if self.art.as_ref().is_some_and(|a| a.url == url) {
            return;
        }
        let resource = Resource::Uri(SharedUri::from(url.clone()));
        let Some(Ok(image)) = window.use_asset::<ImgResourceLoader>(&resource, cx) else {
            return;
        };
        let px = image.size(0);
        let bytes = image.as_bytes(0).unwrap_or_default();
        let cover = Cover::from_bgra(px.width.0 as u32, px.height.0 as u32, bytes);
        let first = self.art.is_none();
        self.bar
            .set_palette(Some(cover.palette), first || self.reduce);
        self.backdrop.set_fallback(cover.palette);
        self.art = Some(Art { url, cover });
    }

    /// Whether a paced frame is due, moving the animation clock if so.
    fn tick(&mut self, animate: bool) -> (bool, f32) {
        let now = Instant::now();
        if !animate {
            self.last = None;
            return (false, 0.0);
        }
        // Other redraws (the app's clock, input) don't add frames.
        let since = self.last.map_or(1.0, |l| (now - l).as_secs_f32());
        if since < 0.8 / fps() as f32 {
            return (false, 0.0);
        }
        let dt = if self.last.is_some() {
            since.min(0.2)
        } else {
            0.0
        };
        self.last = Some(now);
        self.clock += dt;
        (true, dt)
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
                let woke = this.update(cx, |this, cx| {
                    if this.wants_frame() {
                        cx.notify();
                    }
                });
                if woke.is_err() {
                    break;
                }
            }
        }));
    }

    /// Whether the next paced frame would look different. Each frame
    /// redraws the whole window (about 2 ms of CPU here), so while only the
    /// player bar moves, frames come for a beat, a playhead that moved half
    /// a pixel, or the drift at `DRIFT_FPS`.
    fn wants_frame(&self) -> bool {
        let (Some(at), kick, bass) = self.shown else {
            return true;
        };
        if !self.bar_only {
            return true;
        }
        let since = at.elapsed().as_secs_f32();
        if since >= 1.0 / DRIFT_FPS || since * self.head_speed >= 0.5 {
            return true;
        }
        if since < 1.0 / BEAT_FPS {
            return false;
        }
        let bands = self.tap.as_ref().map(AudioTap::bands).unwrap_or_default();
        (bands.kick - kick).abs() > BEAT_STEP || (bands.bass - bass).abs() > BEAT_STEP
    }

    /// The song's position moved: while the ticker runs, its next frame
    /// shows it; otherwise this draws one (a still frame if the playhead
    /// moved).
    pub fn wake(&mut self, cx: &mut Context<Self>) {
        if self.ticker.is_none() {
            cx.notify();
        }
    }

    /// Checks every `KEEP` whether the backdrop or the GPU can go.
    fn keep_reaping(&mut self, cx: &mut Context<Self>) {
        if self.reaper.is_some() || self.gpu.is_none() {
            return;
        }
        self.reaper = Some(cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(KEEP).await;
                let holding = this.update(cx, |this, cx| this.reap(cx));
                if !matches!(holding, Ok(true)) {
                    break;
                }
            }
            let _ = this.update(cx, |this, _| this.reaper = None);
        }));
    }

    /// Drops what has been idle for `KEEP`; whether anything is still held.
    fn reap(&mut self, cx: &mut App) -> bool {
        if !self.input.showing && self.backdrop.has_renderer() && self.shown_at.elapsed() >= KEEP {
            self.backdrop.release(cx);
        }
        if self.drew_at.elapsed() >= KEEP && !self.input.showing {
            log::info!("visuals: idle, releasing the GPU");
            self.bar.release();
            for change in self.changes.borrow_mut().iter_mut() {
                change.release(cx);
            }
            if matches!(self.gpu, Some(Ok(_))) {
                self.gpu = None;
            }
        }
        self.gpu.is_some()
    }

    /// The tap runs while something on screen moves with the music.
    fn listen(&mut self, live: bool) {
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
    }

    /// Follows the reduced-motion setting and the theme; either redraws the
    /// still pictures.
    fn follow_settings(&mut self, cx: &App) {
        let reduce = super::reduced_motion(cx);
        let look = match theme::mode(cx) {
            theme::Mode::Dark => Look::Dark,
            theme::Mode::Light => Look::Light,
        };
        if reduce != self.reduce || look != self.look {
            self.reduce = reduce;
            self.look = look;
            self.backdrop.redraw();
        }
    }

    /// Tells the bar whether to leave its background to this layer; when
    /// that changes, the cached app view renders afresh.
    fn hand_over_bar(&mut self, window: &mut Window, cx: &App) {
        let painted = self.input.bar.is_some() && self.bar.image().is_some();
        if slots::paints_bar(cx) != painted {
            Slots::set_bar_painted(cx, painted);
            self.input_flag.set(true);
            window.request_animation_frame();
        }
    }
}

impl Render for Effects {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.follow_window(window, cx);
        self.follow_settings(cx);
        let visible = window.is_visible();
        let input = self.input.clone();
        let moving = visible && !self.reduce;
        let music = input.playing && (input.showing || input.bar.is_some());
        self.listen(moving && music);
        self.sync_art(window, cx);
        let changing = self.changes.borrow().iter().any(Change::active);
        let fading = (input.showing && self.backdrop.fading()) || self.bar.fading();
        let animate = moving && (music || fading || changing);
        let (due, dt) = self.tick(animate);
        self.bar_only = !input.showing && !fading && !changing;
        if due {
            self.shown = (Some(Instant::now()), self.bands.kick, self.bands.bass);
        }
        self.head_speed = head_speed(input.bar.as_ref(), window.scale_factor(), cx);
        if input.showing {
            self.shown_at = Instant::now();
        }
        let gpu = (input.showing || input.bar.is_some() || changing)
            .then(|| self.gpu(cx))
            .flatten();

        if let Some(gpu) = &gpu {
            let tick = Tick {
                due,
                dt,
                seconds: self.clock,
                bass: self.bands.bass,
                kick: self.bands.kick,
                level: self.bands.level,
                look: self.look,
                reduce: self.reduce,
                gpu,
            };
            let art = self.art.as_ref().map(|a| (&a.url, &a.cover));
            // Now Playing lays out after this layer: its first frame comes
            // next.
            match slots::panel(cx).filter(|_| input.showing) {
                Some(panel) => self.backdrop.update(&tick, panel, art, window),
                None if input.showing => window.request_animation_frame(),
                None => {}
            }
            if let Some(bar) = input.bar.as_ref().filter(|_| visible) {
                // The bar lays out after this layer: its first frame comes next.
                if Slots::get(cx, Slot::Bar).is_none() {
                    window.request_animation_frame();
                }
                self.bar.update(&tick, bar, window, cx);
            }
            let covers = Slots::covers(cx);
            let accent = self.bar.accent();
            let in_flight = super::cover_in_flight(cx);
            let ons = [
                input.bar.is_some() && !input.now_playing && moving,
                input.showing && !in_flight && moving,
            ];
            let wants = [covers.small, covers.large];
            for ((change, want), on) in self.changes.borrow_mut().iter_mut().zip(wants).zip(ons) {
                change.update(want, on, due, self.look, accent, Some(gpu), window, cx);
            }
            if due || self.backdrop.pending() || self.bar.pending() {
                self.drew_at = Instant::now();
            }
        }
        self.hand_over_bar(window, cx);
        // Still pictures to finish, for the effects that are on.
        let pending = gpu.is_some()
            && ((input.showing && self.backdrop.pending())
                || (input.bar.is_some() && visible && self.bar.pending()));
        if pending && self.ticker.is_none() {
            window.request_animation_frame();
        }
        self.pace(animate, cx);
        self.keep_reaping(cx);

        let c = theme::colors(cx);
        let paint = Paint {
            backdrop: input.showing.then(|| self.backdrop.image()).flatten(),
            fallback: self.backdrop.fallback(&c),
            showing: input.showing,
            bar: input.bar.is_some().then(|| self.bar.image()).flatten(),
            spectrum: moving && input.playing && input.showing,
            levels: self.bands.levels,
            color: c.text,
            flag: self.input_flag.clone(),
            hover: self.bar.hover.clone(),
            this: cx.entity().downgrade(),
        };
        div().absolute().inset_0().child(
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
    backdrop: Option<std::sync::Arc<RenderImage>>,
    fallback: Background,
    showing: bool,
    bar: Option<std::sync::Arc<RenderImage>>,
    /// Whether to draw the spectrum.
    spectrum: bool,
    levels: [f32; BANDS],
    color: Hsla,
    flag: Rc<Cell<bool>>,
    hover: Rc<Cell<bool>>,
    this: WeakEntity<Effects>,
}

impl Paint {
    /// Paints where the views laid their slots out in this frame (they lay
    /// out after this layer renders, before it paints).
    fn paint(self, window: &mut Window, cx: &mut App) {
        listen_for_input(&self.flag, window);
        if let Some(image) = self.bar
            && let Some(bar) = Slots::get(cx, Slot::Bar)
        {
            let _ = window.paint_image(bar, bar, Corners::default(), image, 0, false);
            follow_hover(self.hover.clone(), self.this.clone(), window);
        }
        if !self.showing {
            return;
        }
        let Some(panel) = slots::panel(cx) else {
            return;
        };
        let corners = Corners::all(radius::LG);
        // Under the frame: the gradient shows until the first frame and
        // instead of it when there is no GPU device.
        window.paint_quad(fill(panel, self.fallback).corner_radii(corners));
        if let Some(image) = self.backdrop {
            let fitted = dissolve::cover_fit(panel, &image);
            let _ = window.paint_image(panel, fitted, corners, image, 0, false);
        }
        if let Some(strip) = Slots::get(cx, Slot::Spectrum).filter(|_| self.spectrum) {
            paint_spectrum(strip, &self.levels, self.color, window);
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

/// The playhead's speed in device pixels a second.
fn head_speed(bar: Option<&bar::Input>, scale: f32, cx: &App) -> f32 {
    let (Some(bar), Some(seek)) = (bar, Slots::get(cx, Slot::Seek)) else {
        return 0.0;
    };
    if bar.duration <= 0.0 {
        return 0.0;
    }
    f32::from(seek.size.width) * scale / bar.duration as f32
}

/// Frames per second at most: `YTFAST_GPUI_VISUALS_FPS`, 30 by default.
fn fps() -> u32 {
    std::env::var("YTFAST_GPUI_VISUALS_FPS")
        .ok()
        .and_then(|v| v.parse().ok())
        .filter(|&f| f > 0)
        .unwrap_or(30)
}
