//! M6: Stage, the equalizer, the sleep timer and the mini player.

use gpui_kit::*;

use crate::app::MusicApp;

/// Stage (F): the cover and large lyrics fill the window. `None` while it's
/// closed.
pub fn stage(
    _app: &MusicApp,
    _window: &mut Window,
    _cx: &mut Context<MusicApp>,
) -> Option<AnyElement> {
    None
}
