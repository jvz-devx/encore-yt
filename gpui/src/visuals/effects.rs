//! The layer under the app: the animated cover backdrop over the page panel
//! and the spectrum while Now Playing shows, and the player bar's strip
//! (glow, seek bar, halos) whenever a song plays. It also runs the cover
//! dissolves, which the shell paints over the app ([`Effects::overlay`]).
//!
//! It re-renders on its own timer (`fps()`, 20 a second by default on
//! Linux, or with the display when Settings → Visuals' frame rate says so)
//! and never
//! notifies `MusicApp`. Frames stop when the window isn't visible
//! (minimised), playback is paused, or motion is reduced (then a still
//! frame is drawn when something changes). One `ytfast_visuals::Gpu` serves
//! every effect, made in the background ([`Device`]); it is dropped once
//! nothing has drawn for `KEEP`.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::{Duration, Instant};

use gpui_kit::*;
use ytfast_visuals::{AudioTap, BANDS, Bands, Cover, Gpu, Look};

use super::ambient::{Clocks, Field};
use super::backdrop::{self, Backdrop};
use super::bar::{self, Bar};
use super::device::Device;
use super::dissolve::{self, Change};
use super::slots::{self, Slot, Slots};
use super::visualizer::{Place, Vis};
use super::waveform;
use crate::theme::{self, radius, size};

/// The backdrop renderer is dropped this long after Now Playing closes, and
/// the GPU device (a second Vulkan device, ~50-90 MB) this long after the
/// last frame of any effect.
const KEEP: Duration = Duration::from_secs(30);
/// The player bar's slow drift is drawn at this rate; beats and the
/// playhead add frames up to `BEAT_FPS`.
const DRIFT_FPS: f32 = 3.;
/// A change in the kick or bass that is worth a frame of the bar, and the
/// rate beats draw at most.
const BEAT_STEP: f32 = 0.12;
const BEAT_FPS: f32 = 10.;
/// The backdrop draws with every other paced frame (it moves slowly and is
/// blurred; the sparkles over it move with every frame), at least this
/// often and at most `BACKDROP_MAX_FPS`.
const BACKDROP_MIN_FPS: f32 = 10.;
const BACKDROP_MAX_FPS: f32 = 30.;
/// The ticker's rate while only the player bar moves and the frame rate
/// follows the display (its frames come on beats and the playhead).
const BAR_ONLY_FPS: u32 = 30;

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
    /// Stage or the full-window visualiser fills the window (and effects
    /// are on).
    pub scene: Option<Place>,
}

/// One render's frame state, shared by the effects.
#[derive(Clone, Copy)]
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
    /// The spectrum's bands.
    pub levels: [f32; BANDS],
    pub look: Look,
    pub reduce: bool,
    pub gpu: &'a Gpu,
    /// What the engine plays, for the scope's samples.
    pub tap: Option<&'a AudioTap>,
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
    device: Device,
    tap: Option<AudioTap>,
    bands: Bands,
    art: Option<Art>,
    backdrop: Backdrop,
    /// The sparkles' clocks.
    sparkles: Clocks,
    bar: Bar,
    vis: Vis,
    changes: Rc<RefCell<[Change; 2]>>,
    look: Look,
    reduce: bool,
    clock: f32,
    last: Option<Instant>,
    /// Only the player bar moves: frames come when it would look different.
    bar_only: bool,
    /// The kick and bass the bar's last paced frame showed, and when.
    shown: (Option<Instant>, f32, f32),
    /// When the backdrop last drew a paced frame.
    backdrop_at: Option<Instant>,
    /// When a position tick last woke this layer without the ticker.
    woke: Option<Instant>,
    counts: Counts,
    /// How fast the playhead moves, in device pixels a second.
    head_speed: f32,
    /// The last frame any effect drew, for dropping the GPU when idle.
    drew_at: Instant,
    /// When Now Playing was last shown, for dropping the backdrop.
    shown_at: Instant,
    ticker: Option<Task<()>>,
    /// The ticker's period, to start a new one when the frame rate changes.
    ticker_period: Duration,
    reaper: Option<Task<()>>,
    window: Option<AnyWindowHandle>,
    _visibility: Option<Subscription>,
}

