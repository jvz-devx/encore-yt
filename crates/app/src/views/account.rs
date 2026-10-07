//! M3: the account chip and its menu, the like buttons, a page header's
//! account actions, Up next's Save, and the playlist dialogs.
//!
//! Other areas place these slots: the player bar and Now Playing
//! [`like_button`] (M2), the page header [`header_actions`], Library's
//! [`library_actions`] (M1) and Up next's [`save_queue_button`]. Context
//! menus open the playlist picker with `crate::account::add_to_playlist`.

mod channels;
mod chip;
mod controls;
mod dialogs;
mod like_mark;
mod menu;
mod sign_in;

use gpui_kit::*;

use crate::app::MusicApp;

pub use channels::rows as channel_rows;
pub use chip::chip;
pub use controls::{header_actions, library_actions, like_button, save_queue_button};
pub use like_mark::{row_like, row_like_space};

/// The layer over the window: an open playlist dialog, else Settings.
pub fn layer(
    app: &MusicApp,
    window: &mut Window,
    cx: &mut Context<MusicApp>,
) -> Option<AnyElement> {
    sign_in::sheet(app, window, cx)
        .or_else(|| dialogs::dialog(app, window, cx))
        .or_else(|| super::settings::settings(app, window, cx))
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
