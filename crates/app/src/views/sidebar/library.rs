//! The signed-in library: New playlist, Liked music and the account's
//! playlists (Library → Playlists), then Recently played.

use gpui_kit::assets::IconName;
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use ytfast::model::{Item, ItemKind, Target};

use super::super::{runs_text, widgets};
use crate::app::MusicApp;
use crate::nav::View;
use crate::sidebar::{LIKED, playlist_id};
use crate::theme::{Colors, Type, radius, size, space};

pub fn library(app: &MusicApp, rail: bool, c: &Colors, cx: &mut Context<MusicApp>) -> Div {
    let playlists: Vec<AnyElement> = match app.library_playlists() {
        Some(items) => items
            .into_iter()
            .enumerate()
            .map(|(i, item)| row(("playlist", i), &item, app, rail, c, cx).into_any_element())
            .collect(),
        None => (0..3)
            .map(|_| loading_row(rail, c).into_any_element())
            .collect(),
    };
    v_flex()
        .gap(space::XXS)
        .child(new_playlist(rail, c, cx))
        .child(div().h(space::SM))
        .children(playlists)
        .child(recent(app, rail, c, cx))
}

/// Recently played, under its heading; nothing until something was played.
pub fn recent(app: &MusicApp, rail: bool, c: &Colors, cx: &mut Context<MusicApp>) -> Div {
    let rows: Vec<AnyElement> = app
        .sidebar
        .recent
        .iter()
        .enumerate()
        .map(|(i, item)| row(("recent", i), item, app, rail, c, cx).into_any_element())
        .collect();
    v_flex()
        .gap(space::XXS)
        .when(!rows.is_empty(), |s| {
            s.mt(space::XL).child(heading("Recently played", rail, c))
        })
        .children(rows)
}

/// A section's name, or on the rail a short hairline between the groups.
pub fn heading(text: &'static str, rail: bool, c: &Colors) -> impl IntoElement {
    if rail {
        return h_flex()
            .h(space::LG)
            .justify_center()
            .child(div().w(space::XL).h(px(1.)).bg(c.hairline));
    }
    h_flex()
        .h(space::XL)
        .px(space::SM)
        .type_caption()
        .text_color(c.text_muted)
        .child(text)
}

/// New playlist, at the top of the playlists as in YouTube Music: a pill
/// the width of the sidebar, or a round button on the rail.
fn new_playlist(rail: bool, c: &Colors, cx: &mut Context<MusicApp>) -> impl IntoElement {
    let (hover, pressed) = (c.overlay, c.pressed);
    h_flex()
        .id("sidebar-new-playlist")
        .flex_none()
        .h(size::CHIP)
        .gap(space::SM)
        .rounded(radius::FULL)
        .bg(c.raised)
        .type_label()
        .text_color(c.text)
        .cursor_pointer()
        .hover(move |s| s.bg(hover))
        .active(move |s| s.bg(pressed))
        .map(|s| {
            if rail {
                s.w(size::CHIP).mx_auto().justify_center()
            } else {
                s.px(space::MD)
            }
        })
        .child(widgets::icon(IconName::Plus, size::ICON, c.text))
        .when(!rail, |s| s.child("New playlist"))
        .when(rail, |s| s.tooltip(widgets::tooltip("New playlist")))
        .on_click(cx.listener(|this, _, window, cx| this.sidebar_new_playlist(window, cx)))
}

/// A playlist, album or radio: cover, title and subtitle, the open page
/// highlighted and the one playing marked. It opens its page, or plays a
/// radio again.
fn row(
    id: (&'static str, usize),
    item: &Item,
    app: &MusicApp,
    rail: bool,
    c: &Colors,
    cx: &mut Context<MusicApp>,
) -> impl IntoElement {
    let open = is_open(&app.pages.view, item.target.as_ref());
    let playing = app.player.playback.playing
        && app.sidebar.playing_from.as_deref() == item.play.as_ref().and_then(playlist_id);
    let (hover, pressed) = (c.hover, c.pressed);
    let item_for_click = item.clone();
    h_flex()
        .id(id)
        .flex_none()
        .h(size::LIBRARY_ROW)
        .gap(space::MD)
        // Concentric with the cover: its radius plus the inset around it.
        .rounded(radius::XS + space::SM)
        .cursor_pointer()
        .map(|s| {
            if rail {
                s.justify_center()
            } else {
                s.px(space::SM)
            }
        })
        .when(open, |s| s.bg(c.selected))
        .when(!open, |s| s.hover(move |s| s.bg(hover)))
        .active(move |s| s.bg(pressed))
        .child(cover(item, c))
        .when(!rail, |s| {
            s.child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .child(
                        div()
                            .w_full()
                            .truncate()
                            .type_label()
                            .text_color(c.text)
                            .child(item.title.clone()),
                    )
                    .children(
                        (!item.subtitle.is_empty())
                            .then(|| widgets::muted_line(runs_text(&item.subtitle), c)),
                    ),
            )
            .when(playing, |s| {
                s.child(widgets::icon(IconName::AudioLines, size::ICON_SM, c.signal))
            })
        })
        .when(rail, |s| s.tooltip(widgets::tooltip(item.title.clone())))
        .on_click(cx.listener(move |this, _, _, cx| this.choose_library_item(&item_for_click, cx)))
        .map(|el| crate::views::page::intent::page(el, item, cx))
}

/// The item's cover; Liked music without one (it wasn't on any page yet)
/// gets a heart on the placeholder's fill.
fn cover(item: &Item, c: &Colors) -> AnyElement {
    let liked = matches!(&item.target, Some(Target::Browse { id, .. }) if id == LIKED);
    if liked && item.thumbnail.is_none() {
        return h_flex()
            .flex_none()
            .size(size::LIBRARY_THUMB)
            .justify_center()
            .rounded(radius::XS)
            .bg(c.raised)
            .child(widgets::icon(IconName::Heart, size::ICON_SM, c.text_muted))
            .into_any_element();
    }
    widgets::cover(
        item.thumbnail.clone().map(Into::into),
        size::LIBRARY_THUMB,
        item.kind == ItemKind::Artist,
        c,
    )
    .into_any_element()
}

/// The open page is this item's (by browse id: the same playlist comes
/// with different params from different shelves).
fn is_open(view: &View, target: Option<&Target>) -> bool {
    match (view, target) {
        (View::Page(Target::Browse { id: open, .. }), Some(Target::Browse { id, .. })) => {
            open == id
        }
        _ => false,
    }
}

/// A row's shape while Library → Playlists loads for the first time.
fn loading_row(rail: bool, c: &Colors) -> impl IntoElement {
    h_flex()
        .h(size::LIBRARY_ROW)
        .gap(space::MD)
        .map(|s| {
            if rail {
                s.justify_center()
            } else {
                s.px(space::SM)
            }
        })
        .child(widgets::skeleton(
            size::LIBRARY_THUMB,
            size::LIBRARY_THUMB,
            radius::XS,
            c,
        ))
        .when(!rail, |s| {
            s.child(
                v_flex()
                    .gap(space::XS)
                    .child(widgets::skeleton(px(112.), px(12.), radius::XS, c))
                    .child(widgets::skeleton(px(72.), px(10.), radius::XS, c)),
            )
        })
}
