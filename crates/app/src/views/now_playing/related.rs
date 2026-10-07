//! The Related tab: YouTube Music's related page for the song (songs you
//! might like, similar artists, more from the artist), as one list of
//! rows per shelf.

use encore_core::model::{Item, ItemKind, Shelf, Target};
use gpui_kit::assets::IconName;
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::super::widgets;
use super::super::{clock, runs_text};
use crate::app::MusicApp;
use crate::theme::{Colors, Type, radius, size, space};

pub fn related(app: &mut MusicApp, c: &Colors, cx: &mut Context<MusicApp>) -> AnyElement {
    let Some(id) = app.player.playback.related.clone() else {
        return centred(widgets::empty_state(
            IconName::Disc3,
            "Nothing related to show",
            "YouTube Music has no related songs for this one.",
            c,
        ));
    };
    let target = Target::browse(id);
    let key = target.key();
    app.ensure_page(target, false);
    let Some(state) = app.pages.states.get(&key) else {
        return loading(c);
    };
    let Some(page) = &state.page else {
        return match &state.error {
            Some(error) => centred(widgets::empty_state(
                IconName::CircleAlert,
                "Couldn't load related songs",
                error.clone(),
                c,
            )),
            None => loading(c),
        };
    };
    let playing = app.player.current().map(|t| t.video_id.clone());
    v_flex()
        .id(SharedString::from(format!("related:{key}")))
        .flex_1()
        .min_h_0()
        .overflow_y_scroll()
        .pb(space::XXXL)
        .gap(space::XL)
        .children(page.shelves.iter().enumerate().map(|(s, shelf)| {
            let rows: Vec<AnyElement> = shelf
                .items
                .iter()
                .enumerate()
                .map(|(i, item)| {
                    row(&key, s, i, item, playing.as_deref(), c, cx).into_any_element()
                })
                .collect();
            v_flex()
                .gap(space::XS)
                .when(!shelf.title.is_empty(), |col| {
                    col.child(
                        div()
                            .px(space::SM)
                            .pb(space::XS)
                            .type_heading()
                            .child(shelf.title.clone()),
                    )
                })
                .children(rows)
        }))
        .into_any_element()
}

fn centred(content: Div) -> AnyElement {
    v_flex()
        .flex_1()
        .justify_center()
        .pb(space::XXXL)
        .child(content)
        .into_any_element()
}

fn loading(c: &Colors) -> AnyElement {
    v_flex()
        .gap(space::SM)
        .px(space::SM)
        .child(widgets::skeleton(px(160.), px(20.), radius::XS, c))
        .children((0..6).map(|_| {
            h_flex()
                .h(size::ROW)
                .gap(space::MD)
                .child(widgets::skeleton(
                    size::ROW_THUMB,
                    size::ROW_THUMB,
                    radius::XS,
                    c,
                ))
                .child(
                    v_flex()
                        .gap(space::XS)
                        .child(widgets::skeleton(px(180.), px(14.), radius::XS, c))
                        .child(widgets::skeleton(px(120.), px(12.), radius::XS, c)),
                )
        }))
        .into_any_element()
}

/// A song, album, artist or playlist as a row: songs play with their shelf
/// as the queue, the rest open their page.
fn row(
    key: &str,
    shelf: usize,
    i: usize,
    item: &Item,
    playing: Option<&str>,
    c: &Colors,
    cx: &mut Context<MusicApp>,
) -> impl IntoElement {
    let live = matches!((&item.track, playing), (Some(t), Some(id)) if t.video_id == id);
    let round = item.kind == ItemKind::Artist;
    let length = item
        .track
        .as_ref()
        .and_then(|t| t.duration)
        .map(|d| clock(f64::from(d)));
    let key = key.to_string();
    h_flex()
        .id(SharedString::from(format!("related-{shelf}-{i}")))
        .w_full()
        .h(size::ROW)
        .px(space::SM)
        .gap(space::MD)
        .rounded(radius::MD)
        .cursor_pointer()
        .when(live, |s| s.bg(c.selected))
        .hover(|s| s.bg(c.hover))
        .active(|s| s.bg(c.pressed))
        .child(widgets::cover(
            item.thumbnail.clone().map(Into::into),
            size::ROW_THUMB,
            round,
            c,
        ))
        .child(
            v_flex()
                .flex_1()
                .min_w_0()
                .child(
                    div()
                        .truncate()
                        .type_label()
                        .text_color(if live { c.signal } else { c.text })
                        .child(item.title.clone()),
                )
                .child(widgets::muted_line(runs_text(&item.subtitle), c)),
        )
        .children(length.map(|d| {
            div()
                .flex_none()
                .type_small()
                .tabular()
                .text_color(c.text_faint)
                .child(d)
        }))
        .on_click(cx.listener(move |this, _, _, cx| {
            let Some((item, shelf)) = find(this, &key, shelf, i) else {
                return;
            };
            if item.track.is_none() {
                // A page: Now Playing makes way for it.
                this.player.now_playing = false;
                this.player.now_playing_over = None;
            }
            this.activate_item(&item, &shelf, cx);
        }))
}

fn find(this: &MusicApp, key: &str, shelf: usize, item: usize) -> Option<(Item, Shelf)> {
    let shelf = this
        .pages
        .states
        .get(key)?
        .page
        .as_ref()?
        .shelves
        .get(shelf)?;
    Some((shelf.items.get(item)?.clone(), shelf.clone()))
}
