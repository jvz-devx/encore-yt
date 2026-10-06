//! The player bar: the song, transport controls, position and volume.

use gpui_kit::assets::IconName as Icon;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::slider::Slider;
use gpui_kit::component::{ActiveTheme, Selectable, Sizable, StyledExt, h_flex, v_flex};
use gpui_kit::*;
use ytfast::backend::Command;
use ytfast::model::Repeat;

use super::{clock, runs_text};
use crate::app::MusicApp;

pub fn player_bar(app: &MusicApp, cx: &mut Context<MusicApp>) -> impl IntoElement {
    let theme = cx.theme();
    let track = app.current().cloned();
    let playback = &app.playback;
    let cover = px(52.);
    let song = h_flex()
        .w(px(300.))
        .gap_3()
        .items_center()
        .child(match track.as_ref().and_then(|t| t.thumbnail.clone()) {
            Some(url) => img(url)
                .size(cover)
                .flex_none()
                .rounded(px(4.))
                .object_fit(ObjectFit::Cover)
                .into_any_element(),
            None => div()
                .size(cover)
                .rounded(px(4.))
                .bg(theme.muted)
                .into_any_element(),
        })
        .child(
            v_flex()
                .min_w_0()
                .child(
                    div()
                        .truncate()
                        .font_semibold()
                        .child(track.as_ref().map(|t| t.title.clone()).unwrap_or_default()),
                )
                .child(
                    div()
                        .truncate()
                        .text_sm()
                        .text_color(theme.muted_foreground)
                        .child(
                            track
                                .as_ref()
                                .map(|t| runs_text(&t.artists))
                                .unwrap_or_default(),
                        ),
                ),
        );

    let play_icon = if playback.playing {
        Icon::Pause
    } else {
        Icon::Play
    };
    let repeat_icon = if playback.repeat == Repeat::One {
        Icon::Repeat1
    } else {
        Icon::Repeat
    };
    let transport = h_flex()
        .gap_2()
        .justify_center()
        .child(
            Button::new("shuffle")
                .ghost()
                .icon(Icon::Shuffle)
                .selected(playback.shuffle)
                .on_click(cx.listener(|this, _, _, _| this.send(Command::ToggleShuffle))),
        )
        .child(
            Button::new("previous")
                .ghost()
                .icon(Icon::SkipBack)
                .on_click(cx.listener(|this, _, _, _| this.send(Command::Previous))),
        )
        .child(
            Button::new("play")
                .primary()
                .icon(play_icon)
                .loading(playback.loading)
                .on_click(cx.listener(|this, _, _, _| this.send(Command::TogglePause))),
        )
        .child(
            Button::new("next")
                .ghost()
                .icon(Icon::SkipForward)
                .on_click(cx.listener(|this, _, _, _| this.send(Command::Next))),
        )
        .child(
            Button::new("repeat")
                .ghost()
                .icon(repeat_icon)
                .selected(playback.repeat != Repeat::Off)
                .on_click(cx.listener(|this, _, _, _| this.send(Command::CycleRepeat))),
        );
    let position = h_flex()
        .w_full()
        .gap_2()
        .items_center()
        .text_xs()
        .text_color(theme.muted_foreground)
        .child(div().w(px(44.)).child(clock(app.position())))
        .child(div().flex_1().child(Slider::new(&app.seek)))
        .child(div().w(px(44.)).child(clock(playback.duration)));

    h_flex()
        .h(px(84.))
        .px_4()
        .gap_6()
        .items_center()
        .border_t_1()
        .border_color(theme.border)
        .bg(theme.sidebar)
        .child(song)
        .child(
            v_flex()
                .flex_1()
                .gap_1()
                .items_center()
                .child(transport)
                .child(position),
        )
        .child(
            h_flex()
                .w(px(200.))
                .gap_2()
                .items_center()
                .child(gpui_kit::component::Icon::new(Icon::Volume2).small())
                .child(div().flex_1().child(Slider::new(&app.volume))),
        )
}
