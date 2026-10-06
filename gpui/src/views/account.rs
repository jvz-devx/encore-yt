//! M3: the account chip in the top bar, and account dialogs.

use gpui_kit::component::ActiveTheme;
use gpui_kit::*;
use ytfast::model::Account;

use crate::app::MusicApp;

pub fn chip(app: &MusicApp, _window: &mut Window, cx: &mut Context<MusicApp>) -> impl IntoElement {
    let label = match &app.account.account {
        Account::Checking => "Checking your account…".to_string(),
        Account::SignedIn { name, .. } => name.clone(),
        Account::SignedOut { .. } => "Signed out of YouTube Music".to_string(),
        Account::Unverified { .. } => "Offline".to_string(),
    };
    div()
        .text_sm()
        .text_color(cx.theme().muted_foreground)
        .child(label)
}
