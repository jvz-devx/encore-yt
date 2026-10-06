//! A page: its header, chips and shelves, in one vertical scroll.
//!
//! Clicks look their item up again by (page key, shelf, item) when they
//! happen, so a frame never clones the shelves into its closures.

use gpui_kit::assets::IconName;
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use ytfast::model::{Header, Item, ItemKind, Page, Shelf, ShelfStyle};

use super::widgets::{self, Pill};
use super::{clock, runs_text};
use crate::app::MusicApp;
use crate::assets::Glyph;
use crate::theme::{self, Colors, Type, elevation, motion, radius, size, space};

/// Quick picks: rows per column.
const ROWS_PER_COLUMN: usize = 4;

/// What a frame needs to know besides the page: which song is playing.
struct Ctx<'a> {
    key: &'a str,
    playing: Option<&'a str>,
    c: Colors,
}

pub fn page(app: &MusicApp, _window: &mut Window, cx: &mut Context<MusicApp>) -> AnyElement {
    let c = theme::colors(cx);
    let target = app.pages.view.target();
    let key = target.key();
    let Some(state) = app.pages.states.get(&key) else {
        return loading(&c).into_any_element();
    };
    let Some(page) = &state.page else {
        if let Some(error) = &state.error {
            return failed(error.clone(), target, &c, cx).into_any_element();
        }
        return loading(&c).into_any_element();
    };
    let ctx = Ctx {
        key: &key,
        playing: app.player.current().map(|t| t.video_id.as_str()),
        c,
    };
    v_flex()
        .id(SharedString::from(format!("page:{key}")))
        .flex_1()
        .w_full()
        .min_h_0()
        .overflow_y_scroll()
        .px(size::GUTTER)
        .pt(space::SM)
        .pb(space::XXXL)
        .gap(space::XXL + space::SM)
        .children(page.header.as_ref().map(|h| header(h, &c, cx)))
        .children(message(page, &c))
        .children(
            page.shelves
                .iter()
                .enumerate()
                .map(|(i, shelf)| shelf_view(&ctx, i, shelf, cx)),
        )
        // A new page settles in rather than popping.
        .with_animation(
            SharedString::from(format!("enter:{key}")),
            Animation::new(motion::BASE).with_easing(motion::ease_out),
            |el, t| el.opacity(t),
        )
        .into_any_element()
}

/// Loading: the shape of a page (a title and a row of covers), pulsing.
fn loading(c: &Colors) -> impl IntoElement {
    let shelf = |n: usize| {
        v_flex()
            .gap(space::LG)
            .child(widgets::skeleton(px(200.), px(24.), radius::SM, c))
            .child(
                h_flex()
                    .gap(space::XL)
                    .items_start()
                    .children((0..n).map(|_| {
                        v_flex()
                            .gap(space::SM)
                            .child(widgets::skeleton(size::CARD, size::CARD, radius::MD, c))
                            .child(widgets::skeleton(px(140.), px(14.), radius::XS, c))
                            .child(widgets::skeleton(px(96.), px(12.), radius::XS, c))
                    })),
            )
    };
    v_flex()
        .flex_1()
        .min_h_0()
        .overflow_hidden()
        .px(size::GUTTER)
        .pt(space::SM)
        .gap(space::XXL + space::SM)
        .child(shelf(6))
        .child(shelf(6))
}

/// A page that failed to load: what happened, and Try again.
fn failed(
    error: String,
    target: ytfast::model::Target,
    c: &Colors,
    cx: &mut Context<MusicApp>,
) -> impl IntoElement {
    v_flex()
        .flex_1()
        .size_full()
        .items_center()
        .justify_center()
        .gap(space::MD)
        .px(size::GUTTER)
        .child(
            h_flex()
                .size(px(48.))
                .justify_center()
                .rounded(radius::FULL)
                .bg(c.raised)
                .child(widgets::icon(IconName::CircleAlert, px(22.), c.text_muted)),
        )
        .child(div().type_heading().child("Couldn't load this page"))
        .child(
            div()
                .max_w(px(420.))
                .text_center()
                .type_small()
                .text_color(c.text_muted)
                .child(error),
        )
        .child(
            widgets::pill_button(
                "retry",
                "Try again",
                Some(widgets::icon(IconName::RefreshCw, size::ICON_SM, c.text)),
                Pill::Secondary,
                c,
            )
            .mt(space::SM)
            .on_click(cx.listener(move |this, _, _, cx| {
                this.ensure_page(target.clone(), true);
                cx.notify();
            })),
        )
}

