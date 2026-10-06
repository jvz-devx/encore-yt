//! M2: the Up next panel beside the page.

use gpui_kit::*;

use crate::app::MusicApp;

pub fn panel(
    app: &MusicApp,
    _window: &mut Window,
    _cx: &mut Context<MusicApp>,
) -> Option<AnyElement> {
    if !app.player.queue_open {
        return None;
    }
    None
}
