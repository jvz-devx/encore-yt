//! Now Playing: the large cover with the song beside YouTube Music's three
//! tabs, Up next, Lyrics and Related. It fills the page panel; navigating
//! to a page closes it.

mod lyrics;
mod related;

use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::player::links;
use super::queue::{self, Place};
use super::widgets;
use crate::app::MusicApp;
use crate::playback::Tab;
use crate::theme::{self, Colors, Type, elevation, motion, radius, size, space};

/// The tab column's width.
const TABS: Pixels = px(420.);
/// Room under the cover for the title, artists and format.
const BELOW_COVER: Pixels = px(150.);
const COVER_MIN: Pixels = px(160.);
const COVER_MAX: Pixels = px(560.);

pub fn now_playing(
    app: &mut MusicApp,
    window: &mut Window,
    cx: &mut Context<MusicApp>,
) -> AnyElement {
    // A page opened from elsewhere (the sidebar, Back) takes the panel.
    if app.player.now_playing_over.as_ref() != Some(&app.pages.view) {
        app.player.now_playing = false;
        app.player.now_playing_over = None;
        return super::page::page(app, window, cx);
    }
    let c = theme::colors(cx);
    let side = cover_side(app, window);
    h_flex()
        .flex_1()
        .w_full()
        .min_h_0()
        .items_start()
        .px(size::GUTTER)
        .pb(space::XL)
        .gap(space::XXL)
        .child(song(app, side, &c, cx))
        .child(tabs_column(app, window, &c, cx))
        .with_animation(
            "enter:now-playing",
            Animation::new(motion::BASE).with_easing(motion::ease_out),
            |el, t| el.opacity(t),
        )
        .into_any_element()
}

/// The cover as large as fits beside the tabs and above the song's lines.
fn cover_side(app: &MusicApp, window: &Window) -> Pixels {
    let view = window.viewport_size();
    let strip = if app.error.is_some() { px(48.) } else { px(0.) };
    let high = view.height
        - size::TOP_BAR
        - size::PLAYER_BAR
        - space::SM
        - space::XS
        - space::XL
        - strip
        - BELOW_COVER;
    let wide = view.width - size::SIDEBAR - space::SM - size::GUTTER * 2. - TABS - space::XXL;
    high.min(wide).clamp(COVER_MIN, COVER_MAX)
}

/// The cover, title, artists and album, the like control and the format.
fn song(app: &MusicApp, side: Pixels, c: &Colors, cx: &mut Context<MusicApp>) -> impl IntoElement {
    let track = app.player.current();
    let like = track.and_then(|t| super::account::like_button(app, t, cx));
    let format = app.player.playback.format.clone();
    v_flex()
        .flex_1()
        .min_w_0()
        .h_full()
        .items_center()
        .justify_center()
        .child(
            widgets::cover(
                track.and_then(|t| t.thumbnail.clone()).map(Into::into),
                side,
                false,
                c,
            )
            .shadow(elevation::high(c)),
        )
        .children(track.map(|track| {
            v_flex()
                .w(side)
                .mt(space::XL)
                .items_center()
                .gap(space::XS)
                .child(
                    div()
                        .w_full()
                        .text_center()
                        .type_title()
                        .line_clamp(2)
                        .child(track.title.clone()),
                )
                .child(
                    h_flex().max_w_full().justify_center().child(
                        links::track_links("now-playing", track, app, c, cx)
                            .type_body()
                            .text_color(c.text_muted),
                    ),
                )
        }))
        .child(
            h_flex()
                .mt(space::MD)
                .gap(space::MD)
                .children(like)
                .children(format.map(|full| {
                    div()
                        .id("now-playing-format")
                        .type_caption()
                        .tabular()
                        .text_color(c.text_faint)
                        .child(crate::playback::short_format(&full))
                        .tooltip(widgets::tooltip(full))
                })),
        )
}

fn tabs_column(
    app: &mut MusicApp,
    window: &mut Window,
    c: &Colors,
    cx: &mut Context<MusicApp>,
) -> impl IntoElement {
    let body = match app.player.tab {
        Tab::UpNext => v_flex()
            .flex_1()
            .min_h_0()
            .gap(space::MD)
            .child(
                queue::controls(app, c, cx)
                    .px(space::SM)
                    .children(super::account::save_queue_button(app, c, cx))
                    .children(queue::clear_button(app, c, cx)),
            )
            .child(queue::list(app, Place::Tab, c, cx))
            .into_any_element(),
        Tab::Lyrics => lyrics::lyrics(app, window, c, cx).into_any_element(),
        Tab::Related => related::related(app, c, cx).into_any_element(),
    };
    v_flex()
        .w(TABS)
        .flex_none()
        .h_full()
        .gap(space::LG)
        .child(tab_bar(app, c, cx))
        .child(body)
}

/// Up next, Lyrics and Related as a segmented control: the chosen tab is a
/// lifted pill on the raised track.
fn tab_bar(app: &MusicApp, c: &Colors, cx: &mut Context<MusicApp>) -> impl IntoElement {
    let related = app.player.playback.related.is_some();
    let tab = |id: &'static str, label: &'static str, which: Tab, enabled: bool| {
        let active = app.player.tab == which;
        let fg = match (active, enabled) {
            (true, _) => c.text,
            (false, true) => c.text_muted,
            (false, false) => c.text_faint,
        };
        let hover = c.text;
        h_flex()
            .id(id)
            .flex_1()
            .h_full()
            .justify_center()
            .rounded(radius::FULL)
            .type_label()
            .text_color(fg)
            .when(active, |s| s.bg(c.overlay).shadow(elevation::low(c)))
            .when(!active && enabled, |s| {
                s.cursor_pointer()
                    .hover(move |s| s.text_color(hover))
                    .on_click(cx.listener(move |this, _, _, cx| this.set_tab(which, cx)))
            })
            .child(label)
    };
    h_flex()
        .flex_none()
        .h(size::CHIP)
        .p(space::XXS)
        .rounded(radius::FULL)
        .bg(c.raised)
        .child(tab("tab-up-next", "Up next", Tab::UpNext, true))
        .child(tab("tab-lyrics", "Lyrics", Tab::Lyrics, true))
        .child(tab("tab-related", "Related", Tab::Related, related))
}
