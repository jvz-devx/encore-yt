//! The app's views as their own entity, so the shell can cache them while
//! the effects animate. It renders `views::root` on `MusicApp`'s behalf and
//! renders again whenever `MusicApp` is notified.

use gpui_kit::*;

use crate::app::MusicApp;

pub struct Content {
    app: WeakEntity<MusicApp>,
    _observe: Subscription,
}

impl Content {
    pub fn new(app: Entity<MusicApp>, cx: &mut Context<Self>) -> Self {
        let observe = cx.observe(&app, |_, _, cx| cx.notify());
        Self {
            app: app.downgrade(),
            _observe: observe,
        }
    }
}

impl Render for Content {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        match self.app.upgrade() {
            Some(app) => app.update(cx, |app, cx| crate::views::root(app, window, cx)),
            None => Empty.into_any_element(),
        }
    }
}
