//! The player bar's quiet second line: the sleep timer (its time left while
//! one is set) and the equalizer, with their panels rising above the bar;
//! and the mini player button beside the volume.

use std::time::{Duration, Instant};

use encore_core::model::{Sleep, SleepTimer};
use gpui_kit::assets::IconName;
use gpui_kit::component::h_flex;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::super::{clock, widgets};
use crate::app::MusicApp;
use crate::theme::{Colors, Type, radius, size, space};

/// The second line's controls are small, so the line stays a caption.
const SMALL: Pixels = px(26.);
const SMALL_ICON: Pixels = px(15.);

/// The sleep timer and the equalizer buttons.
pub fn controls(app: &MusicApp, c: &Colors, cx: &mut Context<MusicApp>) -> impl IntoElement {
    h_flex()
        .gap(space::XXS)
        .child(sleep_button(app, c, cx))
        .child(equalizer_button(app, c, cx))
}

/// The open panel of the two, drawn with the window's overlays: the player
/// bar is a layer under the app's views (`player::layer`), so a panel
/// inside it would open behind the page.
pub fn panel(app: &MusicApp, cx: &mut Context<MusicApp>) -> Option<AnyElement> {
    let c = crate::theme::colors(cx);
    if app.extras.sleep_open {
        Some(super::sleep::menu(app, &c, cx))
    } else if app.extras.equalizer_open {
        Some(super::equalizer::panel(app, &c, cx))
    } else {
        None
    }
}

/// The mini player button, beside the volume.
pub fn mini_button(c: &Colors, cx: &mut Context<MusicApp>) -> impl IntoElement {
    widgets::icon_button(
        "mini-player",
        widgets::icon(IconName::PictureInPicture2, size::ICON, c.text_muted),
        c,
    )
    .tooltip(widgets::tooltip("Mini player (Ctrl+M)"))
    .on_click(cx.listener(|this, _, _, cx| this.toggle_mini(cx)))
}

/// A small ghost button on the second line: an icon, and text beside it
/// while it has something to say.
fn small_button(
    id: &'static str,
    icon: IconName,
    text: Option<String>,
    color: Hsla,
    c: &Colors,
) -> Stateful<Div> {
    h_flex()
        .id(id)
        .h(SMALL)
        .min_w(SMALL)
        .justify_center()
        .gap(space::XS)
        .when(text.is_some(), |s| s.pl(px(7.)).pr(space::SM))
        .rounded(radius::FULL)
        .cursor_pointer()
        .hover(|s| s.bg(c.hover))
        .active(|s| s.bg(c.pressed))
        .child(widgets::icon(icon, SMALL_ICON, color))
        .children(text.map(|t| div().type_caption().tabular().text_color(color).child(t)))
}

fn sleep_button(app: &MusicApp, c: &Colors, cx: &mut Context<MusicApp>) -> Stateful<Div> {
    let timer = app.player.playback.sleep;
    let (color, text) = match timer {
        Some(timer) => (c.signal, Some(time_left(&timer))),
        None if app.extras.sleep_open => (c.text, None),
        None => (c.text_faint, None),
    };
    small_button("sleep-timer", IconName::Moon, text, color, c)
        .when(app.extras.sleep_open, |s| s.bg(c.selected))
        .tooltip(widgets::tooltip("Sleep timer"))
        .on_click(cx.listener(|this, _, window, cx| this.toggle_sleep_menu(window, cx)))
}

fn equalizer_button(app: &MusicApp, c: &Colors, cx: &mut Context<MusicApp>) -> Stateful<Div> {
    let eq = app.equalizer();
    let color = if eq.active() {
        c.signal
    } else if app.extras.equalizer_open {
        c.text
    } else {
        c.text_faint
    };
    let text = eq.active().then(|| eq.preset.label().to_string());
    small_button("equalizer", IconName::SlidersVertical, text, color, c)
        .when(app.extras.equalizer_open, |s| s.bg(c.selected))
        .tooltip(widgets::tooltip("Equalizer (E)"))
        .on_click(cx.listener(|this, _, window, cx| this.toggle_equalizer(window, cx)))
}

/// "12:34" left on a timer, or "End of song".
pub fn time_left(timer: &SleepTimer) -> String {
    match (timer.choice, timer.deadline) {
        (Sleep::Minutes(_), Some(deadline)) => clock(
            deadline
                .saturating_duration_since(Instant::now())
                .as_secs_f64()
                .ceil(),
        ),
        _ => "End of song".into(),
    }
}

/// While a timer counts down, the bar redraws each second (the position
/// clock only ticks while a song plays).
pub fn keep_counting(app: &mut MusicApp, cx: &mut Context<MusicApp>) {
    let counting = matches!(
        app.player.playback.sleep,
        Some(SleepTimer {
            deadline: Some(_),
            ..
        })
    );
    if !counting {
        app.extras.sleep_tick = None;
        return;
    }
    if app.extras.sleep_tick.is_some() {
        return;
    }
    app.extras.sleep_tick = Some(cx.spawn(async move |this, cx| {
        loop {
            cx.background_executor().timer(Duration::from_secs(1)).await;
            if this.update(cx, |_, cx| cx.notify()).is_err() {
                break;
            }
        }
    }));
}