fn message(page: &Page, c: &Colors) -> Option<impl IntoElement> {
    let message = page.message.clone()?;
    Some(div().type_body().text_color(c.text_muted).child(message))
}

fn header(header: &Header, c: &Colors, cx: &mut Context<MusicApp>) -> impl IntoElement {
    let cover = widgets::cover(
        header.thumbnail.clone().map(Into::into),
        size::HEADER_COVER,
        header.round,
        c,
    );
    let actions = h_flex()
        .mt(space::MD)
        .gap(space::SM)
        .children(header.play.clone().map(|target| {
            widgets::pill_button(
                "header-play",
                "Play",
                Some(widgets::glyph(
                    Glyph::Play,
                    size::ICON_SM,
                    c.primary_foreground,
                )),
                Pill::Primary,
                c,
            )
            .on_click(cx.listener(move |this, _, _, cx| this.activate(target.clone(), cx)))
        }))
        .children(header.shuffle.clone().map(|target| {
            widgets::pill_button(
                "header-shuffle",
                "Shuffle",
                Some(widgets::icon(IconName::Shuffle, size::ICON_SM, c.text)),
                Pill::Secondary,
                c,
            )
            .on_click(cx.listener(move |this, _, _, cx| this.activate(target.clone(), cx)))
        }))
        .children(header.radio.clone().map(|target| {
            widgets::pill_button(
                "header-radio",
                "Radio",
                Some(widgets::icon(IconName::Radio, size::ICON_SM, c.text)),
                Pill::Secondary,
                c,
            )
            .on_click(cx.listener(move |this, _, _, cx| this.activate(target.clone(), cx)))
        }));
    h_flex()
        .pt(space::LG)
        .gap(space::XXL)
        .items_end()
        .child(cover)
        .child(
            v_flex()
                .flex_1()
                .min_w_0()
                .gap(space::XS)
                .child(
                    div()
                        .type_display()
                        .line_clamp(2)
                        .child(header.title.clone()),
                )
                .child(
                    div()
                        .mt(space::XS)
                        .type_body()
                        .text_color(c.text_muted)
                        .child(runs_text(&header.subtitle)),
                )
                .when(!header.second_subtitle.is_empty(), |col| {
                    col.child(
                        div()
                            .type_small()
                            .text_color(c.text_muted)
                            .child(header.second_subtitle.clone()),
                    )
                })
                .children(header.description.clone().map(|d| {
                    div()
                        .mt(space::SM)
                        .max_w(px(640.))
                        .type_small()
                        .text_color(c.text_muted)
                        .line_clamp(2)
                        .child(d)
                }))
                .child(actions),
        )
}

