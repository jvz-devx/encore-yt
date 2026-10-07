//! The song row (DESIGN.md "Song row"): thumb or track number, title,
//! linked subtitle, the like mark (M25), duration.

use gpui_kit::assets::IconName;
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use ytfast::model::{Item, ItemKind, Shelf};

use super::item_keys::Anchor;
use super::runs::runs_line;
use super::{Ctx, on_activate};
use crate::app::MusicApp;
use crate::assets::Glyph;
use crate::theme::{Colors, Type, radius, size, space};
use crate::views::{account, clock, menu, widgets};

/// How a list numbers its rows.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Album {
    /// An album: every row has a track number, shown in the thumb's place.
    Numbered,
    /// Anything else: thumbs, and a rank before them where there is one.
    Other,
}

impl Album {
    pub fn of(shelf: &Shelf) -> Self {
        let numbered = !shelf.items.is_empty() && shelf.items.iter().all(|i| i.index.is_some());
        let thumbs = shelf.items.iter().any(|i| i.thumbnail.is_some());
        if numbered && !thumbs {
            Album::Numbered
        } else {
            Album::Other
        }
    }
}

pub fn row(
    shelf: usize,
    i: usize,
    item: &Item,
    album: Album,
    ctx: &Ctx,
    cx: &mut Context<MusicApp>,
) -> AnyElement {
    let c = &ctx.c;
    let playing = matches!(
        (&item.track, &ctx.playing),
        (Some(t), Some(id)) if t.video_id == *id
    );
    let duration = item
        .track
        .as_ref()
        .and_then(|t| t.duration)
        .map(|d| clock(f64::from(d)));
    let lead = match (album, &item.thumbnail) {
        (Album::Numbered, _) | (_, None) => number_slot(item, playing, c),
        (Album::Other, Some(url)) => thumb(url, item.kind == ItemKind::Artist, playing, c),
    };
    let title_color = if playing { c.signal } else { c.text };
    h_flex()
        .id(ctx.id(format!("row:{shelf}:{i}")))
        .debug_selector(|| ctx.id(format!("row:{shelf}:{i}")).to_string())
        .group("row")
        .w_full()
        .h(size::ROW)
        .px(space::SM)
        .gap(space::MD)
        .rounded(radius::MD)
        .cursor_pointer()
        .when(playing, |s| s.bg(c.selected))
        .hover(|s| s.bg(c.hover))
        .active(|s| s.bg(c.pressed))
        // A chart's rank before the thumb.
        .when(album == Album::Other && item.thumbnail.is_some(), |r| {
            r.children(item.index.clone().map(|n| rank(n, playing, c)))
        })
        .child(lead)
        .child(
            v_flex()
                .flex_1()
                .min_w_0()
                .child(
                    div()
                        .truncate()
                        .type_label()
                        .text_color(title_color)
                        .child(item.title.clone()),
                )
                .child(div().w_full().truncate().type_small().child(runs_line(
                    ctx.id(format!("row-sub:{shelf}:{i}")),
                    &item.subtitle,
                    c.text_muted,
                    ctx.link.as_ref(),
                    c,
                    cx,
                ))),
        )
        .when(ctx.likes.shown(), |r| {
            r.child(like_slot(shelf, i, item, ctx, cx))
        })
        .map(|r| {
            if !menu::has_menu(item) {
                return r.children(duration.map(|d| {
                    div()
                        .flex_none()
                        .pl(space::SM)
                        .type_small()
                        .tabular()
                        .text_color(c.text_faint)
                        .child(d)
                }));
            }
            // The length gives way to ⋮ under the pointer; right-click too.
            let dots = menu::on_item_dots(&ctx.key, shelf, i, cx);
            r.child(menu::row_trailing(
                ctx.id(format!("row-menu:{shelf}:{i}")),
                duration,
                "row",
                dots,
                c,
            ))
            .on_mouse_down(
                MouseButton::Right,
                menu::on_item_right_click(&ctx.key, shelf, i, cx),
            )
        })
        .on_click(on_activate(ctx, shelf, i, cx))
        .map(|el| super::intent::page(el, item, cx))
        .map(|el| crate::views::extras::audition::hook(el, item.track.as_ref(), radius::MD, cx))
        .map(|el| super::item_keys::hook(el, &ctx.key, shelf, i, Anchor::Row, c, cx))
        .into_any_element()
}

/// The like mark of a song's row; the same room left empty in other rows.
fn like_slot(
    shelf: usize,
    i: usize,
    item: &Item,
    ctx: &Ctx,
    cx: &mut Context<MusicApp>,
) -> AnyElement {
    match &item.track {
        Some(track) => account::row_like(
            ctx.id(format!("row-like:{shelf}:{i}")),
            track,
            &ctx.likes,
            "row",
            &ctx.c,
            cx,
        ),
        None => account::row_like_space(),
    }
}

/// The 40 px thumb, with a play glyph on hover and the live mark while it
/// plays.
fn thumb(url: &str, round: bool, playing: bool, c: &Colors) -> AnyElement {
    let corner = if round { radius::FULL } else { radius::XS };
    widgets::cover(Some(url.to_string().into()), size::ROW_THUMB, round, c)
        .child(
            h_flex()
                .absolute()
                .inset_0()
                .justify_center()
                .rounded(corner)
                .bg(c.scrim)
                .when(!playing, |s| {
                    s.opacity(0.)
                        .group_hover("row", |s| s.opacity(1.))
                        .child(widgets::glyph(Glyph::Play, size::ICON_SM, c.on_media))
                })
                .when(playing, |s| {
                    s.child(widgets::icon(IconName::AudioLines, size::ICON, c.on_media))
                }),
        )
        .into_any_element()
}

/// An album's track number in the thumb's place; a play glyph on hover and
/// the live mark while it plays.
fn number_slot(item: &Item, playing: bool, c: &Colors) -> AnyElement {
    let number = item.index.clone().unwrap_or_default();
    h_flex()
        .relative()
        .flex_none()
        .size(size::ROW_THUMB)
        .justify_center()
        .when(playing, |s| {
            s.child(widgets::icon(IconName::AudioLines, size::ICON, c.signal))
        })
        .when(!playing, |s| {
            s.child(
                div()
                    .type_label()
                    .tabular()
                    .text_color(c.text_faint)
                    .group_hover("row", |s| s.opacity(0.))
                    .child(number),
            )
            .child(
                h_flex()
                    .absolute()
                    .inset_0()
                    .justify_center()
                    .opacity(0.)
                    .group_hover("row", |s| s.opacity(1.))
                    .child(widgets::glyph(Glyph::Play, size::ICON_SM, c.text)),
            )
        })
        .into_any_element()
}

/// A chart position before the thumb.
fn rank(n: String, playing: bool, c: &Colors) -> impl IntoElement {
    div()
        .w(px(24.))
        .flex_none()
        .text_right()
        .type_label()
        .tabular()
        .text_color(if playing { c.signal } else { c.text_faint })
        .child(n)
}
