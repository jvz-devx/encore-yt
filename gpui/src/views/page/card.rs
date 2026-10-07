//! The cover card (DESIGN.md "Cover card") and the round play button that
//! rises over covers.

use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use ytfast::model::{Item, ItemKind};

use super::item_keys::Anchor;
use super::runs::runs_line;
use super::{Ctx, on_activate, on_play};
use crate::app::MusicApp;
use crate::assets::Glyph;
use crate::theme::{Colors, Type, elevation, radius, size, space};
use crate::views::{menu, widgets};

pub fn card(
    ctx: &Ctx,
    shelf: usize,
    i: usize,
    item: &Item,
    cx: &mut Context<MusicApp>,
) -> AnyElement {
    let c = &ctx.c;
    let round = item.kind == ItemKind::Artist;
    let playable = item.play.is_some() || item.track.is_some();
    let cover = widgets::cover(item.thumbnail.clone().map(Into::into), size::CARD, round, c)
        .child(
            // Hover: the cover dims and a play button appears in the corner.
            div()
                .absolute()
                .inset_0()
                .rounded(if round { radius::FULL } else { radius::MD })
                .bg(c.scrim)
                .opacity(0.)
                .group_hover("card", |s| s.opacity(1.)),
        )
        .children(crate::views::extras::audition::ring(
            item.track.as_ref(),
            if round { radius::FULL } else { radius::MD },
            cx,
        ))
        .when(menu::has_menu(item), |cover| {
            cover.child(
                menu::cover_dots(ctx.id(format!("card-menu:{shelf}:{i}")), c)
                    .absolute()
                    .top(space::SM)
                    .right(space::SM)
                    .opacity(0.)
                    .group_hover("card", |s| s.opacity(1.))
                    .on_click(menu::on_item_dots(&ctx.key, shelf, i, cx)),
            )
        })
        .when(playable, |cover| {
            let centre = (size::CARD - size::PLAY_BUTTON) / 2.;
            let button = play_button(ctx.id(format!("card-play:{shelf}:{i}")), c);
            cover.child(
                super::intent::play(button, item, cx)
                    .absolute()
                    .when(round, |b| b.top(centre).left(centre))
                    .when(!round, |b| b.right(space::SM).bottom(space::SM))
                    .opacity(0.)
                    .group_hover("card", |s| s.opacity(1.))
                    .on_click(on_play(ctx, shelf, i, cx)),
            )
        });
    let subtitle = (!item.subtitle.is_empty()).then(|| {
        div()
            .mt(space::XXS)
            .w_full()
            .truncate()
            .type_small()
            .when(round, |s| s.text_center())
            .child(runs_line(
                ctx.id(format!("card-sub:{shelf}:{i}")),
                &item.subtitle,
                c.text_muted,
                ctx.link.as_ref(),
                c,
                cx,
            ))
    });
    v_flex()
        .id(ctx.id(format!("card:{shelf}:{i}")))
        .debug_selector(|| ctx.id(format!("card:{shelf}:{i}")).to_string())
        .group("card")
        .w(size::CARD)
        .flex_none()
        .cursor_pointer()
        .when(round, |s| s.items_center())
        .child(cover)
        .child(
            div()
                .mt(space::SM)
                .w_full()
                .truncate()
                .type_label()
                .when(round, |s| s.text_center())
                .child(item.title.clone()),
        )
        .children(subtitle)
        .when(menu::has_menu(item), |card| {
            card.on_mouse_down(
                MouseButton::Right,
                menu::on_item_right_click(&ctx.key, shelf, i, cx),
            )
        })
        .on_click(on_activate(ctx, shelf, i, cx))
        .map(|el| super::intent::page(el, item, cx))
        .map(|el| crate::views::extras::audition::listen(el, item.track.as_ref(), cx))
        .map(|el| super::item_keys::hook(el, &ctx.key, shelf, i, Anchor::Card, c, cx))
        .into_any_element()
}

/// The round play button over covers: a white disc with a dark glyph, the
/// same in both looks because it sits on the art.
pub fn play_button(id: SharedString, c: &Colors) -> Stateful<Div> {
    h_flex()
        .id(id.clone())
        .debug_selector(|| id.to_string())
        .size(size::PLAY_BUTTON)
        .justify_center()
        .rounded(radius::FULL)
        .bg(c.on_media)
        .shadow(elevation::low(c))
        // Optical centre of a triangle sits left of its box.
        .pl(px(2.))
        .cursor_pointer()
        .child(widgets::glyph(
            Glyph::Play,
            size::ICON,
            c.on_media_foreground,
        ))
}
