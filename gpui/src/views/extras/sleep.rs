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
use crate::theme::{Colors, Type, elevation, motion, radius, size, space};

const WIDTH: Pixels = px(232.);

const CHOICES: [(&str, Sleep); 5] = [
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
    panel
        .with_animation(
            id,
            Animation::new(motion::FAST).with_easing(motion::ease_out),
            |el, t| el.opacity(t).mb(px(6.) * (1. - t)),
        )
        .into_any_element()
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
    // YTFAST_GPUI_SHORT_SLEEP=1 offers one minute, to check the fade.
    let short = std::env::var_os("YTFAST_GPUI_SHORT_SLEEP")
        .is_some()
        .then_some(("1 minute", Sleep::Minutes(1)));
    let rows = short
        .into_iter()
        .chain(CHOICES)
        .enumerate()
        .map(|(i, (label, choice))| {
            option(("sleep-choice", i), label, chosen == Some(choice), c).on_click(cx.listener(
                move |this, _, window, cx| {
                    this.sleep(Some(choice), cx);
                    this.close_panels(window, cx);
                },
            ))
        })
        .collect::<Vec<_>>();
    let off = timer.is_some().then(|| {
        option(("sleep-choice", 9usize), "Turn off", false, c).on_click(cx.listener(
            |this, _, window, cx| {
                this.sleep(None, cx);
                this.close_panels(window, cx);
            },
        ))
    });
    let panel = floating("sleep-menu", app, c, cx)
        .w(WIDTH)
        .p(space::XS)
        .child(header)
        .children(rows)
        .children(off.map(|off| {
            v_flex()
                .child(div().mx(space::MD).my(space::XS).h(px(1.)).bg(c.hairline))
                .child(off)
        }));
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
        .hover(|s| s.bg(c.hover))
        .active(|s| s.bg(c.pressed))
        .child(div().flex_1().child(label))
        .when(on, |s| {
            s.child(widgets::icon(IconName::Check, size::ICON_SM, c.signal))
        })
}
