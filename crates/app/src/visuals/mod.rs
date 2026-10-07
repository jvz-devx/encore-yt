//! The effects (PLAN M8, M9), drawn around the app instead of inside it, so
//! an animation frame doesn't re-render the app.
//!
//! [`shell`] is what `MusicApp` renders. It stacks five layers:
//!
//! 1. [`effects::Effects`]: the window's base colour, the animated cover
//!    backdrop over the page panel and the spectrum while Now Playing
//!    shows ([`backdrop`]), and the player bar's background, seek bar and
//!    beat halos ([`bar`]), all on one GPU device of our own
//!    (`encore-visuals`). It re-renders on its own timer.
//! 2. The player bar (`views::PlayerBar`), a cached view in the room the
//!    app's views leave at the bottom. It renders again when `MusicApp` is
//!    notified or only the position moved (`playback::Clock`).
//! 3. [`content::Content`]: the app itself (`views::root`), a cached view.
//!    The page panel (behind Now Playing), the player bar and its slider
//!    are see-through where the effects paint.
//! 4. The cover dissolves on a track change ([`dissolve`]), over the app.
//! 5. [`flight::Flight`]: the cover flying between the player bar and Now
//!    Playing.
//!
//! GPUI marks a notified view and all its ancestors dirty, and a window's
//! root re-renders on every frame. `MusicApp` is that root's child, so an
//! effects frame re-renders `MusicApp`; with the app's views and the bar in
//! cached views beside the effects, that is only this small shell, and a
//! position tick re-renders only the bar. Each frame still redraws the
//! whole window (about 2 ms of CPU and 8-12 ms of GPU time on an Intel UHD
//! 630), so while only the player bar moves, frames come when it would look
//! different, and Now Playing draws at 20 frames a second.
//!
//! The views place the effects with [`slot`] (empty boxes whose bounds the
//! layers read); Now Playing draws the song's waveform with [`waveform`].
//! Settings: `ENCORE_VISUALS=0` turns the effects off,
//! `ENCORE_VISUALS_FPS` sets the highest frame rate (default 20),
//! `ENCORE_REDUCED_MOTION=1` (or the desktop's reduced motion) freezes
//! them, `ENCORE_VISUALS_UNCACHED=1` turns the cached view off (to
//! measure it), `ENCORE_VISUALS_SKIP=backdrop,strip,spectrum,particles,upload`
//! leaves single effects out (to measure them) and
//! `ENCORE_VISUALS_FLIGHT_MS` slows the flying cover down (to look at
//! it) and `ENCORE_FRAME_LOG=<ms>` logs frames slower than that
//! ([`timing`]).

mod ambient;
mod backdrop;
mod bar;
pub mod config;
mod content;
mod device;
mod dissolve;
mod effects;
mod flight;
mod frames;
mod slots;
mod timing;
mod visualizer;
mod waveform;

use std::cell::Cell;
use std::rc::Rc;

use gpui_kit::*;

use crate::app::MusicApp;
use crate::theme;

pub use slots::{Covers, Slot, paints_bar, paints_cover_shadow, set_bar_cover, set_covers, slot};
pub use waveform::waveform;

/// The layers, made with the first window and kept for the next one.
struct Layers {
    handles: Handles,
    /// The window whose root background is cleared ([`clear_root_background`]).
    cleared: Cell<Option<AnyWindowHandle>>,
    _keys: Subscription,
}

#[derive(Clone)]
struct Handles {
    content: Entity<content::Content>,
    bar: Entity<crate::views::PlayerBar>,
    effects: Entity<effects::Effects>,
    flight: Entity<flight::Flight>,
    /// Set by input the cached app view may not hear about (a slider drag
    /// moves a model, not a view): the next frame renders the app afresh.
    input: Rc<Cell<bool>>,
}

impl Global for Layers {}

