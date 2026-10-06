//! A page: its header, chips and shelves, in one vertical scroll.
//!
//! Clicks look their item up again by (page key, shelf, item) when they
//! happen, so a frame never clones the shelves into its closures.

use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::{ActiveTheme, StyledExt, h_flex, v_flex};
use gpui_kit::*;
use ytfast::model::{Header, Item, ItemKind, Page, Shelf, ShelfStyle};

use super::{clock, runs_text};
use crate::app::MusicApp;

const CARD: f32 = 168.;
const ROW_THUMB: f32 = 44.;

pub fn page(app: &MusicApp, _window: &mut Window, cx: &mut Context<MusicApp>) -> AnyElement {
    let key = app.pages.view.target().key();
    let Some(state) = app.pages.states.get(&key) else {
        return centered(Spinner::new().into_any_element());
    };
    let Some(page) = &state.page else {
        if let Some(error) = &state.error {
            return centered(div().child(error.clone()).into_any_element());
        }
        return centered(Spinner::new().into_any_element());
    };
    v_flex()
        .id(SharedString::from(format!("page:{key}")))
        .flex_1()
        .w_full()
        .min_h_0()
        .overflow_y_scroll()
        .px_6()
        .pb_6()
        .gap_6()
        .children(page.header.as_ref().map(|h| header(h, cx)))
        .children(message(page, cx))
        .children(
            page.shelves
                .iter()
                .enumerate()
                .map(|(i, shelf)| shelf_view(&key, i, shelf, cx)),
        )
        .into_any_element()
}

fn centered(child: AnyElement) -> AnyElement {
    div()
        .flex_1()
        .size_full()
        .flex()
        .items_center()
        .justify_center()
        .child(child)
        .into_any_element()
}

fn message(page: &Page, cx: &mut Context<MusicApp>) -> Option<impl IntoElement> {
    let message = page.message.clone()?;
    Some(div().text_color(cx.theme().muted_foreground).child(message))
}

fn header(header: &Header, cx: &mut Context<MusicApp>) -> impl IntoElement {
    let theme = cx.theme();
    let size = px(200.);
    h_flex()
        .gap_6()
        .pt_4()
        .items_end()
        .children(header.thumbnail.as_ref().map(|url| {
            let cover = img(url.clone()).size(size).object_fit(ObjectFit::Cover);
            if header.round {
                cover.rounded_full()
            } else {
                cover.rounded(theme.radius)
            }
        }))
        .child(
            v_flex()
                .gap_2()
                .min_w_0()
                .child(div().text_3xl().font_bold().child(header.title.clone()))
                .child(
                    div()
                        .text_color(theme.muted_foreground)
                        .child(runs_text(&header.subtitle)),
                )
                .child(
                    div()
                        .text_sm()
                        .text_color(theme.muted_foreground)
                        .child(header.second_subtitle.clone()),
                )
                .children(header.play.clone().map(|target| {
                    div()
                        .id("header-play")
                        .mt_2()
                        .px_4()
                        .py_2()
                        .w(px(96.))
                        .flex()
                        .justify_center()
                        .rounded_full()
                        .bg(theme.primary)
                        .text_color(theme.primary_foreground)
                        .font_semibold()
                        .cursor_pointer()
                        .child("Play")
                        .on_click(
                            cx.listener(move |this, _, _, cx| this.activate(target.clone(), cx)),
                        )
                })),
        )
}

