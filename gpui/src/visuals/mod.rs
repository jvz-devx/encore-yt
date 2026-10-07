//! The effects (PLAN M8, M9), drawn around the app instead of inside it, so
//! an animation frame doesn't re-render the app.
//!
//! [`shell`] is what `MusicApp` renders. It stacks four layers:
//!
//! 1. [`effects::Effects`]: the window's base colour, the animated cover
//!    backdrop over the page panel and the spectrum while Now Playing
//!    shows ([`backdrop`]), and the player bar's background, seek bar and
//!    beat halos ([`bar`]), all on one GPU device of our own
//!    (`ytfast-visuals`). It re-renders on its own timer.
//! 2. [`content::Content`]: the app itself (`views::root`), as a cached
//!    view while an effect animates. The page panel (behind Now Playing),
//!    the player bar and its slider are see-through where the effects
//!    paint.
//! 3. The cover dissolves on a track change ([`dissolve`]), over the app.
//! 4. [`flight::Flight`]: the cover flying between the player bar and Now
//!    Playing.
//!
//! GPUI marks a notified view and all its ancestors dirty, and a window's
//! root re-renders on every frame. `MusicApp` is that root's child, so an
//! effects frame re-renders `MusicApp`; with the app's views in a cached
//! `Content` beside the effects, that is only this small shell. Each frame
//! still redraws the whole window (about 2 ms of CPU), so while only the
//! player bar moves, frames come when it would look different.
//!
//! The views place the effects with [`slot`] (empty boxes whose bounds the
//! layers read); Now Playing draws the song's waveform with [`waveform`].
//! Settings: `YTFAST_GPUI_VISUALS=0` turns the effects off,
//! `YTFAST_GPUI_VISUALS_FPS` sets the highest frame rate (default 30),
//! `YTFAST_GPUI_REDUCED_MOTION=1` (or the desktop's reduced motion) freezes
//! them, `YTFAST_GPUI_VISUALS_UNCACHED=1` turns the cached view off (to
//! measure it) and `YTFAST_GPUI_VISUALS_FLIGHT_MS` slows the flying cover
//! down (to look at it).

mod backdrop;
mod bar;
mod content;
mod dissolve;
mod effects;
mod flight;
mod frames;
mod slots;
mod waveform;

use std::cell::Cell;
use std::rc::Rc;

use gpui_kit::*;

use crate::app::MusicApp;
use crate::theme;

pub use slots::{Covers, Slot, paints_bar, set_bar_cover, set_covers, slot};
pub use waveform::waveform;

/// The layers, made with the first window and kept for the next one.
struct Layers {
    handles: Handles,
    _keys: Subscription,
}

#[derive(Clone)]
struct Handles {
    content: Entity<content::Content>,
    effects: Entity<effects::Effects>,
    flight: Entity<flight::Flight>,
    /// Set by input the cached app view may not hear about (a slider drag
    /// moves a model, not a view): the next frame renders the app afresh.
    input: Rc<Cell<bool>>,
}

impl Global for Layers {}

/// The window's content: effects under the app, the flying cover over it.
pub fn shell(app: &mut MusicApp, window: &mut Window, cx: &mut Context<MusicApp>) -> AnyElement {
    let layers = layers(cx);
    let showing = fills_panel(app);
    let playback = &app.player.playback;
    let playing = playback.playing && !playback.loading;
    let bar = bar_input(app, cx);
    let bar_moves = bar.is_some() && playing && !reduced_motion(cx);
    let input = effects::Input {
        showing: showing && enabled(),
        playing,
        now_playing: app.player.now_playing,
        bar,
    };
    layers
        .effects
        .update(cx, |effects, _| effects.input = input);
    let flying = layers
        .flight
        .update(cx, |flight, cx| flight.follow(showing, window, cx));
    // Cached only while effects or the flight animate, and not after input
    // the app's models may have taken.
    let cache = (showing || flying || bar_moves) && !layers.input.take() && !uncached();
    let overlay = layers.effects.read(cx).overlay();
    let content = if cache {
        layers
            .content
            .clone()
            .cached(StyleRefinement::default().size_full())
            .into_any_element()
    } else {
        layers.content.clone().into_any_element()
    };
    div()
        .size_full()
        .relative()
        .bg(theme::colors(cx).base)
        .child(layers.effects.clone())
        .child(content)
        .child(overlay)
        .child(layers.flight.clone())
        .into_any_element()
}

/// The player bar's state for the effects layer, or `None` while the bar
/// isn't on screen (Stage) or effects are off. Also asks for the song's
/// waveform. (The bar tells the layer its cover, [`set_bar_cover`].)
fn bar_input(app: &MusicApp, cx: &mut Context<MusicApp>) -> Option<bar::Input> {
    if !enabled() || app.extras.stage.open {
        slots::set_bar_cover(None, cx);
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
    })
}

/// Now Playing fills the page panel: the panel is left see-through and the
/// effects layer paints it.
pub fn fills_panel(app: &MusicApp) -> bool {
    app.player.now_playing && app.player.now_playing_over.as_ref() == Some(&app.pages.view)
}

/// Whether the cover in Now Playing is hidden because its copy is flying
/// into place.
pub fn cover_in_flight(cx: &App) -> bool {
    cx.try_global::<Layers>()
        .is_some_and(|layers| layers.handles.flight.read(cx).landing())
}

/// `YTFAST_GPUI_VISUALS_UNCACHED=1` renders the app's views on every
/// effects frame, as before the cached view: for measuring what it saves.
fn uncached() -> bool {
    std::env::var_os("YTFAST_GPUI_VISUALS_UNCACHED").is_some_and(|v| v == "1")
}

/// Effects are on unless `YTFAST_GPUI_VISUALS=0`.
fn enabled() -> bool {
    std::env::var_os("YTFAST_GPUI_VISUALS").is_none_or(|v| v != "0")
}

/// Motion is reduced when the desktop asks for it ([`theme::reduced_motion`])
/// or `YTFAST_GPUI_REDUCED_MOTION=1` (for trying it without changing the
/// desktop): the backdrop holds still, the spectrum and particles are off
/// and the cover doesn't fly.
pub fn reduced_motion(cx: &App) -> bool {
    theme::reduced_motion(cx)
        || std::env::var_os("YTFAST_GPUI_REDUCED_MOTION").is_some_and(|v| v == "1")
}

fn layers(cx: &mut Context<MusicApp>) -> Handles {
    if !cx.has_global::<Layers>() {
        cx.set_global(slots::Slots::default());
        let input = Rc::new(Cell::new(false));
        let app = cx.entity();
        let content = cx.new(|cx| content::Content::new(app, cx));
        let effects = cx.new(|_| effects::Effects::new(input.clone()));
        let flight = cx.new(|_| flight::Flight::new(content.clone()));
        let keys_input = input.clone();
        let keys = cx.observe_keystrokes(move |_, _, _, _| keys_input.set(true));
        cx.set_global(Layers {
            handles: Handles {
                content,
                effects,
                flight,
                input,
            },
            _keys: keys,
        });
    }
    cx.global::<Layers>().handles.clone()
}