/// The window's content: effects under the app, the flying cover over it.
pub fn shell(app: &mut MusicApp, window: &mut Window, cx: &mut Context<MusicApp>) -> AnyElement {
    timing::frame_started();
    let layers = layers(app, cx);
    clear_root_background(window, cx);
    let showing = fills_panel(app);
    let playback = &app.player.playback;
    let playing = playback.playing && !playback.loading;
    let bar = bar_input(app, cx);
    let scene = if !enabled() {
        None
    } else if app.extras.stage.open {
        Some(visualizer::Place::Stage)
    } else if app.extras.visualizer {
        Some(visualizer::Place::Full)
    } else {
        None
    };
    let input = effects::Input {
        showing: showing && enabled(),
        playing,
        now_playing: app.player.now_playing,
        bar,
        scene,
    };
    layers
        .effects
        .update(cx, |effects, _| effects.input = input);
    // Stage and the visualiser cover Now Playing without closing it: no
    // flight when they open or close over it.
    let open =
        app.player.now_playing && app.player.now_playing_over.as_ref() == Some(&app.pages.view);
    layers
        .flight
        .update(cx, |flight, cx| flight.follow(open, window, cx));
    // Cached, except after input the app's models may have taken: a cached
    // view renders again only when notified.
    let cache = !layers.input.take() && !uncached();
    let overlay = layers.effects.read(cx).overlay();
    let content = view(layers.content.clone().into(), cache);
    // Under the app's views, in the room they leave for it, so their
    // dialogs and menus cover it. Stage and the visualiser hide it.
    // Redrawn afresh in this frame once its elapsed time or ridge would
    // show the position differently (`MusicApp::position_moved`).
    let bar_stale = app.player.bar_shown != Some(app.bar_shows(cx));
    let bar_layer = (!app.extras.stage.open && !app.extras.visualizer).then(|| {
        div()
            .absolute()
            .left_0()
            .right_0()
            .bottom_0()
            .h(theme::size::PLAYER_BAR)
            .child(view(layers.bar.clone().into(), cache && !bar_stale))
    });
    div()
        .size_full()
        .relative()
        .bg(theme::colors(cx).base)
        .child(layers.effects.clone())
        .children(bar_layer)
        .child(content)
        .child(overlay)
        .child(layers.flight.clone())
        .children(timing::frame_end())
        .into_any_element()
}

/// The kit's root paints the theme's background over the whole window,
/// and the shell paints the window's base colour over all of it, so the
/// root's is never seen. It is cleared: on an Intel UHD 630 every layer
/// over the whole window costs about a millisecond of GPU time per frame.
fn clear_root_background(window: &mut Window, cx: &mut App) {
    let handle = window.window_handle();
    let Some(layers) = cx.try_global::<Layers>() else {
        return;
    };
    if layers.cleared.get() == Some(handle) {
        return;
    }
    layers.cleared.set(Some(handle));
    if let Some(Some(root)) = window.root::<gpui_kit::base::Root>() {
        root.update(cx, |root, _| {
            root.style().background = Some(transparent_black().into());
        });
    }
}

/// A layer's view, cached or rendered afresh.
fn view(view: AnyView, cache: bool) -> AnyElement {
    if cache {
        view.cached(StyleRefinement::default().size_full())
            .into_any_element()
    } else {
        view.into_any_element()
    }
}

/// The player bar's state for the effects layer, or `None` while the bar
/// isn't on screen (Stage) or effects are off. Also asks for the song's
/// waveform. (The bar tells the layer its cover, [`set_bar_cover`].)
fn bar_input(app: &MusicApp, cx: &mut Context<MusicApp>) -> Option<bar::Input> {
    if !enabled() {
        slots::set_bar_cover(None, cx);
        return None;
    }
    // Stage and the full-window visualiser hand over the song's cover
    // themselves, for the backdrop's colours.
    if app.extras.stage.open || app.extras.visualizer {
        return None;
    }
    let player = &app.player;
    let track = player.current();
    waveform::ensure(app, cx);
    let duration = player.playback.duration;
    let progress = if player.seeking {
        player.seek.read(cx).value().start() / crate::playback::SEEK_SCALE
    } else if duration > 0.0 {
        (player.position() / duration) as f32
    } else {
        0.0
    };
    Some(bar::Input {
        track: track.is_some(),
        progress: progress.clamp(0.0, 1.0),
        known: duration > 0.0,
        duration,
        video_id: track.map(|t| t.video_id.clone()),
        heat: app.current_heat().cloned(),
    })
}

