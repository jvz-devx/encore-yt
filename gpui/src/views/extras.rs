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

use gpui_kit::*;

use crate::app::MusicApp;

pub use controls::{controls, mini_button};
pub use ridge::ridge;

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