fn shelf_view(ctx: &Ctx, index: usize, shelf: &Shelf, cx: &mut Context<MusicApp>) -> AnyElement {
    let c = &ctx.c;
    let title = (!shelf.title.is_empty()).then(|| {
        v_flex()
            .gap(space::XXS)
            .children(
                shelf
                    .strapline
                    .clone()
                    .map(|s| div().type_caption().text_color(c.text_muted).child(s)),
            )
            .child(div().type_title().truncate().child(shelf.title.clone()))
    });
    let cards = |cx: &mut Context<MusicApp>| -> Vec<AnyElement> {
        shelf
            .items
            .iter()
            .enumerate()
            .map(|(i, item)| card(ctx, index, i, item, cx))
            .collect()
    };
    let body = match shelf.style {
        ShelfStyle::Carousel => h_flex()
            .id(SharedString::from(format!("carousel:{}:{index}", ctx.key)))
            .w_full()
            .gap(space::XL)
            .items_start()
            .overflow_x_scroll()
            .pb(space::SM)
            .children(cards(cx))
            .into_any_element(),
        ShelfStyle::Grid | ShelfStyle::TopResult => h_flex()
            .w_full()
            .flex_wrap()
            .gap(space::XL)
            .items_start()
            .children(cards(cx))
            .into_any_element(),
        ShelfStyle::Buttons => h_flex()
            .w_full()
            .flex_wrap()
            .gap(space::SM)
            .children(
                shelf
                    .items
                    .iter()
                    .enumerate()
                    .map(|(i, item)| chip(ctx, index, i, item, cx)),
            )
            .into_any_element(),
        ShelfStyle::RowCarousel => row_columns(ctx, index, shelf, cx),
        ShelfStyle::List => v_flex()
            .w_full()
            .children(
                shelf
                    .items
                    .iter()
                    .enumerate()
                    .map(|(i, item)| row(ctx, index, i, item, cx)),
            )
            .into_any_element(),
    };
    v_flex()
        .gap(space::LG)
        .children(title)
        .child(body)
        .into_any_element()
}

/// Quick picks: columns of four rows that scroll sideways.
fn row_columns(ctx: &Ctx, index: usize, shelf: &Shelf, cx: &mut Context<MusicApp>) -> AnyElement {
    let columns: Vec<AnyElement> = shelf
        .items
        .chunks(ROWS_PER_COLUMN)
        .enumerate()
        .map(|(col, items)| {
            v_flex()
                .w(size::ROW_COLUMN)
                .flex_none()
                .children(
                    items
                        .iter()
                        .enumerate()
                        .map(|(j, item)| row(ctx, index, col * ROWS_PER_COLUMN + j, item, cx)),
                )
                .into_any_element()
        })
        .collect();
    h_flex()
        .id(SharedString::from(format!("rows:{}:{index}", ctx.key)))
        .w_full()
        .items_start()
        .gap(space::LG)
        .overflow_x_scroll()
        .pb(space::SM)
        .children(columns)
        .into_any_element()
}

/// Finds item `item` of shelf `shelf` on page `key` when a click happens.
fn find(this: &MusicApp, key: &str, shelf: usize, item: usize) -> Option<(Item, Shelf)> {
    this.pages
        .states
        .get(key)
        .and_then(|s| s.page.as_ref())
        .and_then(|p| p.shelves.get(shelf))
        .and_then(|s| Some((s.items.get(item)?.clone(), s.clone())))
}

/// The click handler for item `item` of shelf `shelf` on page `key`.
fn on_activate(
    key: &str,
    shelf: usize,
    item: usize,
    cx: &mut Context<MusicApp>,
) -> impl Fn(&ClickEvent, &mut Window, &mut App) + 'static {
    let key = key.to_string();
    cx.listener(move |this, _: &ClickEvent, _, cx| {
        if let Some((item, shelf)) = find(this, &key, shelf, item) {
            this.activate_item(&item, &shelf, cx);
        }
    })
}

/// The cover's play button: plays the item's own play target where it has
/// one (an album, a playlist) instead of opening it.
fn on_play(
    key: &str,
    shelf: usize,
    item: usize,
    cx: &mut Context<MusicApp>,
) -> impl Fn(&ClickEvent, &mut Window, &mut App) + 'static {
    let key = key.to_string();
    cx.listener(move |this, _: &ClickEvent, _, cx| {
        cx.stop_propagation();
        let Some((item, shelf)) = find(this, &key, shelf, item) else {
            return;
        };
        match &item.play {
            Some(play) if item.track.is_none() => {
                this.send(ytfast::backend::Command::PlayTarget(play.clone()))
            }
            _ => this.activate_item(&item, &shelf, cx),
        }
    })
}

