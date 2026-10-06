//! M3: the account chip in the top bar, and account dialogs.

use gpui_kit::assets::IconName;
use gpui_kit::component::h_flex;
use gpui_kit::*;
use ytfast::model::Account;

use super::widgets;
use crate::app::MusicApp;
use crate::theme::{self, Type, radius, size, space};

pub fn chip(app: &MusicApp, _window: &mut Window, cx: &mut Context<MusicApp>) -> impl IntoElement {
    let c = theme::colors(cx);
    let label = match &app.account.account {
        Account::Checking => "Checking your account…".to_string(),
        Account::SignedIn { name, .. } => name.clone(),
        Account::SignedOut { .. } => "Signed out of YouTube Music".to_string(),
        Account::Unverified { .. } => "Offline".to_string(),
    };
    h_flex()
        .h(px(32.))
        .pl(space::SM)
        .pr(space::MD)
        .gap(space::SM)
        .rounded(radius::FULL)
        .bg(c.raised)
        .type_small()
        .text_color(c.text_muted)
        .child(widgets::icon(
            IconName::UserRound,
            size::ICON_SM,
            c.text_muted,
        ))
        .child(label)
}

/// M3: the like/dislike control for `track`, placed by the player bar and
/// Now Playing (M2). `None` until M3 fills it in or while signed out.
#[allow(dead_code, reason = "placed by M2's player bar")]
pub fn like_button(
    _app: &MusicApp,
    _track: &ytfast::model::Track,
    _cx: &mut Context<MusicApp>,
) -> Option<AnyElement> {
    None
}

/// M3: a page header's account actions (save to library, subscribe, edit
/// playlist), placed by the page header (M1). `None` when there are none.
pub fn header_actions(
    _app: &MusicApp,
    _header: &ytfast::model::Header,
    _cx: &mut Context<MusicApp>,
) -> Option<AnyElement> {
    None
}
