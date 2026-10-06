//! One Up next row, and what a dragged row looks like under the pointer.

use gpui_kit::assets::IconName;
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use ytfast::backend::Command;

use super::super::widgets;
use super::super::{clock, runs_text};
use super::Place;
use crate::app::MusicApp;
use crate::assets::Glyph;
use crate::theme::{self, Colors, Type, elevation, radius, size, space};

/// The width of a dragged row's ghost.
const GHOST: Pixels = px(320.);
/// The length column, wide enough for "1:02:03".
const LENGTH: Pixels = px(52.);

/// A row being dragged: where it came from, and what the ghost shows.
#[derive(Clone)]
pub struct DraggedSong {
    from: usize,
    title: SharedString,
    subtitle: SharedString,
    thumbnail: Option<SharedString>,
}

impl Render for DraggedSong {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let c = theme::colors(cx);
        h_flex()
            .w(GHOST)
            .h(size::ROW)
            .px(space::SM)
            .gap(space::MD)
            .rounded(radius::MD)
            .bg(c.overlay)
            .shadow(elevation::high(&c))
            .text_color(c.text)
            .child(widgets::cover(
                self.thumbnail.clone(),
                size::ROW_THUMB,
                false,
                &c,
            ))
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .child(div().truncate().type_label().child(self.title.clone()))
                    .child(widgets::muted_line(self.subtitle.clone(), &c)),
            )
    }
}

pub fn row(
    app: &MusicApp,
    place: Place,
    i: usize,
    c: &Colors,
    cx: &mut Context<MusicApp>,
) -> Option<AnyElement> {
    let track = app.player.queue.get(i)?;
    let current = app.player.playback.index == Some(i);
    let playing = current && app.player.playback.playing;
    let subtitle = runs_text(&track.artists);
    let dragged = DraggedSong {
        from: i,
        title: track.title.clone().into(),
        subtitle: subtitle.clone().into(),
        thumbnail: track.thumbnail.clone().map(Into::into),
    };
    let line = c.text_muted;
    let group = SharedString::from(format!("queue-row-{}", place.name()));
    Some(
        h_flex()
            .id(SharedString::from(format!("queue-{}-{i}", place.name())))
            .group(group.clone())
            .w_full()
            .h(size::ROW)
            .px(space::SM)
            .gap(space::MD)
            .rounded(radius::MD)
            .cursor_pointer()
            .when(current, |s| s.bg(c.selected))
            .hover(|s| s.bg(c.hover))
            .active(|s| s.bg(c.pressed))
            .child(thumb(track.thumbnail.clone(), current, playing, &group, c))
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .child(
                        div()
                            .truncate()
                            .type_label()
                            .text_color(if current { c.signal } else { c.text })
                            .child(track.title.clone()),
                    )
                    .child(widgets::muted_line(subtitle, c)),
            )
            .child(trailing(place, i, track.duration, current, &group, c, cx))
            // Drag to reorder: the row lands where it is dropped, and a line
            // shows on the side it will take.
            .on_drag(dragged, |song, _, _, cx| cx.new(|_| song.clone()))
            .drag_over::<DraggedSong>(move |style, song, _, _| {
                if song.from < i {
                    style.border_b_2().border_color(line)
                } else if song.from > i {
                    style.border_t_2().border_color(line)
                } else {
                    style
                }
            })
            .on_drop(cx.listener(move |this, song: &DraggedSong, _, cx| {
                if song.from != i {
                    this.edit_queue(
                        Command::MoveInQueue {
                            from: song.from,
                            to: i,
                        },
                        cx,
                    );
                }
            }))
            .on_click(cx.listener(move |this, _, _, cx| {
                if this.player.playback.index != Some(i) {
                    this.send(Command::JumpTo(i));
                }
                cx.notify();
            }))
            .into_any_element(),
    )
}

/// The cover; the current song's carries the playing mark, the others a
/// play glyph under the pointer.
fn thumb(
    url: Option<String>,
    current: bool,
    playing: bool,
    group: &SharedString,
    c: &Colors,
) -> impl IntoElement {
    let overlay = h_flex()
        .absolute()
        .inset_0()
        .justify_center()
        .rounded(radius::XS)
        .bg(c.scrim);
    let overlay = if current {
        let mark = if playing {
            widgets::icon(IconName::AudioLines, size::ICON, c.on_media).into_any_element()
        } else {
            widgets::glyph(Glyph::Play, size::ICON_SM, c.on_media).into_any_element()
        };
        overlay.child(mark)
    } else {
        overlay
            .opacity(0.)
            .group_hover(group.clone(), |s| s.opacity(1.))
            .child(widgets::glyph(Glyph::Play, size::ICON_SM, c.on_media))
    };
    widgets::cover(url.map(Into::into), size::ROW_THUMB, false, c).child(overlay)
}

/// The length, which gives way to Remove under the pointer (the current
/// song can't be removed).
fn trailing(
    place: Place,
    i: usize,
    duration: Option<u32>,
    current: bool,
    group: &SharedString,
    c: &Colors,
    cx: &mut Context<MusicApp>,
) -> impl IntoElement {
    let length = duration.map(|d| clock(f64::from(d))).unwrap_or_default();
    let cell = div()
        .relative()
        .flex_none()
        .w(LENGTH)
        .h(size::ICON_BUTTON)
        .child(
            h_flex()
                .absolute()
                .inset_0()
                .justify_end()
                .type_small()
                .tabular()
                .text_color(c.text_faint)
                .when(!current, |s| {
                    s.group_hover(group.clone(), |s| s.opacity(0.))
                })
                .child(length),
        );
    if current {
        return cell;
    }
    cell.child(
        widgets::icon_button(
            SharedString::from(format!("remove-{}-{i}", place.name())),
            widgets::icon(IconName::X, size::ICON_SM, c.text_muted),
            c,
        )
        .absolute()
        .top_0()
        .right_0()
        .opacity(0.)
        .group_hover(group.clone(), |s| s.opacity(1.))
        .tooltip(widgets::tooltip("Remove from queue"))
        .on_click(cx.listener(move |this, _, _, cx| {
            cx.stop_propagation();
            this.edit_queue(Command::RemoveFromQueue(i), cx);
        })),
    )
}
