//! The account menu under the chip: who is signed in and through which
//! browser or cookie file (or why nobody is), the account's channels to
//! switch between, Reconnect, and Settings.

use gpui_kit::assets::IconName;
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use ytfast::model::Account;

use super::super::widgets;
use crate::app::MusicApp;
use crate::desktop::lists::List;
use crate::theme::{self, Colors, Type, radius, size, space};

const WIDTH: Pixels = px(300.);

pub fn menu(app: &MusicApp, _window: &mut Window, cx: &mut Context<MusicApp>) -> impl IntoElement {
    let c = theme::colors(cx);
    let nav = &app.desktop.lists.account;
    let selected = nav.selected;
    let channels = super::channels::rows(app, "menu-channel", true, &c, cx);
    let n = channels.len();
    let divider = || div().mx(space::MD).my(space::XS).h(px(1.)).bg(c.hairline);
    let mut i = n;
    let mut item =
        |id: &'static str, icon: IconName, label: &'static str, cx: &mut Context<MusicApp>| {
            let entry = widgets::menu_item(id, icon, label, &c)
                .debug_selector(move || format!("account-menu:{label}"));
            let entry = super::super::menu::list_entry(entry, List::Account, i, selected, &c, cx);
            i += 1;
            entry
        };
    let sign_in = matches!(app.account.account, Account::SignedOut { .. })
        .then(|| item("menu-sign-in", IconName::LogIn, "Sign in", cx));
    let reconnect = item("menu-reconnect", IconName::RefreshCw, "Reconnect", cx);
    let settings = item("menu-settings", IconName::Settings, "Settings", cx);
    let panel = widgets::floating(&c)
        .id("account-menu")
        .debug_selector(|| "account-menu".into())
        .w(WIDTH)
        .p(space::XS)
        .occlude()
        .on_mouse_down_out(cx.listener(|this, _, window, cx| this.close_account_menu(window, cx)))
        .child(identity(&app.account.account, &c, cx))
        .when(n > 0, |el| {
            el.child(divider())
                .children(channels.into_iter().enumerate().map(|(k, row)| {
                    super::super::menu::list_highlight(
                        row.px(space::MD),
                        List::Account,
                        k,
                        selected,
                        &c,
                        cx,
                    )
                }))
        })
        .child(divider())
        .children(sign_in)
        .child(reconnect)
        .child(settings);
    let panel = super::super::menu::list_keys(
        panel,
        List::Account,
        &nav.focus,
        |this, window, cx| this.close_account_menu(window, cx),
        cx,
    );
    // Hangs from the chip's bottom right corner, above the page.
    div().absolute().right_0().top(px(48.)).child(deferred(
        anchored()
            .anchor(Anchor::TopRight)
            .snap_to_window_with_margin(space::SM)
            .child(widgets::settle_in(
                "account-menu-in",
                theme::motion::Kind::Menus,
                panel,
            )),
    ))
}

/// Who is signed in, or what is wrong, at the top of the menu.
fn identity(account: &Account, c: &Colors, cx: &App) -> impl IntoElement {
    let (lead, title, detail): (AnyElement, String, String) = match account {
        Account::SignedIn {
            name,
            photo,
            source,
        } => (
            super::avatar(photo.as_deref(), name, px(40.), cx).into_any_element(),
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
            format!(
                "{}. Sign in to YouTube Music in your browser, then Reconnect.",
                reason.trim_end_matches('.')
            ),
        ),
        Account::Unverified { reason } => (
            disc(IconName::WifiOff, c),
            "Couldn't reach YouTube Music".into(),
            format!(
                "{}. Check the connection, then Reconnect.",
                reason.trim_end_matches('.')
            ),
        ),
    };
    h_flex()
        .items_start()
        .p(space::MD)
        .gap(space::MD)
        .child(lead)
        .child(
            v_flex()
                .flex_1()
                .min_w_0()
                .gap(space::XXS)
                .child(div().type_label().truncate().child(title))
                .child(
                    div()
                        .type_small()
                        .text_color(c.text_muted)
                        .line_clamp(3)
                        .child(detail),
                ),
        )
}

/// A 40 px disc with a muted icon, in a person's place.
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
