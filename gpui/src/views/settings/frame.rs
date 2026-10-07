//! Settings' right-hand panel (the category's title and line, its Reset
//! and the close button, its tabs, and its body scrolling on its own) and
//! the strip of key hints along the modal's foot.

use gpui_kit::assets::IconName;
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::super::overlays::keycap::combo;
use super::super::widgets;
use crate::app::MusicApp;
use crate::settings::Category;
use crate::theme::{Colors, Type, elevation, radius, size, space};
use crate::update;

/// The category showing, on its `surface` panel.
pub fn content(
    app: &MusicApp,
    c: &Colors,
    window: &mut Window,
    cx: &mut Context<MusicApp>,
) -> AnyElement {
    let category = app.settings.category;
    let reset = super::reset(app, category, c, cx);
    let header = header(category.label(), category.blurb(), reset, c, cx);
    let tabs = tabs(app, category, c, cx);
    let body = super::body(app, category, c, window, cx);
    panel(
        header,
        tabs,
        body,
        SharedString::from(format!("settings-body-{category:?}")),
        c,
    )
}

/// The right-hand panel: a header, maybe tabs, and a body that scrolls.
pub(super) fn panel(
    header: AnyElement,
    tabs: Option<AnyElement>,
    body: Vec<AnyElement>,
    id: SharedString,
    c: &Colors,
) -> AnyElement {
    v_flex()
        .flex_1()
        .min_w_0()
        .h_full()
        .mt(space::SM)
        .mr(space::SM)
        .rounded(radius::LG)
        .bg(c.surface)
        .overflow_hidden()
        .child(header)
        .children(tabs.map(|t| div().flex_none().px(size::GUTTER).pb(space::LG).child(t)))
        .child(
            div()
                .id(id)
                .debug_selector(|| "settings-content".into())
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .child(
                    v_flex()
                        .w_full()
                        .max_w(size::SETTINGS_TEXT + size::GUTTER * 2.)
                        .px(size::GUTTER)
                        .pt(space::XS)
                        .pb(space::XL)
                        .gap(space::XL)
                        .children(body),
                ),
        )
        .into_any_element()
}

/// The title and its line, what acts on the whole category, and Close.
pub(super) fn header(
    title: impl Into<SharedString>,
    blurb: impl Into<SharedString>,
    action: Option<AnyElement>,
    c: &Colors,
    cx: &mut Context<MusicApp>,
) -> AnyElement {
    h_flex()
        .flex_none()
        .items_start()
        .gap(space::LG)
        .pl(size::GUTTER)
        .pr(space::LG)
        .pt(space::XL)
        .pb(space::LG)
        .child(
            v_flex()
                .flex_1()
                .min_w_0()
                .gap(space::XXS)
                .child(div().type_title().truncate().child(title.into()))
                .child(widgets::muted_line(blurb, c)),
        )
        .children(action.map(|a| div().pt(space::XXS).child(a)))
        .child(
            super::focusable(
                widgets::icon_button(
                    "settings-close",
                    widgets::icon(IconName::X, size::ICON, c.text_muted),
                    c,
                ),
                c,
            )
            .tooltip(widgets::tooltip("Close (Esc)"))
            .on_click(cx.listener(|this, _, window, cx| this.open_settings(false, window, cx))),
        )
        .into_any_element()
}

/// The category's tabs as a segmented control, as Now Playing's: the
/// chosen one a lifted pill on the raised track. A few tabs hug their
/// labels; many share the width.
fn tabs(
    app: &MusicApp,
    category: Category,
    c: &Colors,
    cx: &mut Context<MusicApp>,
) -> Option<AnyElement> {
    let labels = category.tabs();
    // With every effect off, Visuals has only its General settings.
    if labels.is_empty() || (category == Category::Visuals && !super::visuals::tabs_shown()) {
        return None;
    }
    let chosen = app.settings.tab(category);
    let wide = labels.len() > 3;
    let items = labels.iter().enumerate().map(|(i, label)| {
        let active = i == chosen;
        let hover = c.text;
        let name: SharedString = format!("settings-tab:{label}").into();
        let tab = h_flex()
            .id(("settings-tab", i))
            .debug_selector(move || name.to_string())
            .h_full()
            .px(space::LG)
            .justify_center()
            .rounded(radius::FULL)
            .type_label()
            .whitespace_nowrap()
            .text_color(if active { c.text } else { c.text_muted })
            .when(wide, |t| t.flex_1().px(space::XS))
            .when(active, |s| s.bg(c.overlay).shadow(elevation::low(c)))
            .when(!active, |s| {
                s.cursor_pointer().hover(move |s| s.text_color(hover))
            })
            .on_click(cx.listener(move |this, _, _, cx| {
                this.settings.set_tab(category, i);
                cx.notify();
            }))
            .child(*label);
        super::focusable(tab, c)
    });
    Some(
        h_flex()
            .h(size::CHIP)
            .p(space::XXS)
            .rounded(radius::FULL)
            .bg(c.raised)
            .map(|t| {
                if wide {
                    t.w_full().max_w(size::SETTINGS_TEXT)
                } else {
                    t.flex_none()
                }
            })
            .children(items)
            .into_any_element(),
    )
}

/// The modal's own keys, and the version.
pub fn footer(c: &Colors) -> AnyElement {
    let hint = |keys: &'static [&'static str], what: &'static str| {
        h_flex()
            .flex_none()
            .gap(space::SM)
            .child(combo(keys, c))
            .child(div().type_caption().text_color(c.text_muted).child(what))
    };
    h_flex()
        .flex_none()
        .h(size::SETTINGS_FOOTER)
        .px(space::LG)
        .gap(space::XL)
        .overflow_hidden()
        .child(hint(&["Esc"], "Close"))
        .child(hint(&["↑", "↓"], "Categories"))
        .child(hint(&["Control", "Tab"], "Next category"))
        .child(hint(&["/"], "Search"))
        .child(hint(&["Tab"], "Next setting"))
        .child(div().flex_1())
        .child(
            div()
                .flex_none()
                .type_caption()
                .tabular()
                .text_color(c.text_faint)
                .child(format!("Music {}", update::VERSION)),
        )
        .into_any_element()
}