/// The position moved: whether the effects layer shows it without the
/// player bar being notified (it draws the seek bar, and the shell redraws
/// the bar in its frames when needed). While its ticker runs the next frame
/// shows it; otherwise (reduced motion) it is woken for a frame when `bar`
/// (the bar shows something new) or the playhead moved a pixel.
pub fn position_moved(bar: bool, cx: &mut App) -> bool {
    if !paints_bar(cx) {
        return false;
    }
    let Some(effects) = cx.try_global::<Layers>().map(|l| l.handles.effects.clone()) else {
        return false;
    };
    effects.update(cx, |effects, cx| effects.wake(bar, cx));
    true
}

/// Whether the effects paint behind Stage (its backdrop or visualiser):
/// Stage leaves its background see-through then.
pub fn paints_stage() -> bool {
    let config = config::get();
    enabled() && ((config.stage.backdrop && config.backdrop.on) || config.stage.visualizer)
}

/// The band Stage keeps free along the bottom of its body for the
/// visualiser's bars or line (in a window `height` points tall), so the
/// cover, titles and lyrics sit above it; nothing for the other styles or
/// without the visualiser.
pub fn stage_band(height: f32) -> Pixels {
    let config = config::get();
    let banded = matches!(
        config.visualizer.style,
        config::Style::Bars | config::Style::Mirrored | config::Style::Line | config::Style::Scope
    );
    if enabled() && config.stage.visualizer && banded {
        px((height * 0.16).clamp(72., 200.))
    } else {
        px(0.)
    }
}

/// Whether the effects paint behind the full-window visualiser.
pub fn paints_full() -> bool {
    enabled()
}

/// How far the ring's bars reach out of a scene's cover of `side`.
pub fn ring_reach(side: Pixels) -> Pixels {
    (side * 0.2).min(px(120.))
}

/// How far the ring's bars reach out of Now Playing's cover of `side`.
pub fn now_playing_ring_reach(side: Pixels) -> Pixels {
    (side * 0.1).min(px(36.))
}

/// Room to keep round Now Playing's cover of about `side` for the ring
/// (its gap off the cover, its bars and their caps), or nothing when the
/// ring isn't the visualiser there. The cover shrinks by it, so the ring
/// stays inside the panel.
pub fn now_playing_ring_room(side: Pixels) -> Pixels {
    let config = config::get();
    let v = &config.visualizer;
    if !(enabled() && v.now_playing.visualizer() && v.style == config::Style::Ring) {
        return px(0.);
    }
    // The shader's gap: 3 px and 3 % of the cover.
    px(3.) + side * 0.03 + now_playing_ring_reach(side) + px(8.)
}

/// How far the ring's bars reach out of Stage's cover of `side` (Stage
/// shares its room with the lyrics, so less than the full window's).
pub fn stage_ring_reach(side: Pixels) -> Pixels {
    (side * 0.12).min(px(64.))
}

/// Room to keep round Stage's cover of about `side` for the ring, or
/// nothing when the ring isn't Stage's visualiser. The cover shrinks by
/// it, so the ring clears the titles, the lyrics and the window's edge.
pub fn stage_ring_room(side: Pixels) -> Pixels {
    let config = config::get();
    if !(enabled() && config.stage.visualizer && config.visualizer.style == config::Style::Ring) {
        return px(0.);
    }
    px(3.) + side * 0.03 + stage_ring_reach(side) + px(8.)
}

/// Stage's cover's corner radius at `side` (also the full-window
/// visualiser's).
pub fn stage_cover_radius(side: Pixels) -> Pixels {
    (side * 0.02).clamp(theme::radius::MD, px(16.))
}

