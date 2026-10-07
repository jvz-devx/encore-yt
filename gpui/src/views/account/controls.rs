//! The account's controls placed by other areas: like and dislike, Save to
//! playlist, Save to library, Subscribe, the own playlist's Edit and
//! Delete, New playlist, and Up next's Save. Each sends
//! an [`AccountAction`]; `crate::account` shows it at once and rolls it
//! back if YouTube Music refuses.

use gpui_kit::assets::IconName;
use gpui_kit::component::h_flex;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use ytfast::account::{AccountAction, Dialog};
use ytfast::model::{Header, LikeStatus, Track};

use super::super::widgets::{self, Pill};
use crate::app::MusicApp;
use crate::assets::Glyph;
use crate::theme::{self, Colors, size, space};

/// Dislike, Like and Save to playlist for `track` (the player bar and Now
/// Playing). `None` while signed out.
pub fn like_button(
    app: &MusicApp,
    track: &Track,
    cx: &mut Context<MusicApp>,
) -> Option<AnyElement> {
    if !app.account.signed_in() {
        return None;
    }
    let c = theme::colors(cx);
    let status = app.account.state.marks.like(track);
    let (liked, disliked) = (status == LikeStatus::Like, status == LikeStatus::Dislike);
    let save = {
        let track = track.clone();
        cx.listener(move |this, _: &ClickEvent, window, cx| {
            cx.stop_propagation();
            crate::account::add_to_playlist(this, vec![track.clone()], window, cx);
        })
    };
    Some(
        h_flex()
            .flex_none()
            .gap(space::XXS)
            .child(
                widgets::icon_button("dislike", dislike_icon(disliked, &c), &c)
                    .tooltip(widgets::tooltip(if disliked {
                        "Remove dislike"
                    } else {
                        "Dislike"
                    }))
                    .on_click(rate(
                        track.clone(),
                        if disliked {
                            LikeStatus::Indifferent
                        } else {
                            LikeStatus::Dislike
                        },
                        cx,
                    )),
            )
            .child(
                widgets::icon_button("like", like_icon(liked, size::ICON, &c), &c)
                    .tooltip(widgets::tooltip(if liked { "Remove like" } else { "Like" }))
                    .on_click(rate(
                        track.clone(),
                        if liked {
                            LikeStatus::Indifferent
                        } else {
                            LikeStatus::Like
                        },
                        cx,
                    )),
            )
            .child(
                widgets::icon_button(
                    "save-to-playlist",
                    widgets::icon(IconName::ListPlus, size::ICON, c.text_muted),
                    &c,
                )
                .tooltip(widgets::tooltip("Save to playlist"))
                .on_click(save),
            )
            .into_any_element(),
    )
}

/// Gives `track` the rating `status` on click.
fn rate(
    track: Track,
    status: LikeStatus,
    cx: &mut Context<MusicApp>,
) -> impl Fn(&ClickEvent, &mut Window, &mut App) + 'static {
    cx.listener(move |this, _: &ClickEvent, _, cx| {
        cx.stop_propagation();
        let track = track.clone();
        this.account_act(AccountAction::Rate { track, status }, cx);
    })
}

/// A thumb up: filled in `signal` when liked (a toggle that is on).
fn like_icon(liked: bool, px: Pixels, c: &Colors) -> AnyElement {
    if liked {
        widgets::glyph(Glyph::ThumbsUp, px, c.signal).into_any_element()
    } else {
        widgets::icon(IconName::ThumbsUp, px, c.text_muted).into_any_element()
    }
}

/// A thumb down: filled when disliked. Not `signal`: a dislike isn't live.
fn dislike_icon(disliked: bool, c: &Colors) -> AnyElement {
    if disliked {
        widgets::glyph(Glyph::ThumbsDown, size::ICON, c.text).into_any_element()
    } else {
        widgets::icon(IconName::ThumbsDown, size::ICON, c.text_muted).into_any_element()
    }
}

