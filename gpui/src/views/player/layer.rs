//! The player bar as its own view, beside the app's views rather than in
//! them: playback reports and the clock move the position several times a
//! second, and GPUI re-renders a notified view's ancestors too, so inside
//! the app's views each tick would re-render the whole window.
//!
//! It renders [`super::player_bar`] on `MusicApp`'s behalf, again whenever
//! `MusicApp` is notified, and on its own when only the position moved
//! ([`crate::playback::Clock`]). The shell (`visuals::shell`) lays it out
//! under the app's views, in the room [`super::space`] leaves, so dialogs
//! and menus still cover it.

use gpui_kit::*;

use crate::app::MusicApp;
use crate::playback::Clock;
use crate::theme::{self, Type};

pub struct PlayerBar {
    app: WeakEntity<MusicApp>,
    _observe: [Subscription; 2],
}

impl PlayerBar {
    /// `clock` is the app's `player.clock` (the app is being updated while
    /// this is made, so it can't be read here).
    pub fn new(app: Entity<MusicApp>, clock: Entity<Clock>, cx: &mut Context<Self>) -> Self {
        let observe = [
            cx.observe(&app, |_, _, cx| cx.notify()),
            cx.observe(&clock, |_, _, cx| cx.notify()),
        ];
        Self {
            app: app.downgrade(),
            _observe: observe,
        }
    }
}

impl Render for PlayerBar {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(app) = self.app.upgrade() else {
            return Empty.into_any_element();
        };
        // What the app's root sets for the views under it. The cover loads
        // through the window's asset cache, as the effects layer loads it
        // too, so the app's cover cache can't drop it under a reused frame.
        let c = theme::colors(cx);
        div()
            .size_full()
            .text_color(c.text)
            .type_body()
            .child(app.update(cx, |app, cx| super::player_bar(app, cx).into_any_element()))
            .into_any_element()
    }
}
