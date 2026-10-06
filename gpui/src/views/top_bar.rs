//! The bar above the page: Back/Forward, search (M1) and the account chip
//! (M3).

use gpui_kit::assets::IconName;
use gpui_kit::component::h_flex;
use gpui_kit::component::input::Input;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::widgets;
use crate::app::MusicApp;
use crate::theme::{self, Colors, Type, radius, size, space};

pub fn top_bar(
    app: &MusicApp,
    window: &mut Window,
    cx: &mut Context<MusicApp>,
) -> impl IntoElement {
    let c = theme::colors(cx);
    let (back, forward) = (!app.pages.history.is_empty(), !app.pages.forward.is_empty());
    h_flex()
        .h(size::TOP_BAR)
        .flex_none()
        .pl(space::XL)
        .pr(space::XL)
        .gap(space::XS)
        .child(
            nav_button("back", IconName::ChevronLeft, back, &c).when(back, |b| {
                b.on_click(cx.listener(|this, _, _, cx| this.back(cx)))
            }),
        )
        .child(
            nav_button("forward", IconName::ChevronRight, forward, &c).when(forward, |b| {
                b.on_click(cx.listener(|this, _, _, cx| this.go_forward(cx)))
            }),
        )
        .child(
            div().flex_1().max_w(px(480.)).ml(space::MD).child(
                Input::new(&app.pages.search)
                    .cleanable(true)
                    .prefix(widgets::icon(IconName::Search, size::ICON_SM, c.text_muted))
                    .h(px(40.))
                    .px(space::LG)
                    .rounded(radius::FULL)
                    .bg(c.raised)
                    .border_color(c.raised)
                    .type_body(),
            ),
        )
        .child(div().flex_1())
        .child(super::account::chip(app, window, cx))
}

/// Back or Forward: dimmed, and inert, while there is nowhere to go.
fn nav_button(id: &'static str, icon: IconName, enabled: bool, c: &Colors) -> Stateful<Div> {
    if enabled {
        return widgets::icon_button(id, widgets::icon(icon, size::ICON, c.text), c);
    }
    h_flex()
        .id(id)
        .flex_none()
        .size(size::ICON_BUTTON)
        .justify_center()
        .child(widgets::icon(icon, size::ICON, c.text_faint.opacity(0.6)))
}
