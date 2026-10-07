//! The account chip at the right of the top bar: who is signed in (photo
//! and name), or that nobody is. It opens the account menu.

use encore_core::model::Account;
use gpui_kit::assets::IconName;
use gpui_kit::component::h_flex;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::super::widgets;
use crate::app::MusicApp;
use crate::theme::{self, Type, radius, size, space};

/// The avatar inside the 40 px chip, inset `XS` so the shapes stay
/// concentric.
const AVATAR: Pixels = px(32.);

pub fn chip(app: &MusicApp, window: &mut Window, cx: &mut Context<MusicApp>) -> AnyElement {
    let c = theme::colors(cx);
    if crate::sign_in::SignIn::needed(&app.account.account, &app.account.profiles) {
        return sign_in(&c, cx);
    }
    let (lead, label): (AnyElement, String) = match &app.account.account {
        Account::Checking => (
            widgets::skeleton(AVATAR, AVATAR, radius::FULL, &c).into_any_element(),
            "Checking…".into(),
        ),
        Account::SignedIn { name, photo, .. } => (
            super::avatar(photo.as_deref(), name, AVATAR, cx).into_any_element(),
            name.clone(),
        ),
        Account::SignedOut { .. } => (status_icon(IconName::UserRound, cx), "Signed out".into()),
        Account::Unverified { .. } => (status_icon(IconName::WifiOff, cx), "Offline".into()),
    };
    let open = app.account.menu;
    div()
        .relative()
        .flex_none()
        .child(
            h_flex()
                .id("account-chip")
                .h(px(40.))
                .pl(space::XS)
                .pr(space::LG)
                .gap(space::SM)
                .rounded(radius::FULL)
                .bg(if open { c.pressed } else { c.raised })
                .type_label()
                .cursor_pointer()
                .hover(|s| s.bg(c.overlay))
                .active(|s| s.opacity(0.9))
                .child(lead)
                .child(div().max_w(px(160.)).truncate().child(label))
                .on_click(cx.listener(|this, _, window, cx| this.toggle_account_menu(window, cx))),
        )
        .when(open, |el| el.child(super::menu::menu(app, window, cx)))
        .into_any_element()
}

/// Nobody to reconnect: the chip is the Sign in button (M18).
fn sign_in(c: &crate::theme::Colors, cx: &mut Context<MusicApp>) -> AnyElement {
    widgets::pill_button(
        "account-sign-in",
        "Sign in",
        Some(widgets::icon(IconName::LogIn, size::ICON_SM, c.text)),
        widgets::Pill::Primary,
        c,
    )
    .h(px(40.))
    .on_click(cx.listener(|this, _, window, cx| this.open_sign_in(window, cx)))
    .into_any_element()
}

/// A muted icon in the avatar's place, for the signed-out and offline chips.
fn status_icon(icon: IconName, cx: &App) -> AnyElement {
    let c = theme::colors(cx);
    h_flex()
        .size(AVATAR)
        .flex_none()
        .justify_center()
        .rounded(radius::FULL)
        .bg(c.overlay)
        .child(widgets::icon(icon, size::ICON_SM, c.text_muted))
        .into_any_element()
}
