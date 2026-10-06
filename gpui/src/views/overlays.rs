//! M4: layers over the window: Play anything (Ctrl+K), the shortcuts sheet,
//! context menus.

use gpui_kit::*;

use crate::app::MusicApp;

pub fn overlays(
    _app: &MusicApp,
    _window: &mut Window,
    _cx: &mut Context<MusicApp>,
) -> Option<AnyElement> {
    None
}
