//! M3: the account chip and its menu, the like buttons, a page header's
//! account actions, song row controls, and the playlist dialogs.
//!
//! Other areas place these slots: the player bar and Now Playing
//! [`like_button`] (M2), the page header [`header_actions`], song rows
//! [`row_actions`] and Library's [`library_actions`] (M1). Context menus
//! open the playlist picker with [`add_to_playlist_menu`].

mod chip;
mod controls;
mod dialogs;
mod menu;

use gpui_kit::*;

use crate::app::MusicApp;

pub use chip::chip;
#[allow(unused_imports, reason = "placed by M1 and M2 (see the module doc)")]
pub use controls::{
    header_actions, library_actions, like_button, remove_from_playlist, row_actions,
};

/// Opens Add to playlist ("Save to playlist") for `tracks`.
#[allow(dead_code, reason = "called by M4's context menus")]
pub fn add_to_playlist_menu(
    app: &mut MusicApp,
    tracks: Vec<ytfast::model::Track>,
    window: &mut Window,
    cx: &mut Context<MusicApp>,
) {
    crate::account::add_to_playlist(app, tracks, window, cx);
}

/// The layer over the window: an open playlist dialog, else Settings.
pub fn layer(
    app: &MusicApp,
    window: &mut Window,
    cx: &mut Context<MusicApp>,
) -> Option<AnyElement> {
    dialogs::dialog(app, window, cx).or_else(|| super::settings::settings(app, window, cx))
}

/// A person's photo as a disc, with their initial underneath while it loads
/// (or when there is none).
pub(super) fn avatar(photo: Option<&str>, name: &str, side: Pixels, cx: &App) -> Div {
    let c = crate::theme::colors(cx);
    let initial = name
        .chars()
        .next()
        .map(|ch| ch.to_uppercase().to_string())
        .unwrap_or_default();
    use crate::theme::{Type, radius};
    gpui_kit::component::h_flex()
        .relative()
        .flex_none()
        .size(side)
        .justify_center()
        .rounded(radius::FULL)
        .bg(c.overlay)
        .type_label()
        .text_color(c.text_muted)
        .child(initial)
        .children(photo.map(|url| {
            img(SharedString::from(url.to_string()))
                .absolute()
                .top_0()
                .left_0()
                .size(side)
                .rounded(radius::FULL)
                .object_fit(ObjectFit::Cover)
        }))
        .child(
            div()
                .absolute()
                .inset_0()
                .rounded(radius::FULL)
                .border_1()
                .border_color(c.outline),
        )
}
