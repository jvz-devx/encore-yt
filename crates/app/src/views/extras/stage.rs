//! Stage (F): the music fills the window. A huge cover on the window's base
//! with the song under it, timed lyrics in large type beside it, and the
//! transport along the bottom. F or Esc leaves, F11 goes full screen.

use encore_core::backend::Command;
use encore_core::model::{Lyrics, Repeat, Track};
use gpui_kit::assets::IconName;
use gpui_kit::component::slider::Slider;
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::super::page::covers;
use super::super::{clock, runs_text, widgets};
use crate::app::MusicApp;
use crate::assets::Glyph;
use crate::playback::SEEK_SCALE;
use crate::theme::motion::MotionExt as _;
use crate::theme::{self, Colors, Type, elevation, motion, radius, size, space};
use crate::visuals::{self, Slot};

/// The transport's band along the bottom.
const TRANSPORT: f32 = 128.;
/// Room under the cover for the title and artists.
const TITLES: f32 = 96.;
/// The cover is asked for at this size, whatever it is drawn at, so a
/// resize or full screen doesn't load it again.
const COVER_SOURCE: Pixels = px(1200.);

pub fn stage(app: &mut MusicApp, window: &mut Window, cx: &mut Context<MusicApp>) -> AnyElement {
    let c = theme::colors(cx);
    let view = window.viewport_size();
    let (w, h) = (f32::from(view.width), f32::from(view.height));
    let margin = (w.min(h) * 0.06).max(32.);
    let track = app.player.current().cloned();
    // The bar is hidden: the backdrop's colours come from this cover.
    visuals::set_bar_cover(
        track
            .as_ref()
            .and_then(|t| t.thumbnail.as_deref())
            .map(|u| covers::sized(u, size::PLAYER_COVER).into()),
        cx,
    );
    if track.is_some() {
        app.request_current_lyrics();
    }
    let lyrics = app
        .player
        .current_lyrics()
        .and_then(|r| r.clone().ok().flatten());
    let words = lyrics
        .as_ref()
        .filter(|l| !l.lines.is_empty() || !l.text.trim().is_empty());
    // The visualiser's band, when it shows, stays clear under the body.
    let band = visuals::stage_band(h - TRANSPORT);
    let room = h - margin - TRANSPORT - TITLES - f32::from(band);
    let side = if words.is_some() {
        room.min((w - 2. * margin) * 0.42)
    } else {
        room.min((w - 2. * margin) * 0.62)
    }
    .max(120.);
    // The visualiser's ring, when it shows, stands in the cover's room.
    let ring = visuals::stage_ring_room(px(side));
    let side = (px(side) - ring * 2.).max(px(120.));
    let shadow = !visuals::paints_cover_shadow(cx);
    let song = song_column(track.as_ref(), side, ring, shadow, &c);
    let body = h_flex()
        .relative()
        .child(visuals::slot(Slot::StageBody))
        .flex_1()
        .min_h_0()
        .w_full()
        .px(px(margin))
        .pt(px(margin))
        .pb(band)
        .gap(px(margin))
        .child(
            v_flex()
                .h_full()
                .when(words.is_some(), |s| s.w(px((w - 2. * margin) * 0.42)))
                .when(words.is_none(), |s| s.flex_1())
                .flex_none()
                .items_center()
                .justify_center()
                .child(song),
        )
        .when_some(words.cloned(), |el, lyrics: Lyrics| {
            let id = track
                .as_ref()
                .map(|t| t.video_id.clone())
                .unwrap_or_default();
            // Its own box for the scrim (the list inside scrolls).
            el.child(
                h_flex()
                    .relative()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .child(visuals::slot(Slot::Lyrics))
                    .child(super::stage_lyrics::lyrics(
                        app, &id, &lyrics, h, window, &c, cx,
                    )),
            )
        });
    if words.is_none() {
        visuals::clear_slot(Slot::Lyrics, cx);
    }
    if track.is_none() {
        visuals::clear_slot(Slot::Title, cx);
    }
    let inner = v_flex()
        .key_context("Stage")
        .track_focus(&app.focus)
        .size_full()
        .relative()
        // The effects paint the backdrop and the visualiser behind it.
        .when(!visuals::paints_stage(), |d| d.bg(c.base))
        .child(visuals::slot(Slot::Stage))
        .text_color(c.text)
        .type_body()
        .image_cache(covers::root_cache(cx))
        .child(body)
        .child(transport(app, margin, &c, cx))
        .child(corner_buttons(window, &c, cx))
        .with_motion(
            "enter:stage",
            motion::Kind::NowPlaying,
            motion::SLOW,
            |el, t| el.opacity(t),
        );
    // Every area's shortcuts work in Stage too; Stage's own (Esc, F11,
    // F) are bound deeper, in its "Stage" context.
    let root = div().key_context("Music").size_full().child(inner);
    let root = crate::pages::on_actions(root, cx);
    let root = crate::playback::on_actions(root, cx);
    let root = crate::account::on_actions(root, cx);
    let root = crate::desktop::on_actions(root, cx);
    crate::extras::on_actions(root, cx).into_any_element()
}

