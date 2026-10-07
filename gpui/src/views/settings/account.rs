//! Settings → Account: who is signed in and through what, Reconnect, which
//! of the account's channels to act as, and which browser profile or cookie
//! file to take the YouTube session from.

use gpui_kit::assets::IconName;
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use ytfast::auth::Profile;
use ytfast::model::Account;

use super::super::widgets::{self, Pill};
use crate::app::MusicApp;
use crate::theme::{Colors, Type, radius, size, space};

pub fn page(app: &MusicApp, c: &Colors, cx: &mut Context<MusicApp>) -> Vec<AnyElement> {
    let mut sections = vec![super::section("", c, [status(app, c, cx)])];
    let channels = super::super::account::channel_rows(app, "settings-channel", c, cx);
    if !channels.is_empty() {
        sections.push(super::section(
            "Use YouTube Music as",
            c,
            channels
                .into_iter()
                .map(|row| row.mx(-space::SM).px(space::SM).into_any_element()),
        ));
    }
    let mut rows = Vec::new();
    if app.account.profiles.is_empty() {
        rows.push(
            div()
                .py(space::SM)
                .type_small()
                .text_color(c.text_muted)
                .child("No browser profile or cookie file is signed in to YouTube.")
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
    // Signed out with nothing to reconnect, the status row has Sign in.
    if !crate::sign_in::SignIn::needed(&app.account.account, &app.account.profiles) {
        rows.push(another_way(c, cx));
    }
    sections.push(super::section(
        "Use the YouTube account signed in to",
        c,
        rows,
    ));
    sections
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
        .py(space::MD)
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
        .child(super::focusable(
            if crate::sign_in::SignIn::needed(&app.account.account, &app.account.profiles) {
                widgets::pill_button(
                    "settings-sign-in",
                    "Sign in",
                    Some(widgets::icon(IconName::LogIn, size::ICON_SM, c.text)),
                    Pill::Primary,
                    c,
                )
                .on_click(cx.listener(|this, _, window, cx| this.open_sign_in(window, cx)))
            } else {
                widgets::pill_button(
                    "settings-reconnect",
                    "Reconnect",
                    Some(widgets::icon(IconName::RefreshCw, size::ICON_SM, c.text)),
                    Pill::Tonal,
                    c,
                )
                .on_click(cx.listener(|this, _, _, cx| this.reconnect(cx)))
            },
            c,
        ))
        .into_any_element()
}

/// Opens the sign-in sheet to add a browser, a cookies file or pasted
/// cookies (M18).
fn another_way(c: &Colors, cx: &mut Context<MusicApp>) -> AnyElement {
    h_flex()
        .pt(space::SM)
        .pb(space::MD)
        .child(
            super::focusable(
                widgets::pill_button(
                    "settings-sign-in-other",
                    "Sign in another way",
                    Some(widgets::icon(IconName::LogIn, size::ICON_SM, c.text)),
                    Pill::Tonal,
                    c,
                ),
                c,
            )
            .on_click(cx.listener(|this, _, window, cx| this.open_sign_in(window, cx))),
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
    let row = h_flex()
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
        .on_click(cx.listener(move |this, _, _, cx| this.use_profile(id.clone(), cx)));
    super::focusable(row, c).into_any_element()
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
