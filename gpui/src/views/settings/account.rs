//! Settings → Account: who is signed in and through what, Reconnect, and
//! which browser profile or cookie file to take the YouTube session from.

use gpui_kit::assets::IconName;
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use ytfast::auth::Profile;
use ytfast::model::Account;

use super::super::widgets::{self, Pill};
use crate::app::MusicApp;
use crate::theme::{Colors, Type, radius, size, space};

pub fn section(app: &MusicApp, c: &Colors, cx: &mut Context<MusicApp>) -> AnyElement {
    let mut rows = vec![status(app, c, cx)];
    rows.push(
        div()
            .pt(space::MD)
            .type_body()
            .child("Use the YouTube account signed in to")
            .into_any_element(),
    );
    if app.account.profiles.is_empty() {
        rows.push(
            div()
                .type_small()
                .text_color(c.text_muted)
                .child(
                    "No browser profile or cookie file is signed in to YouTube. Sign in to \
                     YouTube Music in a browser, then Reconnect.",
                )
                .into_any_element(),
        );
    }
    let current = app.account.profile.as_deref();
    rows.extend(
        app.account
            .profiles
            .iter()
            .enumerate()
            .map(|(i, p)| profile(i, p, current == Some(p.id.as_str()), c, cx)),
    );
    super::section("Account", c, rows)
}

/// The account as YouTube Music last confirmed it, and Reconnect.
fn status(app: &MusicApp, c: &Colors, cx: &mut Context<MusicApp>) -> AnyElement {
    let (lead, title, detail) = match &app.account.account {
        Account::SignedIn {
            name,
            photo,
            source,
        } => (
            super::super::account::avatar(photo.as_deref(), name, px(40.), cx).into_any_element(),
            name.clone(),
            format!("Signed in through {source}"),
        ),
        Account::Checking => (
            disc(IconName::UserRound, c),
            "Checking your account…".into(),
            "Asking YouTube Music who is signed in.".into(),
        ),
        Account::SignedOut { reason } => (
            disc(IconName::UserRound, c),
            "Signed out of YouTube Music".into(),
            reason.clone(),
        ),
        Account::Unverified { reason } => (
            disc(IconName::WifiOff, c),
            "Couldn't reach YouTube Music".into(),
            reason.clone(),
        ),
    };
    h_flex()
        .py(space::SM)
        .gap(space::MD)
        .child(lead)
        .child(
            v_flex()
                .flex_1()
                .min_w_0()
                .gap(space::XXS)
                .child(div().type_label().truncate().child(title))
                .child(widgets::muted_line(detail, c)),
        )
        .child(
            widgets::pill_button(
                "settings-reconnect",
                "Reconnect",
                Some(widgets::icon(IconName::RefreshCw, size::ICON_SM, c.text)),
                Pill::Secondary,
                c,
            )
            .on_click(cx.listener(|this, _, _, cx| this.reconnect(cx))),
        )
        .into_any_element()
}

fn disc(icon: IconName, c: &Colors) -> AnyElement {
    h_flex()
        .size(px(40.))
        .flex_none()
        .justify_center()
        .rounded(radius::FULL)
        .bg(c.raised)
        .child(widgets::icon(icon, size::ICON, c.text_muted))
        .into_any_element()
}

/// A browser profile or cookie file to sign in through, as a radio row.
fn profile(
    i: usize,
    profile: &Profile,
    current: bool,
    c: &Colors,
    cx: &mut Context<MusicApp>,
) -> AnyElement {
    let id = profile.id.clone();
    h_flex()
        .id(("profile", i))
        .h(px(44.))
        .mx(-space::SM)
        .px(space::SM)
        .gap(space::MD)
        .rounded(radius::MD)
        .cursor_pointer()
        .hover(|s| s.bg(c.hover))
        .active(|s| s.bg(c.pressed))
        .child(radio(current, c))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .type_body()
                .child(profile.label.clone()),
        )
        .on_click(cx.listener(move |this, _, _, cx| this.use_profile(id.clone(), cx)))
        .into_any_element()
}

/// A radio mark: a ring, with a `signal` dot when chosen.
fn radio(on: bool, c: &Colors) -> impl IntoElement {
    h_flex()
        .size(px(18.))
        .flex_none()
        .justify_center()
        .rounded(radius::FULL)
        .border_2()
        .border_color(if on { c.signal } else { c.text_faint })
        .when(on, |r| {
            r.child(div().size(px(8.)).rounded(radius::FULL).bg(c.signal))
        })
}
