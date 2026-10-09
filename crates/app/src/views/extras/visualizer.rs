//! The full-window visualiser (V): the cover, the song and the audio
//! visualiser over the cover backdrop, nothing else. The style can be
//! switched in the top left corner; V or Esc leaves. The effects layer
//! (`crate::visuals`) draws the backdrop and the visualiser where this
//! view's slots say.

use encore_core::backend::Command;
use encore_core::model::Track;
use gpui_kit::assets::IconName;
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::super::page::covers;
use super::super::{runs_text, widgets};
use crate::app::MusicApp;
use crate::assets::Glyph;
use crate::theme::motion::MotionExt as _;
use crate::theme::{self, Colors, Type, elevation, motion, radius, size, space};
use crate::visuals::{self, Slot, config};

/// The cover is asked for at this size, as Stage asks for it.
const COVER_SOURCE: Pixels = px(1200.);

pub fn visualizer(
    app: &mut MusicApp,
    window: &mut Window,
    cx: &mut Context<MusicApp>,
) -> AnyElement {
    let c = theme::colors(cx);
    let view = window.viewport_size();
    let (w, h) = (f32::from(view.width), f32::from(view.height));
    let side = px((h * 0.36).min(w * 0.34).max(120.));
    let track = app.player.current().cloned();
    // The bar is hidden: the backdrop's colours come from this cover.
    visuals::set_bar_cover(
        track
            .as_ref()
            .and_then(|t| t.thumbnail.as_deref())
            .map(|u| covers::sized(u, size::PLAYER_COVER).into()),
        cx,
    );
    let painted = visuals::paints_full();
    if track.is_none() {
        visuals::clear_slot(Slot::Title, cx);
    }
    let shadow = !visuals::paints_cover_shadow(cx);
    if !app.extras.visualizer_hovered {
        visuals::clear_slot(Slot::Corner, cx);
    }
    let inner = div()
        .image_cache(covers::root_cache(cx))
        .id("visualizer")
        .debug_selector(|| "visualizer".to_owned())
        .key_context("Visualizer")
        .track_focus(&app.focus)
        .size_full()
        .relative()
        .when(!painted, |d| d.bg(c.base))
        .text_color(c.text)
        .type_body()
        .hover_listener_mode(HoverListenerMode::InputModalityIndependent)
        .on_hover(cx.listener(|this, hovered, _, cx| {
            this.extras.visualizer_hovered = *hovered;
            cx.notify();
        }))
        .child(visuals::slot(Slot::Stage))
        .child(visuals::slot(Slot::StageBody))
        .child(
            v_flex()
                .size_full()
                .items_center()
                .justify_center()
                // Clear of the band along the bottom (`visuals::visualizer`).
                .pb(px(h * 0.28))
                .child(song(track.as_ref(), side, shadow, &c)),
        )
        .when(app.extras.visualizer_hovered, |d| {
            d.child(top_bar(painted, &c, cx))
                .child(controls(app, &c, cx))
        })
        .with_motion(
            "enter:visualizer",
            motion::Kind::NowPlaying,
            motion::SLOW,
            |el, t| el.opacity(t),
        );
    // Every area's shortcuts work here too (Space plays and pauses).
    let root = div().key_context("Music").size_full().child(inner);
    let root = crate::pages::on_actions(root, cx);
    let root = crate::playback::on_actions(root, cx);
    let root = crate::account::on_actions(root, cx);
    let root = crate::desktop::on_actions(root, cx);
    crate::extras::on_actions(root, cx).into_any_element()
}

/// Quiet controls over the scene, using the player bar's volume state.
fn controls(app: &MusicApp, c: &Colors, cx: &mut Context<MusicApp>) -> impl IntoElement {
    h_flex()
        .id("visualizer-controls")
        .debug_selector(|| "visualizer-controls".to_owned())
        .absolute()
        .bottom(space::XL)
        .left(space::LG)
        .right(space::LG)
        .justify_center()
        .gap(space::LG)
        .opacity(0.6)
        .hover(|s| s.opacity(1.))
        .child(
            h_flex()
                .gap(space::SM)
                .child(super::super::player::volume(app, c, cx))
                .child(
                    div()
                        .debug_selector(|| {
                            format!(
                                "visualizer-volume:{}",
                                app.player.volume.read(cx).value().start()
                            )
                        })
                        .type_caption()
                        .tabular()
                        .text_color(c.text_muted)
                        .child(format!(
                            "{:.0}%",
                            app.player.volume.read(cx).value().start()
                        )),
                ),
        )
        .child(
            widgets::icon_button(
                "visualizer-next",
                widgets::glyph(Glyph::SkipForward, size::ICON, c.text_muted),
                c,
            )
            .debug_selector(|| "visualizer-next".to_owned())
            .tooltip(widgets::tooltip("Next"))
            .on_click(cx.listener(|this, _, _, _| this.send(Command::Next))),
        )
}

