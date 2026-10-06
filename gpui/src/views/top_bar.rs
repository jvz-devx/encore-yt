//! The bar above the page: search (M1) and the account chip (M3).

use gpui_kit::component::h_flex;
use gpui_kit::component::input::Input;
use gpui_kit::*;

use crate::app::MusicApp;

pub fn top_bar(
    app: &MusicApp,
    window: &mut Window,
    cx: &mut Context<MusicApp>,
) -> impl IntoElement {
    h_flex()
        .px_4()
        .py_3()
        .gap_3()
        .items_center()
        .child(
            div()
                .flex_1()
                .max_w(px(560.))
                .child(Input::new(&app.pages.search).cleanable(true)),
        )
        .child(div().flex_1())
        .child(super::account::chip(app, window, cx))
}
