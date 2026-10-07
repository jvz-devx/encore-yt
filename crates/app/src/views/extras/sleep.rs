//! The sleep timer's menu, rising above the player bar: how long, or the
//! end of the song; and Turn off while one is set.

use gpui_kit::assets::IconName;
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use ytfast::model::Sleep;

use super::super::widgets;
use super::controls::time_left;
use crate::app::MusicApp;
use crate::desktop::lists::{List, sleep_items};
use crate::theme::motion::MotionExt as _;
use crate::theme::{Colors, Type, elevation, motion, radius, size, space};

const WIDTH: Pixels = px(232.);

/// The timers offered, here and in Settings → Sleep timer.
pub const CHOICES: [(&str, Sleep); 5] = [
    ("15 minutes", Sleep::Minutes(15)),
    ("30 minutes", Sleep::Minutes(30)),
    ("45 minutes", Sleep::Minutes(45)),
    ("1 hour", Sleep::Minutes(60)),
    ("End of song", Sleep::EndOfSong),
];

/// A floating panel above the player bar's right side, closed by a click
/// outside it or Esc.
pub fn floating(
    id: &'static str,
    app: &MusicApp,
    c: &Colors,
    cx: &mut Context<MusicApp>,
) -> Stateful<Div> {
    v_flex()
        .id(id)
        .key_context("ExtrasPanel")
        .track_focus(&app.extras.panel_focus)
        .absolute()
        .right_0()
        .bottom(size::PLAYER_BAR)
        .rounded(radius::LG)
        .bg(c.overlay)
        .shadow(elevation::high(c))
        .text_color(c.text)
        .cursor_default()
        // Clicks inside stay inside.
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_mouse_down_out(cx.listener(|this, _, window, cx| this.close_panels_outside(window, cx)))
}

/// A panel settles in: it fades and rises a few pixels.
pub fn appear(panel: Stateful<Div>, id: &'static str) -> AnyElement {
    panel.with_motion(id, motion::Kind::Menus, motion::FAST, |el, t| {
        el.opacity(t).mb(px(6.) * (1. - t))
    })
}

pub fn menu(app: &MusicApp, c: &Colors, cx: &mut Context<MusicApp>) -> AnyElement {
    let timer = app.player.playback.sleep;
    let chosen = timer.map(|t| t.choice);
    let header = v_flex()
        .px(space::MD)
        .pt(space::SM)
        .pb(space::XS)
        .gap(space::XXS)
        .child(div().type_label().child("Sleep timer"))
        .child(
            div()
                .type_small()
                .tabular()
                .text_color(if timer.is_some() {
                    c.signal
                } else {
                    c.text_muted
                })
                .child(match &timer {
                    Some(t) if t.deadline.is_some() => format!("Pauses in {}", time_left(t)),
                    Some(_) => "Pauses when this song ends".into(),
                    None => "Fades out, then pauses".into(),
                }),
        );
    let nav = &app.desktop.lists.sleep;
    let selected = nav.selected;
    let items = sleep_items(app);
    let mut rows = Vec::new();
    let mut off = None;
    for (i, (label, choice)) in items.into_iter().enumerate() {
        let on = choice.is_some() && chosen == choice;
        let row = option(("sleep-choice", i), label, on, c)
            .debug_selector(move || format!("sleep-choice:{label}"));
        let row = super::super::menu::list_entry(row, List::Sleep, i, selected, c, cx);
        if choice.is_some() {
            rows.push(row);
        } else {
            off = Some(row);
        }
    }
    // The list holds the keyboard inside the panel (M29).
    let list = super::super::menu::list_keys(
        v_flex().id("sleep-list"),
        List::Sleep,
        &nav.focus,
        |this, window, cx| this.close_panels(window, cx),
        cx,
    )
    .child(header)
    .children(rows)
    .children(off.map(|off| {
        v_flex()
            .child(div().mx(space::MD).my(space::XS).h(px(1.)).bg(c.hairline))
            .child(off)
    }));
    let panel = floating("sleep-menu", app, c, cx)
        .w(WIDTH)
        .p(space::XS)
        .child(list);
    appear(panel, "sleep-menu")
}

/// One choice: 36 tall, its label, and a check while it is the one set.
fn option(id: impl Into<ElementId>, label: &'static str, on: bool, c: &Colors) -> Stateful<Div> {
    h_flex()
        .id(id)
        .h(size::CHIP)
        .px(space::MD)
        .gap(space::SM)
        .rounded(radius::MD)
        .type_body()
        .cursor_pointer()
        .active(|s| s.bg(c.pressed))
        .child(div().flex_1().child(label))
        .when(on, |s| {
            s.child(widgets::icon(IconName::Check, size::ICON_SM, c.signal))
        })
}