fn shelf_view(key: &str, index: usize, shelf: &Shelf, cx: &mut Context<MusicApp>) -> AnyElement {
    let title = (!shelf.title.is_empty()).then(|| {
        v_flex()
            .children(shelf.strapline.clone().map(|s| {
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(s)
            }))
            .child(div().text_xl().font_bold().child(shelf.title.clone()))
    });
    let body = match shelf.style {
        ShelfStyle::Carousel => h_flex()
            .id(SharedString::from(format!("carousel:{key}:{index}")))
            .w_full()
            .gap_4()
            .items_start()
            .overflow_x_scroll()
            .children(
                shelf
                    .items
                    .iter()
                    .enumerate()
                    .map(|(i, item)| card(key, index, i, item, cx)),
            )
            .into_any_element(),
        ShelfStyle::Grid | ShelfStyle::TopResult => h_flex()
            .w_full()
            .flex_wrap()
            .gap_4()
            .items_start()
            .children(
                shelf
                    .items
                    .iter()
                    .enumerate()
                    .map(|(i, item)| card(key, index, i, item, cx)),
            )
            .into_any_element(),
        ShelfStyle::Buttons => h_flex()
            .w_full()
            .flex_wrap()
            .gap_2()
            .children(
                shelf
                    .items
                    .iter()
                    .enumerate()
                    .map(|(i, item)| button(key, index, i, item, cx)),
            )
            .into_any_element(),
        ShelfStyle::List | ShelfStyle::RowCarousel => v_flex()
            .w_full()
            .children(
                shelf
                    .items
                    .iter()
                    .enumerate()
                    .map(|(i, item)| row(key, index, i, item, cx)),
            )
            .into_any_element(),
    };
    v_flex()
        .gap_3()
        .children(title)
        .child(body)
        .into_any_element()
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
        let found = this
            .pages
            .states
            .get(&key)
            .and_then(|s| s.page.as_ref())
            .and_then(|p| p.shelves.get(shelf))
            .and_then(|s| Some((s.items.get(item)?.clone(), s.clone())));
        if let Some((item, shelf)) = found {
            this.activate_item(&item, &shelf, cx);
        }
    })
}

fn card(key: &str, shelf: usize, i: usize, item: &Item, cx: &mut Context<MusicApp>) -> AnyElement {
    let theme = cx.theme();
    let size = px(CARD);
    let cover = match &item.thumbnail {
        Some(url) => {
            let cover = img(url.clone()).size(size).object_fit(ObjectFit::Cover);
            if item.kind == ItemKind::Artist {
                cover.rounded_full().into_any_element()
            } else {
                cover.rounded(theme.radius).into_any_element()
            }
        }
        None => div()
            .size(size)
            .rounded(theme.radius)
            .bg(theme.muted)
            .into_any_element(),
    };
    v_flex()
        .id(SharedString::from(format!("card:{shelf}:{i}")))
        .w(size)
        .flex_none()
        .gap_1()
        .cursor_pointer()
        .child(cover)
        .child(
            div()
                .w_full()
                .truncate()
                .font_semibold()
                .child(item.title.clone()),
        )
        .child(
            div()
                .w_full()
                .truncate()
                .text_sm()
                .text_color(theme.muted_foreground)
                .child(runs_text(&item.subtitle)),
        )
        .on_click(on_activate(key, shelf, i, cx))
        .into_any_element()
}

fn row(key: &str, shelf: usize, i: usize, item: &Item, cx: &mut Context<MusicApp>) -> AnyElement {
    let theme = cx.theme();
    let duration = item
        .track
        .as_ref()
        .and_then(|t| t.duration)
        .map(|d| clock(f64::from(d)));
    h_flex()
        .id(SharedString::from(format!("row:{shelf}:{i}")))
        .w_full()
        .h(px(56.))
        .px_2()
        .gap_3()
        .items_center()
        .rounded(theme.radius)
        .cursor_pointer()
        .hover(|s| s.bg(theme.accent))
        .children(
            item.index
                .clone()
                .map(|n| div().w(px(24.)).text_color(theme.muted_foreground).child(n)),
        )
        .children(item.thumbnail.clone().map(|url| {
            img(url)
                .size(px(ROW_THUMB))
                .flex_none()
                .rounded(px(4.))
                .object_fit(ObjectFit::Cover)
        }))
        .child(
            v_flex()
                .flex_1()
                .min_w_0()
                .child(div().truncate().child(item.title.clone()))
                .child(
                    div()
                        .truncate()
                        .text_sm()
                        .text_color(theme.muted_foreground)
                        .child(runs_text(&item.subtitle)),
                ),
        )
        .children(duration.map(|d| div().text_sm().text_color(theme.muted_foreground).child(d)))
        .on_click(on_activate(key, shelf, i, cx))
        .into_any_element()
}

fn button(
    key: &str,
    shelf: usize,
    i: usize,
    item: &Item,
    cx: &mut Context<MusicApp>,
) -> AnyElement {
    let theme = cx.theme();
    div()
        .id(SharedString::from(format!("button:{shelf}:{i}")))
        .px_4()
        .py_2()
        .rounded(theme.radius)
        .bg(theme.secondary)
        .hover(|s| s.bg(theme.secondary_hover))
        .cursor_pointer()
        .child(item.title.clone())
        .on_click(on_activate(key, shelf, i, cx))
        .into_any_element()
}
