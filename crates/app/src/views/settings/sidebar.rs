//! Settings' sidebar: its name, the search field, and the categories as
//! the window's sidebar draws its items (icon and label, the open one on
//! the `selected` fill). While a search is typed each category shows how
//! many settings in it match, and those without any fade.

use gpui_kit::assets::IconName;
use gpui_kit::component::Icon;
use gpui_kit::component::input::Input;
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::super::overlays::keycap::{combo, keycap};
use super::super::widgets;
use crate::app::MusicApp;
use crate::settings::{self as nav, Category};
use crate::theme::{Colors, Type, radius, size, space};

pub fn sidebar(app: &MusicApp, c: &Colors, cx: &mut Context<MusicApp>) -> AnyElement {
    let hits = (!app.settings.query.is_empty()).then(|| nav::search(&app.settings.query));
    v_flex()
        .flex_none()
        .w(size::SETTINGS_NAV)
        .h_full()
        .px(space::MD)
        .child(
            h_flex()
                .flex_none()
                .h(size::TOP_BAR)
                .px(space::SM)
                .gap(space::SM)
                .child(div().flex_1().type_heading().child("Settings"))
                .child(combo(&["Ctrl", ","], c)),
        )
        .child(search_field(app, c))
        .child(
            v_flex()
                .id("settings-categories")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .pt(space::LG)
                .pb(space::MD)
                .gap(space::XXS)
                .children(Category::ALL.iter().map(|category| {
                    let count = hits
                        .as_ref()
                        .map(|h| h.iter().filter(|e| e.category == *category).count());
                    item(app, *category, count, c, cx)
                })),
        )
        .into_any_element()
}

/// The search field: a pill on `raised` with a search glyph, and `/` at
/// its end while it is empty.
fn search_field(app: &MusicApp, c: &Colors) -> AnyElement {
    let Some(input) = &app.settings.search else {
        return div().into_any_element();
    };
    let empty = app.settings.query.is_empty();
    div()
        .debug_selector(|| "settings-search".into())
        .child(
            Input::new(input)
                .cleanable(true)
                .prefix(widgets::icon(IconName::Search, size::ICON_SM, c.text_muted))
                .when(empty, |i| i.suffix(keycap("/", c)))
                .h(size::CHIP)
                .pl(space::MD)
                .pr(space::SM)
                .rounded(radius::FULL)
                .bg(c.raised)
                .border_color(c.raised)
                .type_body(),
        )
        .into_any_element()
}

/// A category: 40 tall, its icon and label; the open one on `selected`.
/// Tab reaches it and Enter opens it; while searching, its match count.
fn item(
    app: &MusicApp,
    category: Category,
    count: Option<usize>,
    c: &Colors,
    cx: &mut Context<MusicApp>,
) -> AnyElement {
    let active = count.is_none() && app.settings.category == category;
    let faded = count == Some(0);
    let fg = if active { c.text } else { c.text_muted };
    let (hover, pressed, text) = (c.hover, c.pressed, c.text);
    let name: SharedString = format!("settings-category:{}", category.label()).into();
    let focus = app
        .settings
        .item(category)
        .clone()
        .tab_index(0)
        .tab_stop(true);
    let trailing = match count {
        Some(n) if n > 0 => Some(
            div()
                .type_caption()
                .tabular()
                .text_color(c.text_faint)
                .child(n.to_string())
                .into_any_element(),
        ),
        Some(_) => None,
        None if category == Category::Shortcuts => Some(keycap("?", c).into_any_element()),
        None => None,
    };
    let row = h_flex()
        .id(name.clone())
        .debug_selector(move || name.to_string())
        .track_focus(&focus)
        .flex_none()
        .h(size::NAV_ITEM)
        .px(space::MD)
        .gap(space::MD)
        .rounded(radius::MD)
        .type_label()
        .text_color(fg)
        .cursor_pointer()
        .when(faded, |s| s.opacity(0.45))
        .when(active, |s| s.bg(c.selected))
        .when(!active, |s| s.hover(move |s| s.bg(hover).text_color(text)))
        .active(move |s| s.bg(pressed))
        .child(Icon::new(category.icon()).size(size::ICON))
        .child(div().flex_1().min_w_0().truncate().child(category.label()))
        .children(trailing)
        .on_click(cx.listener(move |this, _, window, cx| this.show_category(category, window, cx)));
    super::focusable(row, c).into_any_element()
}
