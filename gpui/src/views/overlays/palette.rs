//! Play anything (Ctrl+K): one large field high on a dimmed window, the
//! results under it, and a line of hints. The arrows move the highlight,
//! Enter plays it, Shift+Enter opens its page, Esc closes.

use gpui_kit::assets::IconName;
use gpui_kit::component::input::{Escape, Input, MoveDown, MoveUp};
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::keycap::{combo, keycap};
use crate::app::MusicApp;
use crate::desktop::palette::{Hit, Kind, PlayAnything};
use crate::theme::{self, Colors, Type, elevation, motion, radius, size, space};
use crate::views::widgets;

/// The panel's widest.
const WIDTH: Pixels = px(640.);
/// The field's row.
const FIELD: Pixels = px(60.);

pub fn palette(
    app: &mut MusicApp,
    window: &mut Window,
    cx: &mut Context<MusicApp>,
) -> Option<AnyElement> {
    app.refresh_hits();
    let pa = app.desktop.layers.palette.as_ref()?;
    let c = theme::colors(cx);
    let viewport = window.viewport_size();
    let width = WIDTH.min(viewport.width - space::XXXL);
    let hits = pa.hits();
    let panel =
        v_flex()
            .id("play-anything")
            .key_context("PlayAnything")
            .relative()
            .w(width)
            .rounded(radius::LG)
            .bg(c.overlay)
            .shadow(elevation::high(&c))
            .overflow_hidden()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            // Before the field moves its caret: the arrows walk the results.
            .capture_action(cx.listener(|this, _: &MoveUp, _, cx| {
                this.move_in_palette(-1, cx);
                cx.stop_propagation();
            }))
            .capture_action(cx.listener(|this, _: &MoveDown, _, cx| {
                this.move_in_palette(1, cx);
                cx.stop_propagation();
            }))
            .capture_action(cx.listener(|this, _: &Escape, window, cx| {
                this.close_play_anything(window, cx);
                cx.stop_propagation();
            }))
            .child(field(pa, &c))
            .when(!hits.is_empty(), |p| {
                p.child(div().h(px(1.)).bg(c.hairline))
                    .child(v_flex().p(space::XS).children(hits.iter().enumerate().map(
                        |(i, hit)| result(i, hit, i == pa.selected, &c, cx).into_any_element(),
                    )))
            })
            .child(footer(pa, &c))
            .with_animation(
                SharedString::from(format!("play-anything-{}", pa.serial)),
                Animation::new(motion::BASE).with_easing(motion::ease_out),
                |el, t| el.top(-space::MD * (1. - t)),
            );
    let top = (viewport.height * 0.14).max(space::XXXL);
    Some(
        super::scrim("play-anything-scrim", &c)
            .pt(top)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| this.close_play_anything(window, cx)),
            )
            .child(panel)
            .with_animation(
                SharedString::from(format!("play-anything-scrim-{}", pa.serial)),
                Animation::new(motion::BASE).with_easing(motion::ease_out),
                |el, t| el.opacity(t),
            )
            .into_any_element(),
    )
}

/// The field: a search glyph, the text, and a spinner while YouTube Music
/// is asked.
fn field(pa: &PlayAnything, c: &Colors) -> impl IntoElement {
    h_flex()
        .h(FIELD)
        .flex_none()
        .px(space::LG)
        .gap(space::MD)
        .child(widgets::icon(IconName::Search, size::ICON, c.text_muted))
        .child(
            Input::new(&pa.input)
                .appearance(false)
                .flex_1()
                .type_heading()
                .font_weight(FontWeight::NORMAL),
        )
        .when(pa.searching(), |f| {
            f.child(Spinner::new().color(c.text_faint))
        })
}

/// One result: cover (or a glyph for searches and commands), title and
/// detail, what it is, and ↵ on the highlighted one.
fn result(
    i: usize,
    hit: &Hit,
    selected: bool,
    c: &Colors,
    cx: &mut Context<MusicApp>,
) -> impl IntoElement {
    let lead = match (&hit.thumbnail, hit.kind) {
        (Some(url), kind) => widgets::cover(
            Some(url.clone().into()),
            size::ROW_THUMB,
            kind == Kind::Artist,
            c,
        )
        .into_any_element(),
        (None, kind) => h_flex()
            .flex_none()
            .size(size::ROW_THUMB)
            .justify_center()
            .rounded(radius::XS)
            .bg(c.raised)
            .child(widgets::icon(glyph(kind), size::ICON, c.text_muted))
            .into_any_element(),
    };
    h_flex()
        .id(("play-anything-hit", i))
        .h(size::ROW)
        .px(space::SM)
        .gap(space::MD)
        .rounded(radius::MD)
        .cursor_pointer()
        .when(selected, |r| r.bg(c.selected))
        .active(|r| r.bg(c.pressed))
        .child(lead)
        .child(
            v_flex()
                .flex_1()
                .min_w_0()
                .child(div().truncate().type_label().child(hit.title.clone()))
                .child(widgets::muted_line(hit.detail.clone(), c)),
        )
        .child(
            div()
                .flex_none()
                .type_caption()
                .text_color(c.text_faint)
                .child(hit.kind.label()),
        )
        .child(
            // Keeps its room so the labels don't shift as the highlight moves.
            div()
                .flex_none()
                .w(px(24.))
                .when(selected, |d| d.child(keycap("↵", c))),
        )
        // Moving the pointer (not a list appearing under it) highlights.
        .on_mouse_move(cx.listener(move |this, _: &MouseMoveEvent, _, cx| {
            if let Some(pa) = &mut this.desktop.layers.palette
                && pa.selected != i
            {
                pa.selected = i;
                cx.notify();
            }
        }))
        .on_click(cx.listener(move |this, event: &ClickEvent, window, cx| {
            this.choose_hit(i, event.modifiers().shift, window, cx)
        }))
}

fn glyph(kind: Kind) -> IconName {
    match kind {
        Kind::Search => IconName::RotateCcwClock,
        Kind::Command => IconName::Command,
        Kind::Artist => IconName::UserRound,
        Kind::Album => IconName::Disc3,
        Kind::Playlist => IconName::ListMusic,
        Kind::Song => IconName::Music,
    }
}

/// What's happening, or the keys.
fn footer(pa: &PlayAnything, c: &Colors) -> impl IntoElement {
    let empty = pa.hits().is_empty();
    let note = if let Some(note) = pa.note {
        Some(note)
    } else if empty && pa.query.trim().is_empty() {
        Some("Type a song, album or artist, or a command such as “next” or “radio …”.")
    } else if empty && pa.searching() {
        Some("Searching YouTube Music…")
    } else if pa.failed() {
        Some("Couldn't reach YouTube Music. Showing what's loaded here.")
    } else if empty {
        Some("Nothing matches that.")
    } else {
        None
    };
    let bar = h_flex()
        .h(px(44.))
        .flex_none()
        .px(space::LG)
        .gap(space::LG)
        .bg(c.hover)
        .type_caption()
        .text_color(c.text_faint);
    match note {
        Some(note) => bar.child(div().truncate().child(note)),
        None => bar
            .child(hint(&["↑", "↓"], "to choose", c))
            .child(hint(&["↵"], "to play", c))
            .child(hint(&["Shift", "↵"], "to open", c))
            .child(div().flex_1())
            .child(hint(&["Esc"], "to close", c)),
    }
}

fn hint(keys: &[&'static str], what: &'static str, c: &Colors) -> impl IntoElement {
    h_flex()
        .flex_none()
        .gap(space::SM)
        .child(combo(keys, c))
        .child(what)
}
