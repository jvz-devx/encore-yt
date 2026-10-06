//! M2: Now Playing: the large cover, Up next, Lyrics and Related.

use gpui_kit::*;

use crate::app::MusicApp;

pub fn now_playing(
    _app: &MusicApp,
    _window: &mut Window,
    _cx: &mut Context<MusicApp>,
) -> AnyElement {
    div().flex_1().into_any_element()
}