/// The height of Now Playing's strip above the title (`base` for the
/// thin spectrum): taller while the visualiser draws its bars there.
pub fn spectrum_height(base: Pixels) -> Pixels {
    if strip_band() { base + px(24.) } else { base }
}

/// Where the visualiser draws in Now Playing's strip: all of it, with room
/// round it for the glow.
fn visualizer_strip(strip: Bounds<Pixels>) -> Bounds<Pixels> {
    Bounds::from_corners(
        point(strip.left() - px(8.), strip.top() - px(10.)),
        point(strip.right() + px(8.), strip.bottom() + px(4.)),
    )
}

/// Whether the visualiser draws a band in Now Playing's strip.
fn strip_band() -> bool {
    let config = config::get();
    let v = &config.visualizer;
    let banded = !matches!(v.style, config::Style::Ring | config::Style::Particles)
        && v.style.scene().is_none();
    enabled() && v.now_playing.visualizer() && banded
}

/// The seek bar's width, as last laid out.
pub fn seek_width(cx: &App) -> Option<Pixels> {
    slots::Slots::get(cx, Slot::Seek).map(|b| b.size.width)
}

/// Now Playing fills the page panel: the panel is left see-through and the
/// effects layer paints it.
pub fn fills_panel(app: &MusicApp) -> bool {
    app.player.now_playing
        && app.player.now_playing_over.as_ref() == Some(&app.pages.view)
        && !app.extras.stage.open
        && !app.extras.visualizer
}

/// Whether this layer paints Now Playing's waveform (so a position tick
/// needn't re-render the app's views).
pub fn paints_waveform(app: &MusicApp) -> bool {
    fills_panel(app) && enabled()
}

/// Whether the cover in Now Playing is hidden because its copy is flying
/// into place.
pub fn cover_in_flight(cx: &App) -> bool {
    cx.try_global::<Layers>()
        .is_some_and(|layers| layers.handles.flight.read(cx).landing())
}

/// `ENCORE_VISUALS_UNCACHED=1` renders the app's views on every
/// effects frame, as before the cached view: for measuring what it saves.
fn uncached() -> bool {
    std::env::var_os("ENCORE_VISUALS_UNCACHED").is_some_and(|v| v == "1")
}

/// Effects are on unless Settings → Visuals has them off (the Off preset)
/// or `ENCORE_VISUALS=0`, and off in the UI tests (they need a GPU
/// device and the audio engine).
pub fn enabled() -> bool {
    !cfg!(test) && config::get().on
}

/// Motion is reduced when the desktop asks for it ([`theme::reduced_motion`])
/// or `ENCORE_REDUCED_MOTION=1` (for trying it without changing the
/// desktop): the backdrop holds still, the spectrum and particles are off
/// and the cover doesn't fly.
pub fn reduced_motion(cx: &App) -> bool {
    theme::reduced_motion(cx) || std::env::var_os("ENCORE_REDUCED_MOTION").is_some_and(|v| v == "1")
}

fn layers(app: &MusicApp, cx: &mut Context<MusicApp>) -> Handles {
    if !cx.has_global::<Layers>() {
        config::load(&app.paths.config);
        cx.set_global(slots::Slots::default());
        let input = Rc::new(Cell::new(false));
        let clock = app.player.clock.clone();
        let cache = app.paths.cache.clone();
        let app = cx.entity();
        let content = cx.new(|cx| content::Content::new(app.clone(), cx));
        let bar = cx.new(|cx| crate::views::PlayerBar::new(app, clock, cx));
        let effects = cx.new(|_| effects::Effects::new(input.clone(), &cache));
        let flight = cx.new(|_| flight::Flight::new(content.clone()));
        let keys_input = input.clone();
        let keys = cx.observe_keystrokes(move |_, _, _, _| keys_input.set(true));
        cx.set_global(Layers {
            handles: Handles {
                content,
                bar,
                effects,
                flight,
                input,
            },
            cleared: Cell::new(None),
            _keys: keys,
        });
    }
    cx.global::<Layers>().handles.clone()
}