/// The cover, huge, with the title and artists centred under it.
/// The cover (with `ring` of room round it for the visualiser's ring) and
/// the song's title and artists under it.
fn song_column(
    track: Option<&Track>,
    side: Pixels,
    ring: Pixels,
    shadow: bool,
    c: &Colors,
) -> impl IntoElement {
    let room = side + ring * 2.;
    let title_size = (f32::from(room) * 0.06).clamp(22., 34.);
    v_flex()
        .items_center()
        .child(div().m(ring).child(big_cover(
            track.and_then(|t| t.thumbnail.as_deref()),
            side,
            shadow,
            c,
        )))
        .children(track.map(|t| {
            v_flex()
                .relative()
                .child(visuals::slot(Slot::Title))
                .w(room * 1.2)
                .mt(space::XL)
                .items_center()
                .gap(space::XS)
                .child(
                    div()
                        .w_full()
                        .text_center()
                        .line_clamp(2)
                        .font_family(theme::FONT_DISPLAY)
                        .font_weight(FontWeight::BOLD)
                        .text_size(px(title_size))
                        .line_height(px(title_size * 1.2))
                        .child(t.title.clone()),
                )
                .child(
                    div()
                        .w_full()
                        .truncate()
                        .text_center()
                        .text_size(px(title_size * 0.62))
                        .line_height(px(title_size * 0.62 * 1.35))
                        .text_color(c.text_muted)
                        .child(runs_text(&t.artists)),
                )
        }))
}

fn big_cover(url: Option<&str>, side: Pixels, shadow: bool, c: &Colors) -> impl IntoElement {
    let corner = visuals::stage_cover_radius(side);
    h_flex()
        .relative()
        .flex_none()
        .size(side)
        .justify_center()
        .rounded(corner)
        .bg(c.raised)
        // The backdrop draws it while it shows.
        .when(shadow, |d| d.shadow(elevation::high(c)))
        .child(widgets::icon(IconName::Music, side * 0.2, c.text_faint))
        .children(url.map(|url| {
            img(SharedString::from(covers::sized(url, COVER_SOURCE)))
                .absolute()
                .inset_0()
                .size(side)
                .rounded(corner)
                .object_fit(ObjectFit::Cover)
        }))
        .child(
            div()
                .absolute()
                .inset_0()
                .rounded(corner)
                .border_1()
                .border_color(c.outline),
        )
        .child(visuals::slot(Slot::StageCover))
}

/// Full screen, the mini player and leave, in the top right corner.
fn corner_buttons(window: &Window, c: &Colors, cx: &mut Context<MusicApp>) -> impl IntoElement {
    let full = window.is_fullscreen();
    h_flex()
        .absolute()
        .top(space::LG)
        .right(space::LG)
        .gap(space::XS)
        .child(visuals::slot(Slot::Corner))
        .child(
            widgets::icon_button(
                "stage-fullscreen",
                widgets::icon(
                    if full {
                        IconName::Minimize2
                    } else {
                        IconName::Maximize2
                    },
                    size::ICON,
                    c.text_muted,
                ),
                c,
            )
            .tooltip(widgets::tooltip(if full {
                "Exit full screen (F11)"
            } else {
                "Full screen (F11)"
            }))
            .on_click(cx.listener(|this, _, window, cx| this.stage_fullscreen(window, cx))),
        )
        .child(
            widgets::icon_button(
                "stage-close",
                widgets::icon(IconName::X, size::ICON, c.text_muted),
                c,
            )
            .tooltip(widgets::tooltip("Leave Stage (Esc)"))
            .on_click(cx.listener(|this, _, window, cx| this.toggle_stage(window, cx))),
        )
}