fn card(ctx: &Ctx, shelf: usize, i: usize, item: &Item, cx: &mut Context<MusicApp>) -> AnyElement {
    let c = &ctx.c;
    let round = item.kind == ItemKind::Artist;
    let playable = item.play.is_some() || item.track.is_some();
    let cover = widgets::cover(item.thumbnail.clone().map(Into::into), size::CARD, round, c)
        .child(
            // Hover: the cover dims and a play button rises from the corner.
            div()
                .absolute()
                .inset_0()
                .rounded(if round { radius::FULL } else { radius::MD })
                .bg(c.scrim)
                .opacity(0.)
                .group_hover("card", |s| s.opacity(1.)),
        )
        .when(playable, |cover| {
            cover.child(
                play_button(SharedString::from(format!("card-play:{shelf}:{i}")), c)
                    .absolute()
                    .when(round, |b| b.top(px(68.)).left(px(68.)))
                    .when(!round, |b| b.right(space::SM).bottom(space::SM))
                    .opacity(0.)
                    .group_hover("card", |s| s.opacity(1.))
                    .on_click(on_play(ctx.key, shelf, i, cx)),
            )
        });
    let subtitle = runs_text(&item.subtitle);
    v_flex()
        .id(SharedString::from(format!("card:{shelf}:{i}")))
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
        .when(!subtitle.is_empty(), |card| {
            card.child(
                widgets::muted_line(subtitle, c)
                    .mt(space::XXS)
                    .when(round, |s| s.text_center()),
            )
        })
        .on_click(on_activate(ctx.key, shelf, i, cx))
        .into_any_element()
}

/// The round play button over covers: a white disc with a dark glyph, the
/// same in both looks because it sits on the art.
fn play_button(id: SharedString, c: &Colors) -> Stateful<Div> {
    h_flex()
        .id(id)
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

fn row(ctx: &Ctx, shelf: usize, i: usize, item: &Item, cx: &mut Context<MusicApp>) -> AnyElement {
    let c = &ctx.c;
    let playing = matches!(
        (&item.track, ctx.playing),
        (Some(t), Some(id)) if t.video_id == id
    );
    let duration = item
        .track
        .as_ref()
        .and_then(|t| t.duration)
        .map(|d| clock(f64::from(d)));
    let thumb = item.thumbnail.clone().map(|url| {
        widgets::cover(Some(url.into()), size::ROW_THUMB, false, c).child(
            h_flex()
                .absolute()
                .inset_0()
                .justify_center()
                .rounded(radius::XS)
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
    });
    let title_color = if playing { c.signal } else { c.text };
    h_flex()
        .id(SharedString::from(format!("row:{shelf}:{i}")))
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
        .children(item.index.clone().map(|n| {
            div()
                .w(px(24.))
                .flex_none()
                .text_right()
                .type_label()
                .tabular()
                .text_color(if playing { c.signal } else { c.text_faint })
                .child(n)
        }))
        .children(thumb)
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
                .child(widgets::muted_line(runs_text(&item.subtitle), c)),
        )
        .children(duration.map(|d| {
            div()
                .flex_none()
                .type_small()
                .tabular()
                .text_color(c.text_faint)
                .child(d)
        }))
        .on_click(on_activate(ctx.key, shelf, i, cx))
        .into_any_element()
}

/// A mood or genre: a pill, led by YouTube Music's own colour for it.
fn chip(ctx: &Ctx, shelf: usize, i: usize, item: &Item, cx: &mut Context<MusicApp>) -> AnyElement {
    let c = &ctx.c;
    h_flex()
        .id(SharedString::from(format!("chip:{shelf}:{i}")))
        .h(size::CHIP)
        .px(space::LG)
        .gap(space::SM)
        .rounded(radius::FULL)
        .bg(c.raised)
        .type_label()
        .cursor_pointer()
        .hover(|s| s.bg(c.overlay))
        .active(|s| s.bg(c.pressed))
        .children(
            item.stripe
                .map(|stripe| div().size(px(8.)).rounded(radius::FULL).bg(rgb(stripe))),
        )
        .child(item.title.clone())
        .on_click(on_activate(ctx.key, shelf, i, cx))
        .into_any_element()
}
