//! The player bar's right side: autoplay, Up next and volume, with the
//! format playing under them.

use encore_core::backend::Command;
use gpui_kit::assets::IconName;
use gpui_kit::component::slider::Slider;
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::*;

use super::super::widgets;
use crate::app::MusicApp;
use crate::theme::{Colors, Type, space};

/// The volume slider's width.
const VOLUME: Pixels = px(104.);

pub fn side(
    app: &MusicApp,
    width: Pixels,
    c: &Colors,
    cx: &mut Context<MusicApp>,
) -> impl IntoElement {
    let player = &app.player;
    let autoplay = player.playback.autoplay;
    let up_next_on =
        player.queue_open || (player.now_playing && player.tab == crate::playback::Tab::UpNext);
    v_flex()
        .w(width)
        .flex_none()
        .items_end()
        .gap(space::XXS)
        .child(
            h_flex()
                .gap(space::XS)
                .child(super::casting::button(app, c, cx))
                .child(
                    widgets::icon_button(
                        "autoplay",
                        widgets::icon(
                            IconName::Infinity,
                            crate::theme::size::ICON,
                            widgets::toggle_color(autoplay, c),
                        ),
                        c,
                    )
                    .tooltip(widgets::tooltip("Autoplay"))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.edit_queue(Command::Autoplay(!autoplay), cx)
                    })),
                )
                .child(
                    widgets::icon_button(
                        "up-next",
                        widgets::icon(
                            IconName::ListMusic,
                            crate::theme::size::ICON,
                            if up_next_on { c.text } else { c.text_muted },
                        ),
                        c,
                    )
                    .tooltip(widgets::tooltip("Up next"))
                    .on_click(cx.listener(|this, _, _, cx| this.toggle_up_next(cx))),
                )
                .child(volume(app, c, cx))
                .child(super::super::extras::mini_button(c, cx)),
        )
        .child(
            h_flex()
                .gap(space::XS)
                .children(format(app, c))
                .child(super::super::extras::controls(app, c, cx)),
        )
}

fn volume(app: &MusicApp, c: &Colors, cx: &mut Context<MusicApp>) -> impl IntoElement {
    let level = app.player.playback.volume;
    let muted = app.player.muted_from.is_some() || level <= 0.0;
    let icon = if muted {
        IconName::VolumeX
    } else if level < 50.0 {
        IconName::Volume1
    } else {
        IconName::Volume2
    };
    h_flex()
        .gap(space::XS)
        .child(
            widgets::icon_button(
                "mute",
                widgets::icon(icon, crate::theme::size::ICON, c.text_muted),
                c,
            )
            .tooltip(widgets::tooltip("Mute"))
            .on_click(cx.listener(|this, _, window, cx| this.toggle_mute(window, cx))),
        )
        .child(
            div().w(VOLUME).child(
                Slider::new(&app.player.volume)
                    .bg(c.text_muted)
                    .text_color(c.text),
            ),
        )
}

/// "Opus 256 kbps" under the controls; the itag in its tooltip.
fn format(app: &MusicApp, c: &Colors) -> Option<impl IntoElement> {
    if let Some(status) = super::casting::status(app) {
        return Some(
            div()
                .id("format")
                .debug_selector(|| format!("cast-status:{status}"))
                .pr(space::XS)
                .max_w_full()
                .truncate()
                .type_caption()
                .tabular()
                .text_color(c.signal)
                .child(status.clone())
                .tooltip(widgets::tooltip(status)),
        );
    }
    let full = app.player.playback.format.clone()?;
    let short = crate::playback::short_format(&full);
    Some(
        div()
            .id("format")
            .pr(space::XS)
            .type_caption()
            .tabular()
            .text_color(c.text_faint)
            .child(short)
            .tooltip(widgets::tooltip(full)),
    )
}
