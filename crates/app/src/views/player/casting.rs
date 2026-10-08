//! The Cast button and its device list. All playback remains backend commands.

use encore_core::backend::Command;
use encore_core::casting::{Device, Kind, Session};
use gpui_kit::assets::IconName;
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::app::MusicApp;
use crate::theme::{self, Colors, Type, elevation, radius, size, space};
use crate::views::widgets::{self, Pill};

pub fn button(app: &MusicApp, c: &Colors, cx: &mut Context<MusicApp>) -> Stateful<Div> {
    let active = matches!(
        app.cast.state.session,
        Some(Session::Active(_) | Session::Disconnecting(_))
    );
    widgets::icon_button(
        "cast",
        widgets::icon(IconName::Cast, size::ICON, widgets::toggle_color(active, c)),
        c,
    )
    .debug_selector(|| "cast-button".into())
    .tooltip(widgets::tooltip("Cast"))
    .on_click(cx.listener(|this, _, window, cx| {
        let open = !this.cast.open;
        if open {
            this.close_layers(window, cx);
        }
        this.show_cast(open, cx);
    }))
}

pub fn status(app: &MusicApp) -> Option<String> {
    match app.cast.state.session.as_ref()? {
        Session::Connecting(_) => Some("Connecting…".into()),
        Session::Active(device) => Some(format!(
            "{} {}",
            if app.player.playback.playing {
                "Playing on"
            } else {
                "Connected to"
            },
            device.name
        )),
        Session::Disconnecting(_) => Some("Returning to this computer…".into()),
        Session::Confirm { .. } => None,
    }
}

pub fn picker(app: &MusicApp, cx: &mut Context<MusicApp>) -> Option<AnyElement> {
    if !app.cast.open {
        return None;
    }
    let c = theme::colors(cx);
    let state = &app.cast.state;
    let header = h_flex()
        .gap(space::SM)
        .child(div().flex_1().type_heading().child("Cast"))
        .child(
            widgets::icon_button(
                "cast-refresh",
                widgets::icon(IconName::RefreshCw, size::ICON_SM, c.text_muted),
                &c,
            )
            .debug_selector(|| "cast-refresh".into())
            .tooltip(widgets::tooltip("Find devices"))
            .on_click(cx.listener(|this, _, _, _| this.send(Command::CastScan(true)))),
        )
        .child(
            widgets::icon_button(
                "cast-close",
                widgets::icon(IconName::X, size::ICON_SM, c.text_muted),
                &c,
            )
            .debug_selector(|| "cast-close".into())
            .tooltip(widgets::tooltip("Close"))
            .on_click(cx.listener(|this, _, _, cx| this.show_cast(false, cx))),
        );
    let mut body = v_flex().gap(space::SM);
    if let Some(Session::Confirm { device, app: other }) = &state.session {
        let device = device.clone();
        let yes = device.clone();
        body = body
            .child(
                div()
                    .type_body()
                    .debug_selector(|| "cast-confirm".into())
                    .child(format!("{} is playing {}. Replace it?", device.name, other)),
            )
            .child(
                h_flex()
                    .gap(space::SM)
                    .child(
                        widgets::pill_button("cast-replace", "Replace", None, Pill::Primary, &c)
                            .debug_selector(|| "cast-replace".into())
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.choose_cast(yes.clone(), true, cx)
                            })),
                    )
                    .child(
                        widgets::pill_button("cast-cancel", "Cancel", None, Pill::Secondary, &c)
                            .debug_selector(|| "cast-cancel".into())
                            .on_click(
                                cx.listener(|this, _, _, _| this.send(Command::CastDisconnect)),
                            ),
                    ),
            );
    } else {
        if state.session.is_some() {
            body = body
                .child(
                    div()
                        .type_small()
                        .text_color(c.signal)
                        .child(status(app).unwrap_or_default()),
                )
                .child(
                    widgets::pill_button("cast-stop", "Stop casting", None, Pill::Secondary, &c)
                        .debug_selector(|| "cast-stop".into())
                        .on_click(cx.listener(|this, _, _, _| this.send(Command::CastDisconnect))),
                );
        }
        if !state.devices.is_empty() {
            let scanning = state.scanning;
            body = body.child(
                div()
                    .h(space::XL)
                    .type_small()
                    .text_color(c.text_muted)
                    .debug_selector(move || {
                        if scanning {
                            "cast-scanning".into()
                        } else {
                            "cast-network".into()
                        }
                    })
                    .child(if scanning {
                        "Looking for devices…"
                    } else {
                        "On your network"
                    }),
            );
        } else if state.scanning {
            body = body.child(
                div()
                    .type_small()
                    .text_color(c.text_muted)
                    .debug_selector(|| "cast-scanning".into())
                    .child("Looking for devices…"),
            );
        } else if state.devices.is_empty() {
            body = body.child(div().type_body().debug_selector(|| "cast-empty".into()).child("No devices found"))
                .child(div().type_small().text_color(c.text_muted).child("Connect to the same network as your speaker or TV. Your firewall must allow local network connections."));
        }
        if app.player.current().is_none() {
            body = body.child(
                div()
                    .type_small()
                    .text_color(c.text_muted)
                    .debug_selector(|| "cast-needs-song".into())
                    .child("Choose a song to start casting"),
            );
        }
        body = body.child(
            v_flex()
                .id("cast-devices")
                .max_h(size::CAST_LIST_MAX)
                .overflow_y_scroll()
                .children(state.devices.iter().map(|device| row(device, app, &c, cx))),
        );
    }
    if let Some(error) = &state.error {
        body = body.child(div().type_small().text_color(c.danger).child(error.clone()));
    }
    Some(
        v_flex()
            .id("cast-picker")
            .debug_selector(|| "cast-picker".into())
            .absolute()
            .right(space::LG)
            .bottom(size::PLAYER_BAR + space::SM)
            .w(size::CAST_PICKER)
            .p(space::LG)
            .gap(space::MD)
            .rounded(radius::LG)
            .bg(c.overlay)
            .text_color(c.text)
            .shadow(elevation::high(&c))
            .occlude()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_mouse_down_out(cx.listener(|this, _, _, cx| this.show_cast(false, cx)))
            .child(header)
            .child(body)
            .into_any_element(),
    )
}

