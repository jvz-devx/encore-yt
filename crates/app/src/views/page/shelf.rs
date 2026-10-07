//! A shelf: its title row (strapline, title, See all) and its body in each
//! `ShelfStyle`: carousels of cards and of Quick picks columns (with arrow
//! buttons on hover), grids, mood tiles and the search top result. Song
//! lists are drawn row by row by the page's list (`row`).

use std::time::{Duration, Instant};

use encore_core::model::{Item, ItemKind, Shelf, ShelfStyle};
use gpui_kit::assets::IconName;
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::card::card;
use super::row::{Album, row};
use super::runs::runs_line;
use super::{Ctx, on_activate, on_play};
use crate::app::MusicApp;
use crate::assets::Glyph;
use crate::theme::{Colors, Type, elevation, motion, radius, size, space};
use crate::views::widgets::{self, Pill};

/// Quick picks: rows per column.
const ROWS_PER_COLUMN: usize = 4;

pub fn shelf(
    index: usize,
    shelf: &Shelf,
    scroll: ScrollHandle,
    ctx: &Ctx,
    cx: &mut Context<MusicApp>,
) -> AnyElement {
    let body = match shelf.style {
        ShelfStyle::Carousel => {
            let cards = shelf
                .items
                .iter()
                .enumerate()
                .map(|(i, item)| card(ctx, index, i, item, cx))
                .collect();
            carousel(index, cards, size::CARD, space::XL, scroll, ctx, cx)
        }
        ShelfStyle::RowCarousel => {
            let columns = row_columns(index, shelf, ctx, cx);
            carousel(index, columns, size::ROW_COLUMN, space::LG, scroll, ctx, cx)
        }
        ShelfStyle::Grid => h_flex()
            .w_full()
            .flex_wrap()
            .items_start()
            .gap(space::XL)
            .children(
                shelf
                    .items
                    .iter()
                    .enumerate()
                    .map(|(i, item)| card(ctx, index, i, item, cx)),
            )
            .into_any_element(),
        ShelfStyle::Buttons => h_flex()
            .w_full()
            .flex_wrap()
            .gap(space::MD)
            .children(
                shelf
                    .items
                    .iter()
                    .enumerate()
                    .map(|(i, item)| tile(ctx, index, i, item, cx)),
            )
            .into_any_element(),
        ShelfStyle::TopResult => match shelf.items.first() {
            Some(item) => top_result(ctx, index, item, cx),
            None => div().into_any_element(),
        },
        // Drawn row by row by the page.
        ShelfStyle::List => v_flex()
            .children(
                shelf
                    .items
                    .iter()
                    .enumerate()
                    .map(|(i, item)| row(index, i, item, Album::of(shelf), ctx, cx)),
            )
            .into_any_element(),
    };
    let has_title = !shelf.title.is_empty() || shelf.more.is_some();
    v_flex()
        .w_full()
        .gap(space::LG)
        .when(has_title, |s| s.child(title(index, shelf, ctx, cx)))
        .child(body)
        .into_any_element()
}

/// Whether a shelf of `style` scrolls sideways in a carousel.
pub fn scrolls(style: ShelfStyle) -> bool {
    matches!(style, ShelfStyle::Carousel | ShelfStyle::RowCarousel)
}

/// Strapline and title, and See all on the right when the shelf has a page.
pub fn title(index: usize, shelf: &Shelf, ctx: &Ctx, cx: &mut Context<MusicApp>) -> AnyElement {
    let c = &ctx.c;
    let see_all = shelf.more.clone().map(|target| {
        widgets::pill_button(
            ctx.id(format!("see-all:{index}")),
            "See all",
            None,
            Pill::Secondary,
            c,
        )
        .on_click(cx.listener(move |this, _, _, cx| this.activate(target.clone(), cx)))
    });
    let list = shelf.style == ShelfStyle::List;
    h_flex()
        .w_full()
        .items_end()
        .gap(space::LG)
        // A song list's title sits apart from its rows like a shelf title
        // from its cards.
        .when(list, |s| s.pb(space::LG))
        .child(
            v_flex()
                .flex_1()
                .min_w_0()
                .gap(space::XXS)
                .children(
                    shelf
                        .strapline
                        .clone()
                        .map(|s| div().type_caption().text_color(c.text_muted).child(s)),
                )
                .child(
                    div()
                        .debug_selector(|| format!("shelf:{}", shelf.title))
                        .type_title()
                        .truncate()
                        .child(shelf.title.clone()),
                ),
        )
        .children(see_all)
        .into_any_element()
}

