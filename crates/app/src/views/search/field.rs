//! The search field (DESIGN.md "Search field") and its keys: the arrows
//! move through the list under it, Enter runs the highlighted entry (or the
//! text), Escape closes the list.

use gpui_kit::assets::IconName;
use gpui_kit::component::input::{Escape, Input, MoveDown, MoveUp};
use gpui_kit::*;

use super::dropdown;
use crate::app::MusicApp;
use crate::theme::{self, Type, radius, size, space};
use crate::views::widgets;

/// The field's height; the list opens this far below its top.
const FIELD: Pixels = px(40.);

pub fn search_box(
    app: &mut MusicApp,
    window: &mut Window,
    cx: &mut Context<MusicApp>,
) -> impl IntoElement {
    let c = theme::colors(cx);
    let input = app.search_input(window, cx);
    let list = app.pages.search.dropdown().map(|kind| {
        div()
            .absolute()
            .top(FIELD + space::XS)
            .left_0()
            .right_0()
            // Drawn above the page, which comes later in the tree.
            .child(deferred(dropdown::dropdown(app, kind, cx)).with_priority(1))
    });
    div()
        .relative()
        .flex_1()
        .max_w(px(480.))
        .ml(space::MD)
        // Before the field moves its caret: the arrows walk the list while
        // it is open.
        .capture_action(cx.listener(|this, _: &MoveUp, _, cx| {
            if this.pages.search.dropdown().is_some() {
                this.move_search_highlight(-1, cx);
                cx.stop_propagation();
            }
        }))
        .capture_action(cx.listener(|this, _: &MoveDown, _, cx| {
            if this.pages.search.dropdown().is_some() {
                this.move_search_highlight(1, cx);
                cx.stop_propagation();
            }
        }))
        .capture_action(cx.listener(|this, _: &Escape, _, cx| {
            if this.close_search_list(cx) {
                cx.stop_propagation();
            }
        }))
        .child(
            Input::new(&input)
                .cleanable(true)
                .prefix(widgets::icon(IconName::Search, size::ICON_SM, c.text_muted))
                .h(FIELD)
                .px(space::LG)
                .rounded(radius::FULL)
                .bg(c.raised)
                .border_color(c.raised)
                .type_body(),
        )
        .children(list)
}