/// The seek line with its ridge, and the transport under it.
fn transport(
    app: &MusicApp,
    margin: f32,
    c: &Colors,
    cx: &mut Context<MusicApp>,
) -> impl IntoElement {
    let player = &app.player;
    let playback = &player.playback;
    let duration = playback.duration;
    let elapsed = if player.seeking {
        f64::from(player.seek.read(cx).value().start() / SEEK_SCALE) * duration
    } else {
        player.position()
    };
    let known = duration > 0.0;
    let time = |t: String| {
        div()
            .w(px(48.))
            .flex_none()
            .type_caption()
            .tabular()
            // Muted rather than faint over the backdrop.
            .text_color(if visuals::paints_stage() {
                c.text_muted
            } else {
                c.text_faint
            })
            .child(t)
    };
    v_flex()
        .h(px(TRANSPORT))
        .flex_none()
        .px(px(margin))
        .justify_center()
        .gap(space::SM)
        .child(
            h_flex()
                .w_full()
                .gap(space::MD)
                .child(time(clock(elapsed)).text_right())
                .child(
                    div()
                        .relative()
                        .flex_1()
                        .children(super::ridge(app, c, cx))
                        .child(
                            Slider::new(&player.seek)
                                .disabled(!known)
                                .bg(c.signal)
                                .text_color(c.text),
                        ),
                )
                .child(time(if known {
                    clock(duration)
                } else {
                    String::new()
                })),
        )
        .child(
            h_flex()
                .w_full()
                .justify_center()
                .gap(space::MD)
                .child(toggle(
                    "stage-shuffle",
                    IconName::Shuffle,
                    playback.shuffle,
                    "Shuffle",
                    Command::ToggleShuffle,
                    c,
                    cx,
                ))
                .child(skip(
                    "stage-previous",
                    Glyph::SkipBack,
                    "Previous",
                    Command::Previous,
                    c,
                    cx,
                ))
                .child(play(playback.playing, playback.loading, c, cx))
                .child(skip(
                    "stage-next",
                    Glyph::SkipForward,
                    "Next",
                    Command::Next,
                    c,
                    cx,
                ))
                .child(toggle(
                    "stage-repeat",
                    if playback.repeat == Repeat::One {
                        IconName::Repeat1
                    } else {
                        IconName::Repeat
                    },
                    playback.repeat != Repeat::Off,
                    "Repeat",
                    Command::CycleRepeat,
                    c,
                    cx,
                )),
        )
}

fn toggle(
    id: &'static str,
    icon: IconName,
    on: bool,
    tip: &'static str,
    command: Command,
    c: &Colors,
    cx: &mut Context<MusicApp>,
) -> impl IntoElement {
    widgets::icon_button(
        id,
        widgets::icon(icon, size::ICON, widgets::toggle_color(on, c)),
        c,
    )
    .tooltip(widgets::tooltip(tip))
    .on_click(cx.listener(move |this, _, _, _| this.send(clone_command(&command))))
}

fn skip(
    id: &'static str,
    glyph: Glyph,
    tip: &'static str,
    command: Command,
    c: &Colors,
    cx: &mut Context<MusicApp>,
) -> impl IntoElement {
    widgets::icon_button(id, widgets::glyph(glyph, px(22.), c.text), c)
        .size(px(44.))
        .tooltip(widgets::tooltip(tip))
        .on_click(cx.listener(move |this, _, _, _| this.send(clone_command(&command))))
}

/// The transport's commands have no data, so they copy by kind.
fn clone_command(command: &Command) -> Command {
    match command {
        Command::Previous => Command::Previous,
        Command::Next => Command::Next,
        Command::ToggleShuffle => Command::ToggleShuffle,
        Command::CycleRepeat => Command::CycleRepeat,
        _ => Command::TogglePause,
    }
}

/// The large play disc.
fn play(playing: bool, loading: bool, c: &Colors, cx: &mut Context<MusicApp>) -> impl IntoElement {
    let fg = c.primary_foreground;
    let content = if loading {
        Spinner::new().color(fg).into_any_element()
    } else if playing {
        widgets::glyph(Glyph::Pause, px(24.), fg).into_any_element()
    } else {
        widgets::glyph(Glyph::Play, px(24.), fg).into_any_element()
    };
    let hover = c.primary_hover;
    h_flex()
        .id("stage-play")
        .mx(space::SM)
        .size(px(56.))
        .justify_center()
        .rounded(radius::FULL)
        .bg(c.primary)
        .when(!playing && !loading, |s| s.pl(px(3.)))
        .cursor_pointer()
        .hover(move |s| s.bg(hover))
        .active(|s| s.opacity(0.9))
        .child(content)
        .tooltip(widgets::tooltip("Play or pause (Space)"))
        .on_click(cx.listener(|this, _, _, _| this.send(Command::TogglePause)))
}
