//! The keyboard shortcuts sheet (`?`): every shortcut, grouped, over a
//! dimmed window. Esc, `?` or a click outside closes it.

use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::*;

use super::keycap::{combo, keycap};
use crate::app::MusicApp;
use crate::desktop::{Group, SHORTCUTS};
use crate::theme::motion::MotionExt as _;
use crate::theme::{self, Colors, Type, elevation, motion, radius, space};

/// The sheet's widest.
const WIDTH: Pixels = px(760.);
/// A shortcut line's height.
const LINE: Pixels = px(34.);

/// The columns, left to right.
const COLUMNS: [&[Group]; 2] = [
    &[Group::Playback, Group::Navigation, Group::Menus],
    &[Group::Views, Group::Library, Group::Settings],
];

pub fn sheet(window: &mut Window, cx: &mut Context<MusicApp>) -> AnyElement {
    let c = theme::colors(cx);
    let viewport = window.viewport_size();
    let width = WIDTH.min(viewport.width - space::XXXL);
    let panel = v_flex()
        .id("shortcuts")
        .relative()
        .w(width)
        .max_h(viewport.height - space::XXXL * 2.)
        .rounded(radius::LG)
        .bg(c.overlay)
        .shadow(elevation::high(&c))
        .overflow_hidden()
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .child(header(&c))
        .child(
            h_flex()
                .id("shortcuts-body")
                .min_h_0()
                .items_start()
                .gap(space::XXL)
                .px(space::XL)
                .pb(space::XL)
                .overflow_y_scroll()
                .children(COLUMNS.iter().map(|groups| column(groups, &c))),
        )
        .with_motion(
            "shortcuts-panel",
            motion::Kind::Panels,
            motion::BASE,
            |el, t| el.top(space::SM * (1. - t)),
        );
    super::scrim("shortcuts-scrim", &c)
        .justify_center()
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, _, window, cx| this.show_shortcuts(false, window, cx)),
        )
        .child(panel)
        .with_motion(
            "shortcuts-scrim",
            motion::Kind::Panels,
            motion::BASE,
            |el, t| el.opacity(t),
        )
}

/// The title, and how to close.
fn header(c: &Colors) -> impl IntoElement {
    h_flex()
        .flex_none()
        .px(space::XL)
        .pt(space::XL)
        .pb(space::LG)
        .gap(space::SM)
        .child(div().flex_1().type_heading().child("Keyboard shortcuts"))
        .child(keycap("Esc", c))
        .child(
            div()
                .type_small()
                .text_color(c.text_faint)
                .child("to close"),
        )
}

fn column(groups: &[Group], c: &Colors) -> impl IntoElement {
    v_flex()
        .flex_1()
        .min_w_0()
        .gap(space::XL)
        .children(groups.iter().map(|g| group(*g, c)))
}

fn group(group: Group, c: &Colors) -> impl IntoElement {
    v_flex()
        .child(
            div()
                .pb(space::XS)
                .type_label()
                .text_color(c.text_muted)
                .child(group.title()),
        )
        .children(lines(group, c))
}

/// The group's shortcuts from the one table (`desktop::SHORTCUTS`), a line
/// each.
pub(crate) fn lines(group: Group, c: &Colors) -> impl Iterator<Item = Div> + '_ {
    SHORTCUTS
        .iter()
        .filter(move |s| s.group == group)
        .map(|s| line(s.keys, s.what, c))
}

/// What it does on the left, its keys on the right ("or" between
/// alternatives).
fn line(keys: &[&[&'static str]], what: &'static str, c: &Colors) -> Div {
    let mut caps = h_flex().flex_none().gap(space::SM);
    for (i, keys) in keys.iter().enumerate() {
        if i > 0 {
            caps = caps.child(div().type_caption().text_color(c.text_faint).child("or"));
        }
        caps = caps.child(combo(keys, c));
    }
    // A long description wraps rather than being cut off beside wide
    // keycaps (two-column layouts are narrow).
    h_flex()
        .min_h(LINE)
        .py(space::XS)
        .gap(space::LG)
        .border_b_1()
        .border_color(c.hairline)
        .child(div().flex_1().min_w_0().type_body().child(what))
        .child(caps)
}