impl Effects {
    /// The layer; `cache` is the app's cache directory (for the pipeline
    /// cache).
    pub fn new(input_flag: Rc<Cell<bool>>, cache: &std::path::Path) -> Self {
        Self {
            input: Input::default(),
            input_flag,
            device: Device::new(cache),
            tap: None,
            bands: Bands::default(),
            art: None,
            backdrop: Backdrop::default(),
            sparkles: Clocks::default(),
            bar: Bar::new(),
            vis: Vis::default(),
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
            backdrop_at: None,
            woke: None,
            counts: Counts::default(),
            head_speed: 0.0,
            drew_at: Instant::now(),
            shown_at: Instant::now(),
            ticker: None,
            ticker_period: Duration::ZERO,
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
        self.vis.forget();
        for change in self.changes.borrow_mut().iter_mut() {
            change.forget();
        }
        let this = cx.entity().downgrade();
        self._visibility = Some(window.observe_window_visibility(move |visibility, _, cx| {
            log::info!("visuals: window {visibility:?}");
            let _ = this.update(cx, |_, cx| cx.notify());
        }));
    }

    /// The device, once made. Asks for it after this frame when `need`
    /// (an effect would draw) or, once, as the warm-up after the first
    /// frame; nothing waits for it.
    fn gpu(&mut self, need: bool, window: &mut Window, cx: &mut Context<Self>) -> Option<Gpu> {
        let gpu = self.device.gpu().filter(|_| need).cloned();
        let warm_up = !need && window.is_visible() && super::enabled();
        if gpu.is_none() && (need || warm_up) && self.device.schedule(!need) {
            cx.on_next_frame(window, |this, _, cx| this.device.start(cx));
        }
        gpu
    }

    /// The device made in the background ([`Device::start`]) is ready:
    /// kept for `KEEP` from now, and the effects draw.
    pub fn device_made(
        &mut self,
        made: anyhow::Result<Gpu>,
        started: Instant,
        cx: &mut Context<Self>,
    ) {
        self.device.finish(made, started);
        self.drew_at = Instant::now();
        self.keep_reaping(cx);
        cx.notify();
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
        if self.rate().is_some_and(|fps| since < 0.8 / fps as f32) {
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

    /// The paced frames' rate now: `fps()`, or with the display `None`
    /// (unless only the bar moves).
    fn rate(&self) -> Option<u32> {
        fps().or(self.bar_only.then_some(BAR_ONLY_FPS))
    }

    /// While animating, a timer notifies this view at `fps()`; GPUI would
    /// otherwise draw at the display's rate (120 Hz here) through
    /// `request_animation_frame`, which is what the display's rate asks
    /// for. Dropping the task stops it.
    fn pace(&mut self, animate: bool, window: &mut Window, cx: &mut Context<Self>) {
        let fps = self.rate();
        if !animate || fps.is_none() {
            self.ticker = None;
            if animate {
                window.request_animation_frame();
            }
            return;
        }
        let period = Duration::from_secs_f32(1.0 / fps.unwrap_or(BAR_ONLY_FPS) as f32);
        if self.ticker.is_some() && self.ticker_period == period {
            return;
        }
        self.ticker_period = period;
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
    /// redraws the whole window (about 2 ms of CPU and 8-10 ms of GPU time
    /// here), so while only the player bar moves, frames come only when it
    /// would look different ([`Self::bar_wants`]).
    fn wants_frame(&self) -> bool {
        !self.bar_only || self.bar_wants()
    }

    /// Whether the player bar would look different: a beat, a playhead
    /// that moved a device pixel, or the drift at `DRIFT_FPS`.
    fn bar_wants(&self) -> bool {
        let (Some(at), kick, bass) = self.shown else {
            return true;
        };
        let since = at.elapsed().as_secs_f32();
        if since >= 1.0 / DRIFT_FPS || since * self.head_speed >= 1.0 {
            return true;
        }
        if since < 1.0 / BEAT_FPS {
            return false;
        }
        // The kick moves the halos and the playhead directly; the bass only
        // feeds the glow's slow breath (at 0.6 of its weight), so it takes
        // twice the step.
        let bands = self.tap.as_ref().map(AudioTap::bands).unwrap_or_default();
        (bands.kick - kick).abs() > BEAT_STEP || (bands.bass - bass).abs() > 2. * BEAT_STEP
    }

    /// The song's position moved: while the ticker runs, its next frame
    /// shows it; otherwise (reduced motion) this draws one when the bar
    /// shows something new (`bar`: its elapsed time) or the playhead moved
    /// a device pixel since the last one, rather than on every tick.
    pub fn wake(&mut self, bar: bool, cx: &mut Context<Self>) {
        if self.ticker.is_some() {
            return;
        }
        let moved = self
            .woke
            .is_none_or(|at| at.elapsed().as_secs_f32() * self.head_speed >= 1.0);
        if bar || moved {
            self.woke = Some(Instant::now());
            cx.notify();
        }
    }

    /// Checks every `KEEP` whether the backdrop or the GPU can go.
    fn keep_reaping(&mut self, cx: &mut Context<Self>) {
        if self.reaper.is_some() || !self.device.held() {
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
        let backdrop = self.backdrop_shows();
        if !backdrop && self.backdrop.has_renderer() && self.shown_at.elapsed() >= KEEP {
            self.backdrop.release(cx);
        }
        if self.drew_at.elapsed() >= KEEP && !backdrop {
            log::info!("visuals: idle, releasing the GPU");
            self.bar.release();
            self.vis.release(cx);
            for change in self.changes.borrow_mut().iter_mut() {
                change.release(cx);
            }
            self.device.release();
        }
        self.device.held()
    }

    /// Stage (with its backdrop on) or the full-window visualiser shows the
    /// backdrop over the whole window.
    fn scene_backdrop(&self) -> bool {
        let config = super::config::get();
        config.backdrop.on
            && match self.input.scene {
                Some(Place::Full) => true,
                Some(_) => config.stage.backdrop,
                None => false,
            }
    }

    /// The backdrop shows: in Now Playing or over a scene.
    fn backdrop_shows(&self) -> bool {
        self.input.showing || self.scene_backdrop()
    }

    /// Where the visualiser shows now, if anywhere.
    fn vis_place(&self) -> Option<Place> {
        let config = super::config::get();
        if self.input.showing {
            return config
                .visualizer
                .now_playing
                .visualizer()
                .then_some(Place::NowPlaying);
        }
        match self.input.scene? {
            Place::Stage => config.stage.visualizer.then_some(Place::Stage),
            place => Some(place),
        }
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

    /// Tells the bar whether to leave its background to this layer, and
    /// Now Playing's cover its shadow; when either changes, the cached app
    /// view renders afresh.
    fn hand_over(&mut self, fills: bool, window: &mut Window, cx: &App) {
        let bar = self.input.bar.is_some() && self.bar.image().is_some();
        let shadow = !fills
            && self.backdrop_shows()
            && self.backdrop.image().is_some()
            && super::config::get().backdrop.on;
        if slots::paints_bar(cx) != bar || slots::paints_cover_shadow(cx) != shadow {
            Slots::set_bar_painted(cx, bar);
            Slots::set_shadow_painted(cx, shadow);
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
        let backdrop_shows = self.backdrop_shows();
        let vis_place = self.vis_place();
        let music =
            input.playing && (input.showing || input.bar.is_some() || input.scene.is_some());
        self.listen(moving && music);
        self.sync_art(window, cx);
        let changing = self.changes.borrow().iter().any(Change::active);
        let fading = (backdrop_shows && self.backdrop.fading()) || self.bar.fading();
        let animate = moving && (music || fading || changing);
        self.sparkles.advance(moving && music && backdrop_shows);
        let (due, dt) = self.tick(animate);
        self.bar_only = !input.showing && input.scene.is_none() && !fading && !changing;
        // Window frames come at `fps()` while Now Playing animates; the bar
        // and the backdrop draw a new picture only in some of them.
        let bar_due = due && (self.bar_only || self.bar.fading() || self.bar_wants());
        if bar_due {
            self.shown = (Some(Instant::now()), self.bands.kick, self.bands.bass);
        }
        let backdrop_due = due
            && (self.backdrop.fading()
                || self
                    .backdrop_at
                    .is_none_or(|at| at.elapsed().as_secs_f32() >= 0.8 / backdrop_fps()));
        if backdrop_due {
            self.backdrop_at = Some(Instant::now());
        }
        if !animate {
            self.backdrop_at = None;
        }
        // A 3D scene in the visualiser fills the backdrop's place: the
        // backdrop and its sparkles rest under it, and the views draw the
        // cover's shadow.
        let fills = vis_place.is_some() && moving && input.playing && self.vis.fills();
        self.counts.add(
            due,
            bar_due && input.bar.is_some(),
            backdrop_due && backdrop_shows && !fills,
        );
        self.head_speed = head_speed(input.bar.as_ref(), window.scale_factor(), cx);
        if backdrop_shows {
            self.shown_at = Instant::now();
        }
        let need = backdrop_shows || input.bar.is_some() || changing || vis_place.is_some();
        let gpu = self.gpu(need, window, cx);

        if let Some(gpu) = &gpu {
            let tick = Tick {
                due,
                dt,
                seconds: self.clock,
                bass: self.bands.bass,
                kick: self.bands.kick,
                level: self.bands.level,
                levels: self.bands.levels,
                look: self.look,
                reduce: self.reduce,
                gpu,
                tap: self.tap.as_ref(),
            };
            let art = self.art.as_ref().map(|a| (&a.url, &a.cover));
            // Now Playing lays out after this layer: its first frame comes
            // next.
            let paced = |due| Tick { due, ..tick };
            let backdrop_on = super::config::get().backdrop.on;
            match backdrop_area(input.showing, cx)
                .filter(|_| backdrop_shows && backdrop_on && !fills)
            {
                Some((area, _)) if !(skip("backdrop") && self.backdrop.image().is_some()) => {
                    let shadow = cover_shadow(input.showing, cx);
                    let tick = paced(backdrop_due);
                    self.backdrop.update(&tick, area, art, shadow, window)
                }
                Some(_) => {}
                None if backdrop_shows && backdrop_on => window.request_animation_frame(),
                None => {}
            }
            match vis_place.filter(|_| moving && input.playing) {
                Some(place) => {
                    let palette = self.bar.palette();
                    let id = input.bar.as_ref().and_then(|b| b.video_id.as_deref());
                    self.vis.update(&tick, place, &palette, id, window, cx);
                }
                None => self.vis.hide(cx),
            }
            if let Some(bar) = input.bar.as_ref().filter(|_| visible) {
                // The bar lays out after this layer: its first frame comes next.
                if Slots::get(cx, Slot::Bar).is_none() {
                    window.request_animation_frame();
                }
                if !(skip("strip") && self.bar.image().is_some()) {
                    self.bar.update(&paced(bar_due), bar, window, cx);
                }
            }
            let covers = Slots::covers(cx);
            let accent = self.bar.accent();
            let in_flight = super::cover_in_flight(cx);
            let dissolve = super::config::get().dissolve.on && dissolve::duration().is_some();
            let ons = [
                input.bar.is_some() && !input.now_playing && moving && dissolve,
                input.showing && !in_flight && moving && dissolve,
            ];
            let wants = [covers.small, covers.large];
            for ((change, want), on) in self.changes.borrow_mut().iter_mut().zip(wants).zip(ons) {
                change.update(want, on, due, self.look, accent, Some(gpu), window, cx);
            }
            if due || self.backdrop.pending() || self.bar.pending() {
                self.drew_at = Instant::now();
            }
        }
        self.hand_over(fills, window, cx);
        // Still pictures to finish, for the effects that are on.
        let pending = gpu.is_some()
            && ((backdrop_shows && self.backdrop.pending())
                || (input.bar.is_some() && visible && self.bar.pending()));
        if pending && self.ticker.is_none() {
            window.request_animation_frame();
        }
        self.pace(animate, window, cx);
        self.keep_reaping(cx);

        let c = theme::colors(cx);
        let config = super::config::get();
        let backdrop_on = config.backdrop.on;
        let paint = Paint {
            backdrop: (backdrop_shows && backdrop_on && !fills)
                .then(|| self.backdrop.image())
                .flatten(),
            sparkles: (backdrop_shows && backdrop_on && !fills && !skip("sparkles"))
                .then(|| {
                    Field::new(
                        &self.sparkles,
                        self.backdrop.wave_clock(),
                        if self.reduce { 0. } else { self.bands.level },
                        self.look,
                        self.bar.palette(),
                        c.signal,
                    )
                })
                .flatten(),
            scene: self.scene_backdrop(),
            fills,
            vis: vis_place.and_then(|_| self.vis.image()),
            fallback: if backdrop_on {
                self.backdrop.fallback(&c)
            } else {
                c.surface.into()
            },
            showing: input.showing,
            bar: input.bar.is_some().then(|| self.bar.image()).flatten(),
            spectrum: moving
                && input.playing
                && input.showing
                && config.visualizer.now_playing.spectrum()
                && !skip("spectrum"),
            levels: self.bands.levels,
            waveform: input
                .showing
                .then(|| Waveform::new(input.bar.as_ref(), &c, cx)),
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
    /// The sparkles over the backdrop.
    sparkles: Option<Field>,
    fallback: Background,
    showing: bool,
    /// The backdrop fills a scene (Stage, the full-window visualiser).
    scene: bool,
    /// A 3D scene fills the backdrop's place (painted as the visualiser).
    fills: bool,
    /// The visualiser's frame, where it goes and its corners.
    vis: Option<(std::sync::Arc<RenderImage>, Bounds<Pixels>, Corners<Pixels>)>,
    bar: Option<std::sync::Arc<RenderImage>>,
    /// Whether to draw the spectrum.
    spectrum: bool,
    levels: [f32; BANDS],
    /// Now Playing's waveform: its outline (once decoded) and progress.
    waveform: Option<Waveform>,
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
        let area =
            backdrop_area(self.showing, cx).filter(|_| (self.showing || self.scene) && !self.fills);
        if let Some((panel, corners)) = area {
            // The gradient shows until the first frame and instead of it
            // when there is no GPU device. Not under the frame: each layer
            // over the whole panel costs GPU time in every window frame.
            match self.backdrop {
                Some(image) => {
                    let fitted = dissolve::cover_fit(panel, &image);
                    let _ = window.paint_image(panel, fitted, corners, image, 0, false);
                }
                None => window.paint_quad(fill(panel, self.fallback).corner_radii(corners)),
            }
            if let Some(sparkles) = &self.sparkles {
                sparkles.paint(panel, window);
            }
        }
        if let Some((image, bounds, corners)) = self.vis {
            let _ = window.paint_image(bounds, bounds, corners, image, 0, false);
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
struct Waveform {
    values: Option<Vec<f32>>,
    progress: f32,
    colors: (Hsla, Hsla),
}

impl Waveform {
    fn new(bar: Option<&bar::Input>, c: &theme::Colors, cx: &App) -> Self {
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
fn backdrop_area(now_playing: bool, cx: &App) -> Option<(Bounds<Pixels>, Corners<Pixels>)> {
    if now_playing {
        slots::panel(cx).map(|p| (p, Corners::all(radius::LG)))
    } else {
        Slots::get(cx, Slot::Stage).map(|s| (s, Corners::default()))
    }
}

/// The large cover (Now Playing's, or the scene's), whose shadow the
/// backdrop draws, unless the cover is flying (the flight draws its own).
fn cover_shadow(now_playing: bool, cx: &App) -> Option<backdrop::Shadow> {
    if now_playing && super::cover_in_flight(cx) {
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
        super::stage_cover_radius(cover.size.width)
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

/// The backdrop's frames a second: half the window's, within
/// `BACKDROP_MIN_FPS..=BACKDROP_MAX_FPS` (the display's rate counts as 60).
fn backdrop_fps() -> f32 {
    (fps().unwrap_or(60) as f32 / 2.).clamp(BACKDROP_MIN_FPS, BACKDROP_MAX_FPS)
}

/// Window frames per second at most while effects move: Settings →
/// Visuals' frame rate or `YTFAST_GPUI_VISUALS_FPS`, by default
/// [`super::config::default_fps`] (each frame redraws the window: 6-10 ms of
/// GPU time on the UHD 630); `None` for the display's own rate.
fn fps() -> Option<u32> {
    let fps = super::config::get().fps;
    (fps != super::config::DISPLAY_FPS).then_some(fps.max(1))
}

/// Paced frames over five seconds, logged while animating.
#[derive(Default)]
struct Counts {
    since: Option<Instant>,
    renders: u32,
    window: u32,
    bar: u32,
    backdrop: u32,
}

impl Counts {
    fn add(&mut self, window: bool, bar: bool, backdrop: bool) {
        self.renders += 1;
        if !window {
            return;
        }
        let since = *self.since.get_or_insert_with(Instant::now);
        self.window += 1;
        self.bar += u32::from(bar);
        self.backdrop += u32::from(backdrop);
        let secs = since.elapsed().as_secs_f32();
        if secs < 5.0 {
            return;
        }
        log::info!(
            "visuals: {:.1} frames a second ({:.1} paced): bar {:.1}, backdrop {:.1}",
            self.renders as f32 / secs,
            self.window as f32 / secs,
            self.bar as f32 / secs,
            self.backdrop as f32 / secs,
        );
        *self = Self::default();
    }
}

/// Whether `YTFAST_GPUI_VISUALS_SKIP` (a comma list of `backdrop`,
/// `strip`, `spectrum`, `particles`, `upload`) leaves `what` out, to
/// measure what it costs: the backdrop and the strip keep their first
/// picture, `upload` keeps showing the first frame of each.
pub(super) fn skip(what: &str) -> bool {
    static SKIP: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    SKIP.get_or_init(|| std::env::var("YTFAST_GPUI_VISUALS_SKIP").unwrap_or_default())
        .split(',')
        .any(|s| s == what)
}