/// A horizontal strip that scrolls sideways (wheel, touchpad, or the arrow
/// buttons that show while the pointer is over it). `pitch` is one item's
/// width; the arrows glide by whole items.
fn carousel(
    index: usize,
    items: Vec<AnyElement>,
    item_width: Pixels,
    gap: Pixels,
    scroll: ScrollHandle,
    ctx: &Ctx,
    cx: &mut Context<MusicApp>,
) -> AnyElement {
    let c = &ctx.c;
    let offset = -scroll.offset().x;
    let max = scroll.max_offset().x;
    let can_back = offset > px(1.);
    let can_forward = max - offset > px(1.);
    let pitch = item_width + gap;
    // The arrows sit level with the middle of the covers (or the rows).
    let arrow_top = if item_width == size::CARD {
        (size::CARD - size::ICON_BUTTON) / 2.
    } else {
        (size::ROW * ROWS_PER_COLUMN as f32 - size::ICON_BUTTON) / 2.
    };
    let arrow = |direction: f32, icon: IconName, cx: &mut Context<MusicApp>| {
        let scroll = scroll.clone();
        let key = ctx.key.clone();
        arrow_button(
            ctx.id(format!("carousel-arrow:{index}:{direction}")),
            icon,
            c,
        )
        .absolute()
        .top(arrow_top)
        .opacity(0.)
        .group_hover("carousel", |s| s.opacity(1.))
        .on_click(cx.listener(move |this, _, _, cx| {
            cx.stop_propagation();
            let to = page_turn(&scroll, direction, pitch);
            glide(this, (key.clone(), index), scroll.clone(), to, cx);
        }))
    };
    let back = can_back.then(|| arrow(-1., IconName::ChevronLeft, cx).left(-space::LG));
    let forward = can_forward.then(|| arrow(1., IconName::ChevronRight, cx).right(-space::LG));
    div()
        .relative()
        .w_full()
        .group("carousel")
        // Room inside the clip for a focused card's ring (M29), taken back
        // outside so nothing moves.
        .child(
            div().m(-space::SM).child(
                h_flex()
                    .id(ctx.id(format!("carousel:{index}")))
                    .w_full()
                    .items_start()
                    .gap(gap)
                    .p(space::SM)
                    .overflow_x_scroll()
                    .restrict_scroll_to_axis()
                    .track_scroll(&scroll)
                    .children(items),
            ),
        )
        .children(back)
        .children(forward)
        .into_any_element()
}

/// A round button over the edge of a carousel.
fn arrow_button(id: SharedString, icon: IconName, c: &Colors) -> Stateful<Div> {
    h_flex()
        .id(id)
        .size(size::ICON_BUTTON)
        .justify_center()
        .rounded(radius::FULL)
        .bg(c.overlay)
        .shadow(elevation::low(c))
        .cursor_pointer()
        .hover(|s| s.bg(c.raised))
        .active(|s| s.opacity(0.9))
        .child(widgets::icon(icon, size::ICON, c.text))
}

/// Where a page turn of a carousel lands: most of its visible width on,
/// settling on an item's edge.
fn page_turn(scroll: &ScrollHandle, direction: f32, pitch: Pixels) -> Pixels {
    let width = scroll.bounds().size.width;
    let from = -scroll.offset().x;
    let items = ((width / pitch).floor() - 1.).max(1.);
    ((from / pitch).round() + direction * items) * pitch
}

/// Scrolls carousel `shelf` (page key, shelf) to `to`, easing out over
/// `motion::SLOW` at the chosen speed, or at once with reduced motion. A
/// glide already under way stops.
pub fn glide(
    app: &mut MusicApp,
    shelf: (String, usize),
    scroll: ScrollHandle,
    to: Pixels,
    cx: &mut Context<MusicApp>,
) {
    let from = -scroll.offset().x;
    let to = to.clamp(px(0.), scroll.max_offset().x);
    let Some(length) = motion::duration(motion::SLOW) else {
        app.pages.glides.remove(&shelf);
        scroll.set_offset(point(-to, scroll.offset().y));
        cx.notify();
        return;
    };
    let task = cx.spawn(async move |this, cx| {
        let start = Instant::now();
        loop {
            cx.background_executor()
                .timer(Duration::from_millis(8))
                .await;
            let t = (start.elapsed().as_secs_f32() / length.as_secs_f32()).min(1.);
            let x = from + (to - from) * motion::ease_out(t);
            let y = scroll.offset().y;
            scroll.set_offset(point(-x, y));
            if this.update(cx, |_, cx| cx.notify()).is_err() || t >= 1. {
                break;
            }
        }
    });
    app.pages.glides.insert(shelf, task);
}

