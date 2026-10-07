//! Settings, over the window: one scrolling panel of sections (Account,
//! Equalizer, Sleep timer, Loudness levelling, Smooth mixes,
//! Notifications, Updates). Each section lives in its own file under
//! `settings/` and draws its rows with [`row`] and its choices with [`choice`]; a new
//! section is one more file and one more entry in [`settings`].

mod account;
mod equalizer;
mod mixes;
mod motion;
mod playback;
mod sleep;
pub mod updates;

use gpui_kit::assets::IconName;
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::*;

use super::widgets;
use crate::app::MusicApp;
use crate::theme::{self, Colors, Type, radius, size, space};

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
        equalizer::section(app, &c, cx),
        sleep::section(app, &c, cx),
        playback::section(app, &c, cx),
        mixes::section(app, &c, window, cx),
        playback::notifications(app, &c, cx),
        motion::section(&c, cx),
        updates::section(app, &c, cx),
    ];
    let panel = widgets::floating(&c)
        .id("settings")
        .debug_selector(|| "settings".into())
        .key_context(crate::account::DIALOG_CONTEXT)
        .track_focus(&app.account.focus)
        .w(WIDTH)
        .max_h(max_h)
        .flex()
        .flex_col()
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
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
            .child(widgets::settle_in(
                "settings-in",
                theme::motion::Kind::Panels,
                panel,
            ))
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
                .child(div().type_body().tabular().child(label.into()))
                .children(detail.map(|d| {
                    div()
                        .type_small()
                        .tabular()
                        .text_color(c.text_muted)
                        .child(d)
                })),
        )
        .child(control)
        .into_any_element()
}

/// One of a set of choices (an equalizer preset, a sleep timer): a pill on
/// `raised`, or `primary` while it is the one in effect, as YouTube Music
/// draws its filter chips.
fn choice(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    chosen: bool,
    c: &Colors,
) -> Stateful<Div> {
    let (bg, hover, fg) = if chosen {
        (c.primary, c.primary_hover, c.primary_foreground)
    } else {
        (c.raised, c.overlay, c.text)
    };
    div()
        .id(id)
        .flex_none()
        .h(size::CHIP)
        .px(space::LG)
        .flex()
        .items_center()
        .rounded(radius::FULL)
        .bg(bg)
        .text_color(fg)
        .type_label()
        .cursor_pointer()
        .hover(move |s| s.bg(hover))
        .active(|s| s.opacity(0.9))
        .child(label.into())
}

/// A wrapping row of [`choice`]s under a setting.
fn choices(chips: impl IntoIterator<Item = Stateful<Div>>) -> AnyElement {
    h_flex()
        .flex_wrap()
        .gap(space::SM)
        .pb(space::SM)
        .children(chips)
        .into_any_element()
}
