//! M4: the desktop around the window: MPRIS, tray, single instance and the
//! command line, notifications, and keyboard shortcuts.

use gpui_kit::*;
use ytfast::backend::Backend;
use ytfast::paths::Paths;

use crate::app::MusicApp;

pub struct Desktop {}

impl Desktop {
    pub fn new(
        _backend: &Backend,
        _paths: &Paths,
        _window: &mut Window,
        _cx: &mut Context<MusicApp>,
    ) -> (Self, Vec<Subscription>) {
        (Self {}, Vec::new())
    }
}