/// The cover with the title and artists under it.
fn song(track: Option<&Track>, side: Pixels, shadow: bool, c: &Colors) -> impl IntoElement {
    let title_size = (f32::from(side) * 0.075).clamp(22., 36.);
    let corner = visuals::stage_cover_radius(side);
    let url = track.and_then(|t| t.thumbnail.as_deref());
    // The ring's bars stand round the cover: the title moves clear of them.
    let ring = config::get().visualizer.style == config::Style::Ring;
    let below = if ring {
        space::XXL + visuals::ring_reach(side)
    } else {
        space::XXL
    };
    let cover = h_flex()
        .relative()
        .flex_none()
        .size(side)
        .justify_center()
        .rounded(corner)
        .bg(c.raised)
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
        .child(visuals::slot(Slot::StageCover));
    v_flex()
        .items_center()
        .child(cover)
        .children(track.map(|t| {
            v_flex()
                .relative()
                .child(visuals::slot(Slot::Title))
                .max_w(side * 1.6)
                .mt(below)
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
                        .text_size(px(title_size * 0.6))
                        .line_height(px(title_size * 0.6 * 1.35))
                        .text_color(c.text_muted)
                        .child(runs_text(&t.artists)),
                )
        }))
}

/// The styles on the left, leave on the right; a line saying the effects
/// are off when they are.
fn top_bar(painted: bool, c: &Colors, cx: &mut Context<MusicApp>) -> impl IntoElement {
    let left = if painted {
        styles(c, cx).into_any_element()
    } else {
        h_flex()
            .gap(space::MD)
            .child(
                div()
                    .type_small()
                    .text_color(c.text_muted)
                    .child("Visuals are off."),
            )
            .child(
                widgets::pill_button(
                    "visualizer-settings",
                    "Turn them on in Settings",
                    None,
                    widgets::Pill::Secondary,
                    c,
                )
                .on_click(cx.listener(|this, _, window, cx| {
                    this.toggle_visualizer(window, cx);
                    this.open_settings_at(crate::settings::Category::Visuals, window, cx);
                })),
            )
            .into_any_element()
    };
    h_flex()
        .id("visualizer-menu")
        .debug_selector(|| "visualizer-menu".to_owned())
        .absolute()
        .top(space::LG)
        .left(space::LG)
        .right(space::LG)
        .justify_between()
        .opacity(0.6)
        .hover(|s| s.opacity(1.))
        .child(visuals::slot(Slot::Corner))
        .child(left)
        .child(
            widgets::icon_button(
                "visualizer-close",
                widgets::icon(IconName::X, size::ICON, c.text_muted),
                c,
            )
            .tooltip(widgets::tooltip("Leave visualiser (Esc)"))
            .on_click(cx.listener(|this, _, window, cx| this.toggle_visualizer(window, cx))),
        )
}

/// The visualiser's styles as a segmented control, as Now Playing draws
/// its tabs.
fn styles(c: &Colors, cx: &mut Context<MusicApp>) -> impl IntoElement {
    let chosen = config::get().visualizer.style;
    let tab = |style: config::Style| {
        let active = style == chosen;
        let fg = if active { c.text } else { c.text_muted };
        let hover = c.text;
        h_flex()
            .id(SharedString::from(format!("visualizer-{}", style.label())))
            .h_full()
            .px(space::LG)
            .justify_center()
            .rounded(radius::FULL)
            .type_label()
            .text_color(fg)
            .when(active, |s| s.bg(c.overlay).shadow(elevation::low(c)))
            .when(!active, |s| {
                s.cursor_pointer()
                    .hover(move |s| s.text_color(hover))
                    .on_click(cx.listener(move |_, _, _, cx| {
                        let mut saved = config::saved();
                        saved.visualizer.style = style;
                        config::set(saved, true);
                        cx.notify();
                    }))
            })
            .child(style.label())
    };
    h_flex()
        .flex_none()
        .h(size::CHIP)
        .p(space::XXS)
        .rounded(radius::FULL)
        .bg(c.raised.opacity(0.85))
        .children(config::Style::ALL.map(tab))
}
