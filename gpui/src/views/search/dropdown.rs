//! The list under the search field: recent searches while the field is
//! empty (each with Remove, and Clear), suggestions while typing. A floating
//! panel (DESIGN.md "Panels"): `overlay` fill, `radius LG`, high elevation.
//!
//! Entries act on mouse down: the field loses its focus on release, which
//! closes the list.

use gpui_kit::assets::IconName;
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::app::MusicApp;
use crate::pages::Dropdown;
use crate::theme::{self, Colors, Type, elevation, radius, size, space};
use crate::views::widgets;

pub fn dropdown(app: &MusicApp, kind: Dropdown, cx: &mut Context<MusicApp>) -> impl IntoElement {
    let c = theme::colors(cx);
    let search = &app.pages.search;
    let typed = search.text().to_string();
    let entries: Vec<String> = search.entries().to_vec();
    let heading = (kind == Dropdown::Recent).then(|| recent_heading(&c, cx));
    let rows: Vec<AnyElement> = entries
        .into_iter()
        .enumerate()
        .map(|(i, text)| {
            let highlighted = search.highlight == Some(i);
            match kind {
                Dropdown::Recent => recent_row(i, text, highlighted, &c, cx),
                Dropdown::Suggestions => suggestion_row(i, text, &typed, highlighted, &c, cx),
            }
        })
        .collect();
    v_flex()
        .id("search-list")
        .w_full()
        .p(space::XS)
        .rounded(radius::LG)
        .bg(c.overlay)
        .shadow(elevation::high(&c))
        .occlude()
        .children(heading)
        .children(rows)
}

fn recent_heading(c: &Colors, cx: &mut Context<MusicApp>) -> AnyElement {
    h_flex()
        .h(px(32.))
        .pl(space::MD)
        .pr(space::XS)
        .child(
            div()
                .flex_1()
                .type_caption()
                .text_color(c.text_muted)
                .child("Recent searches"),
        )
        .child(
            h_flex()
                .id("recent-clear")
                .h(px(28.))
                .px(space::SM)
                .rounded(radius::SM)
                .type_caption()
                .text_color(c.text_muted)
                .cursor_pointer()
                .hover(|s| s.bg(c.hover))
                .child("Clear")
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _, window, cx| {
                        cx.stop_propagation();
                        this.clear_searches(window, cx);
                    }),
                ),
        )
        .into_any_element()
}

/// One entry's frame: 40 tall, an icon, its text; keyboard highlight is a
/// `selected` fill.
fn entry(id: SharedString, icon: IconName, highlighted: bool, c: &Colors) -> Stateful<Div> {
    h_flex()
        .id(id)
        .group("search-entry")
        .w_full()
        .h(size::NAV_ITEM)
        .pl(space::MD)
        .pr(space::XS)
        .gap(space::MD)
        .rounded(radius::MD)
        .cursor_pointer()
        .when(highlighted, |s| s.bg(c.selected))
        .hover(|s| s.bg(c.hover))
        .child(widgets::icon(icon, size::ICON_SM, c.text_faint))
}

fn recent_row(
    i: usize,
    query: String,
    highlighted: bool,
    c: &Colors,
    cx: &mut Context<MusicApp>,
) -> AnyElement {
    let run = query.clone();
    let forget = query.clone();
    entry(
        SharedString::from(format!("recent:{i}")),
        IconName::RotateCcwClock,
        highlighted,
        c,
    )
    .child(div().flex_1().min_w_0().truncate().type_body().child(query))
    .child(
        h_flex()
            .id(SharedString::from(format!("recent-remove:{i}")))
            .flex_none()
            .size(px(32.))
            .justify_center()
            .rounded(radius::FULL)
            .opacity(0.)
            .group_hover("search-entry", |s| s.opacity(1.))
            .hover(|s| s.bg(c.hover))
            .child(widgets::icon(IconName::X, size::ICON_SM, c.text_muted))
            .tooltip(|window, cx| {
                gpui_kit::component::tooltip::Tooltip::new("Remove").build(window, cx)
            })
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _, window, cx| {
                    cx.stop_propagation();
                    this.forget_search(&forget, window, cx);
                }),
            ),
    )
    .on_mouse_down(
        MouseButton::Left,
        cx.listener(move |this, _, window, cx| {
            this.run_search(run.clone(), window, cx);
        }),
    )
    .into_any_element()
}

fn suggestion_row(
    i: usize,
    suggestion: String,
    typed: &str,
    highlighted: bool,
    c: &Colors,
    cx: &mut Context<MusicApp>,
) -> AnyElement {
    let run = suggestion.clone();
    // What YouTube adds to the typed text is the part to read.
    let typed_len = suggestion
        .char_indices()
        .nth(typed.chars().count())
        .map_or(suggestion.len(), |(at, _)| at);
    let completes = suggestion
        .get(..typed_len)
        .is_some_and(|start| start.eq_ignore_ascii_case(typed));
    let highlights = if completes && typed_len < suggestion.len() {
        vec![(
            typed_len..suggestion.len(),
            HighlightStyle {
                font_weight: Some(FontWeight::SEMIBOLD),
                color: Some(c.text),
                ..Default::default()
            },
        )]
    } else {
        Vec::new()
    };
    let base = if completes { c.text_muted } else { c.text };
    entry(
        SharedString::from(format!("suggestion:{i}")),
        IconName::Search,
        highlighted,
        c,
    )
    .debug_selector(|| format!("suggestion:{run}"))
    .child(
        div()
            .flex_1()
            .min_w_0()
            .truncate()
            .type_body()
            .text_color(base)
            .child(StyledText::new(suggestion).with_highlights(highlights)),
    )
    .on_mouse_down(
        MouseButton::Left,
        cx.listener(move |this, _, window, cx| {
            this.run_search(run.clone(), window, cx);
        }),
    )
    .into_any_element()
}
