//! Settings, over the window: one scrolling panel of sections (Account,
//! Playback, Notifications). Each section lives in its own file under
//! `settings/` and draws its rows with [`row`]; a new section (M6:
//! Equalizer, Sleep timer, Smooth mixes) is one more file and one more
//! entry in [`settings`].

mod account;
mod playback;

use gpui_kit::assets::IconName;
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::*;

use super::widgets;
use crate::app::MusicApp;
use crate::theme::{self, Colors, Type, size, space};

const WIDTH: Pixels = px(560.);

/// The Settings panel while it's open.
pub fn settings(
    app: &MusicApp,
    window: &mut Window,
    cx: &mut Context<MusicApp>,
) -> Option<AnyElement> {
    if !app.account.settings {
        return None;
    }
    let c = theme::colors(cx);
    // Leaves the window's edges visible around it, however small the window.
    let max_h = window.viewport_size().height - space::XXXL * 2.;
    let sections = [
        account::section(app, &c, cx),
        playback::section(app, &c, cx),
        playback::notifications(app, &c, cx),
    ];
    let panel = widgets::floating(&c)
        .id("settings")
        .track_focus(&app.account.focus)
        .w(WIDTH)
        .max_h(max_h)
        .flex()
        .flex_col()
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .capture_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
            if event.keystroke.key == "escape" {
                this.open_settings(false, window, cx);
                cx.stop_propagation();
            }
        }))
        .child(
            h_flex()
                .flex_none()
                .pl(space::XL)
                .pr(space::MD)
                .pt(space::MD)
                .pb(space::XS)
                .justify_between()
                .child(div().type_heading().child("Settings"))
                .child(
                    widgets::icon_button(
                        "settings-close",
                        widgets::icon(IconName::X, size::ICON, c.text_muted),
                        &c,
                    )
                    .tooltip(widgets::tooltip("Close"))
                    .on_click(
                        cx.listener(|this, _, window, cx| this.open_settings(false, window, cx)),
                    ),
                ),
        )
        .child(
            v_flex()
                .id("settings-sections")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .px(space::XL)
                .pb(space::XL)
                .gap(space::XL)
                .children(sections),
        );
    Some(
        widgets::scrim("settings-scrim", &c)
            .on_click(cx.listener(|this, _, window, cx| this.open_settings(false, window, cx)))
            .child(widgets::settle_in("settings-in", panel))
            .into_any_element(),
    )
}

/// A section: its name, then its rows.
fn section(
    name: &'static str,
    c: &Colors,
    rows: impl IntoIterator<Item = AnyElement>,
) -> AnyElement {
    v_flex()
        .gap(space::XS)
        .child(
            div()
                .pt(space::SM)
                .pb(space::XS)
                .type_label()
                .text_color(c.text_muted)
                .child(name),
        )
        .children(rows)
        .into_any_element()
}

/// A setting: what it is, a line saying what it does or what it is now,
/// and its control at the right.
fn row(
    label: impl Into<SharedString>,
    detail: Option<SharedString>,
    control: impl IntoElement,
    c: &Colors,
) -> AnyElement {
    h_flex()
        .py(space::SM)
        .gap(space::LG)
        .child(
            v_flex()
                .flex_1()
                .min_w_0()
                .gap(space::XXS)
                .child(div().type_body().child(label.into()))
                .children(detail.map(|d| div().type_small().text_color(c.text_muted).child(d))),
        )
        .child(control)
        .into_any_element()
}