fn row(device: &Device, app: &MusicApp, c: &Colors, cx: &mut Context<MusicApp>) -> Stateful<Div> {
    let selected = app
        .cast
        .state
        .session
        .as_ref()
        .is_some_and(|s| s.device().id == device.id && s.device().kind == device.kind);
    let available = app.player.current().is_some() && app.cast.state.session.is_none();
    let icon = if device.group {
        IconName::Group
    } else if device.model.to_ascii_lowercase().contains("tv") {
        IconName::Tv
    } else {
        IconName::Speaker
    };
    let subtitle = match device.kind {
        Kind::Cast => "Google Cast",
        Kind::Dlna => "DLNA",
    };
    let device = device.clone();
    let choose = device.clone();
    let key = SharedString::from(format!("cast-device-{:?}-{}", device.kind, device.id));
    h_flex()
        .id(key)
        .debug_selector(move || format!("cast-device:{}", device.name))
        .min_h(size::ROW)
        .w_full()
        .px(space::SM)
        .gap(space::MD)
        .rounded(radius::MD)
        .when(selected, |s| s.bg(c.selected))
        .when(available, |s| {
            s.cursor_pointer()
                .hover(|s| s.bg(c.hover))
                .active(|s| s.bg(c.pressed))
        })
        .child(widgets::icon(
            icon,
            size::ICON,
            if selected { c.signal } else { c.text_muted },
        ))
        .child(
            v_flex()
                .flex_1()
                .min_w_0()
                .child(div().type_label().truncate().child(choose.name.clone()))
                .child(div().type_small().text_color(c.text_muted).child(subtitle)),
        )
        .when(selected, |s| {
            s.child(widgets::icon(IconName::Check, size::ICON_SM, c.signal))
        })
        .on_click(cx.listener(move |this, _, _, cx| {
            this.choose_cast(choose.clone(), false, cx);
        }))
}