/// A page header's account actions: Save to library (albums, other
/// people's playlists), Subscribe (artists), Edit and Delete (the
/// account's own playlists). `None` while signed out or when there are none.
pub fn header_actions(
    app: &MusicApp,
    header: &Header,
    cx: &mut Context<MusicApp>,
) -> Option<AnyElement> {
    if !app.account.signed_in() {
        return None;
    }
    let c = theme::colors(cx);
    let marks = app.account.state.marks.clone();
    let title = header.title.clone();
    let library = header.library.as_ref().map(|library| {
        let saved = marks.saved(library);
        let (playlist_id, title) = (library.playlist_id.clone(), title.clone());
        let (label, icon) = if saved {
            ("Saved to library", IconName::Check)
        } else {
            ("Save to library", IconName::Plus)
        };
        widgets::toggle_pill("header-library", label, icon, saved, &c)
            .when(saved, |p| {
                p.tooltip(widgets::tooltip("Remove from library"))
            })
            .on_click(cx.listener(move |this, _, _, cx| {
                let action = AccountAction::Save {
                    playlist_id: playlist_id.clone(),
                    title: title.clone(),
                    save: !saved,
                };
                this.account_act(action, cx);
            }))
    });
    let subscribe = header.subscription.as_ref().map(|subscription| {
        let subscribed = marks.subscribed(subscription);
        let (channel_id, name) = (subscription.channel_id.clone(), title.clone());
        let (label, icon) = if subscribed {
            ("Subscribed", IconName::Check)
        } else {
            ("Subscribe", IconName::Bell)
        };
        widgets::toggle_pill("header-subscribe", label, icon, subscribed, &c)
            .when(subscribed, |p| p.tooltip(widgets::tooltip("Unsubscribe")))
            .on_click(cx.listener(move |this, _, _, cx| {
                let action = AccountAction::Subscribe {
                    channel_id: channel_id.clone(),
                    name: name.clone(),
                    subscribe: !subscribed,
                };
                this.account_act(action, cx);
            }))
    });
    let own = header
        .editable
        .clone()
        .map(|id| own_playlist_actions(id, header, &c, cx));
    if library.is_none() && subscribe.is_none() && own.is_none() {
        return None;
    }
    Some(
        h_flex()
            .gap(space::SM)
            .children(library)
            .children(subscribe)
            .children(own)
            .into_any_element(),
    )
}

/// Edit playlist and Delete playlist, for the account's own playlist.
fn own_playlist_actions(
    playlist_id: String,
    header: &Header,
    c: &Colors,
    cx: &mut Context<MusicApp>,
) -> impl IntoElement {
    let edit = Dialog::EditPlaylist {
        playlist_id: playlist_id.clone(),
        title: header.title.clone(),
        description: header.description.clone().unwrap_or_default(),
    };
    let delete = Dialog::DeletePlaylist {
        playlist_id,
        title: header.title.clone(),
    };
    h_flex()
        .gap(space::SM)
        .child(
            widgets::pill_button(
                "header-edit",
                "Edit playlist",
                Some(widgets::icon(IconName::Pencil, size::ICON_SM, c.text)),
                Pill::Secondary,
                c,
            )
            .on_click(cx.listener(move |this, _, window, cx| {
                this.open_account_dialog(edit.clone(), window, cx)
            })),
        )
        .child(
            widgets::pill_button(
                "header-delete",
                "Delete playlist",
                Some(widgets::icon(IconName::Trash, size::ICON_SM, c.text)),
                Pill::Secondary,
                c,
            )
            .on_click(cx.listener(move |this, _, window, cx| {
                this.open_account_dialog(delete.clone(), window, cx)
            })),
        )
}

/// Library → Playlists: New playlist. `None` while signed out.
pub fn library_actions(app: &MusicApp, cx: &mut Context<MusicApp>) -> Option<AnyElement> {
    if !app.account.signed_in() {
        return None;
    }
    let c = theme::colors(cx);
    Some(
        widgets::pill_button(
            "new-playlist",
            "New playlist",
            Some(widgets::icon(IconName::Plus, size::ICON_SM, c.text)),
            Pill::Secondary,
            &c,
        )
        .on_click(cx.listener(|this, _, window, cx| {
            let dialog = Dialog::NewPlaylist {
                title: String::new(),
                description: String::new(),
                tracks: Vec::new(),
            };
            this.open_account_dialog(dialog, window, cx);
        }))
        .into_any_element(),
    )
}

/// Up next's Save: the queue as a new playlist. `None` while signed out or
/// with nothing queued.
pub fn save_queue_button(
    app: &MusicApp,
    c: &Colors,
    cx: &mut Context<MusicApp>,
) -> Option<Stateful<Div>> {
    if !app.account.signed_in() || app.player.queue.is_empty() {
        return None;
    }
    Some(
        widgets::pill_button(
            "save-queue",
            "Save",
            Some(widgets::icon(IconName::ListPlus, size::ICON_SM, c.text)),
            Pill::Secondary,
            c,
        )
        .tooltip(widgets::tooltip("Save the queue as a playlist"))
        .on_click(cx.listener(|this, _, window, cx| this.save_queue(window, cx))),
    )
}
