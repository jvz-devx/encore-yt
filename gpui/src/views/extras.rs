//! M6's views: Stage, the equalizer panel, the sleep timer, the
//! most-replayed ridge, audition marks on songs, and the mini player.

pub mod audition;
mod controls;
mod eq_graph;
mod equalizer;
pub mod mini;
mod ridge;
mod sleep;
mod stage;
mod stage_lyrics;
mod visualizer;

use gpui_kit::*;

use crate::app::MusicApp;

pub use controls::{controls, mini_button, panel, time_left};
pub use ridge::ridge;
pub use sleep::CHOICES as SLEEP_CHOICES;

/// The full-window visualiser (V), `None` while it's closed.
pub fn visualizer(
    app: &mut MusicApp,
    window: &mut Window,
    cx: &mut Context<MusicApp>,
) -> Option<AnyElement> {
    app.extras
        .visualizer
        .then(|| visualizer::visualizer(app, window, cx))
}

/// Stage (F): the cover and large lyrics fill the window. `None` while it's
/// closed. Called first on every frame of the main window, so it also runs
/// this area's per-frame work.
pub fn stage(
    app: &mut MusicApp,
    window: &mut Window,
    cx: &mut Context<MusicApp>,
) -> Option<AnyElement> {
    app.extras_frame(window, cx);
    controls::keep_counting(app, cx);
    if !app.extras.stage.open {
        return None;
    }
    Some(stage::stage(app, window, cx))
}