/// Quick picks: columns of four rows.
fn row_columns(
    index: usize,
    shelf: &Shelf,
    ctx: &Ctx,
    cx: &mut Context<MusicApp>,
) -> Vec<AnyElement> {
    shelf
        .items
        .chunks(ROWS_PER_COLUMN)
        .enumerate()
        .map(|(col, items)| {
            v_flex()
                .w(size::ROW_COLUMN)
                .flex_none()
                .children(items.iter().enumerate().map(|(j, item)| {
                    row(
                        index,
                        col * ROWS_PER_COLUMN + j,
                        item,
                        Album::Other,
                        ctx,
                        cx,
                    )
                }))
                .into_any_element()
        })
        .collect()
}

/// A mood or genre: a tile led by YouTube Music's own colour for it.
fn tile(ctx: &Ctx, shelf: usize, i: usize, item: &Item, cx: &mut Context<MusicApp>) -> AnyElement {
    let c = &ctx.c;
    h_flex()
        .id(ctx.id(format!("tile:{shelf}:{i}")))
        .w(size::CARD)
        .h(size::ROW)
        .flex_none()
        .pr(space::MD)
        .gap(space::MD)
        .rounded(radius::MD)
        .overflow_hidden()
        .bg(c.raised)
        .type_label()
        .cursor_pointer()
        .hover(|s| s.bg(c.overlay))
        .active(|s| s.bg(c.pressed))
        .child(
            div()
                .w(space::XS)
                .h_full()
                .flex_none()
                // Content colour: YouTube Music's own for this mood.
                .when_some(item.stripe, |s, stripe| s.bg(rgb(stripe)))
                .when(item.stripe.is_none(), |s| s.bg(c.hairline)),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .child(item.title.clone()),
        )
        .on_click(on_activate(ctx, shelf, i, cx))
        .map(|el| super::intent::page(el, item, cx))
        .into_any_element()
}

/// The search's top result: one large card with its cover, what it is, and
/// Play.
fn top_result(ctx: &Ctx, shelf: usize, item: &Item, cx: &mut Context<MusicApp>) -> AnyElement {
    let c = &ctx.c;
    let round = item.kind == ItemKind::Artist;
    let playable = item.play.is_some() || item.track.is_some();
    let play = playable.then(|| {
        widgets::pill_button(
            ctx.id("top-play"),
            "Play",
            Some(widgets::glyph(
                Glyph::Play,
                size::ICON_SM,
                c.primary_foreground,
            )),
            Pill::Primary,
            c,
        )
        .on_click(on_play(ctx, shelf, 0, cx))
        .map(|el| super::intent::play(el, item, cx))
    });
    h_flex()
        .id(ctx.id("top-result"))
        .max_w(px(720.))
        .p(space::LG)
        .gap(space::XL)
        // Concentric with the cover inside it.
        .rounded(radius::MD + space::LG)
        .bg(c.raised)
        .cursor_pointer()
        .hover(|s| s.bg(c.overlay))
        .child(widgets::cover(
            item.thumbnail.clone().map(Into::into),
            size::CARD,
            round,
            c,
        ))
        .child(
            v_flex()
                .flex_1()
                .min_w_0()
                .gap(space::XS)
                .child(div().type_title().line_clamp(2).child(item.title.clone()))
                .child(div().w_full().truncate().type_body().child(runs_line(
                    ctx.id("top-sub"),
                    &item.subtitle,
                    c.text_muted,
                    ctx.link.as_ref(),
                    c,
                    cx,
                )))
                .children(play.map(|p| h_flex().mt(space::MD).child(p))),
        )
        .on_click(on_activate(ctx, shelf, 0, cx))
        .map(|el| super::intent::page(el, item, cx))
        .map(|el| crate::views::extras::audition::listen(el, item.track.as_ref(), cx))
        .into_any_element()
}
