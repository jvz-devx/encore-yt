//! Settings' search results in place of a category: the matching settings
//! grouped by category, each with its label, its line and the tab it is
//! on. A click (or ↑/↓ and Enter in the field) opens it.

use gpui_kit::assets::IconName;
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::super::widgets;
use crate::app::MusicApp;
use crate::settings::{self as nav, Category, Entry};
use crate::theme::{Colors, Type, radius, size, space};

pub fn content(app: &MusicApp, c: &Colors, cx: &mut Context<MusicApp>) -> AnyElement {
    let query = app.settings.query.clone();
    let hits = nav::search(&query);
    let blurb = match hits.len() {
        0 => "No settings match".to_string(),
        1 => "1 setting".to_string(),
        n => format!("{n} settings"),
    };
    let header = super::frame::header(format!("Results for “{query}”"), blurb, None, c, cx);
    let body = if hits.is_empty() {
        vec![
            div()
                .pt(space::XXXL)
                .child(widgets::empty_state(
                    IconName::SearchX,
                    "No settings match",
                    "Try another word, like crossfade, lyrics or blur.",
                    c,
                ))
                .into_any_element(),
        ]
    } else {
        groups(&hits, app.settings.hit, c, cx)
    };
    super::frame::panel(header, None, body, "settings-results".into(), c)
}

/// One section per category with matches, in the sidebar's order.
fn groups(
    hits: &[Entry],
    selected: usize,
    c: &Colors,
    cx: &mut Context<MusicApp>,
) -> Vec<AnyElement> {
    let mut out = Vec::new();
    for category in Category::ALL {
        let rows: Vec<AnyElement> = hits
            .iter()
            .enumerate()
            .filter(|(_, e)| e.category == category)
            .map(|(i, e)| hit(i, e, i == selected, c, cx))
            .collect();
        if rows.is_empty() {
            continue;
        }
        out.push(
            v_flex()
                .gap(space::SM)
                .child(
                    h_flex()
                        .gap(space::SM)
                        .text_color(c.text_muted)
                        .child(widgets::icon(category.icon(), size::ICON_SM, c.text_muted))
                        .child(div().type_label().child(category.label())),
                )
                .child(
                    v_flex()
                        .rounded(radius::MD)
                        .bg(c.hover)
                        .p(space::XS)
                        .gap(space::XXS)
                        .children(rows),
                )
                .into_any_element(),
        );
    }
    out
}

/// A matching setting: label and line, its tab, and a chevron.
fn hit(i: usize, e: &Entry, selected: bool, c: &Colors, cx: &mut Context<MusicApp>) -> AnyElement {
    let (category, tab) = (e.category, e.tab);
    let tab_name = tab.and_then(|t| category.tabs().get(t).copied());
    let name: SharedString = format!("settings-hit:{}", e.label).into();
    let (hover, pressed) = (c.hover, c.pressed);
    let row = h_flex()
        .id(("settings-hit", i))
        .debug_selector(move || name.to_string())
        .min_h(size::ROW)
        .px(space::MD)
        .py(space::SM)
        .gap(space::MD)
        .rounded(radius::SM)
        .cursor_pointer()
        .when(selected, |r| r.bg(c.selected))
        .when(!selected, |r| r.hover(move |r| r.bg(hover)))
        .active(move |r| r.bg(pressed))
        .child(
            v_flex()
                .flex_1()
                .min_w_0()
                .gap(space::XXS)
                .child(div().type_label().truncate().child(e.label.clone()))
                .child(widgets::muted_line(e.about.clone(), c)),
        )
        .children(tab_name.map(|t| {
            div()
                .flex_none()
                .type_caption()
                .text_color(c.text_faint)
                .child(t)
        }))
        .child(widgets::icon(
            IconName::ChevronRight,
            size::ICON_SM,
            c.text_faint,
        ))
        .on_click(
            cx.listener(move |this, _, window, cx| this.jump_to_setting(category, tab, window, cx)),
        );
    super::focusable(row, c).into_any_element()
}
