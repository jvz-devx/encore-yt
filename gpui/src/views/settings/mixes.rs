//! Settings → Smooth mixes: radios, mixes and autoplay crossfade between
//! songs, and for how long.

use gpui_kit::component::slider::Slider;
use gpui_kit::*;
use ytfast::model::Mixes;

use super::super::widgets;
use crate::app::MusicApp;
use crate::theme::Colors;

const SLIDER: Pixels = px(160.);

pub fn section(
    app: &MusicApp,
    c: &Colors,
    window: &mut Window,
    cx: &mut Context<MusicApp>,
) -> AnyElement {
    let mixes = app.player.playback.mixes;
    let seconds = app.extras.mix_drag.unwrap_or(mixes.seconds);
    sync_slider(app, mixes.seconds, window, cx);
    let switch = widgets::switch("mixes", mixes.on, c).on_click(cx.listener(
        move |this, on: &bool, _, cx| {
            let mixes = this.player.playback.mixes;
            this.set_mixes(Mixes { on: *on, ..mixes }, cx);
        },
    ));
    let slider = div().w(SLIDER).child(
        Slider::new(&app.extras.mix_length)
            .disabled(!mixes.on)
            .bg(if mixes.on { c.signal } else { c.text_faint })
            .text_color(c.text),
    );
    super::section(
        "Smooth mixes",
        c,
        [
            super::row(
                "Crossfade between songs",
                Some("On radios, mixes and autoplay. Albums and playlists stay gapless.".into()),
                switch,
                c,
            ),
            super::row(
                "Crossfade length",
                Some(format!("{seconds} seconds").into()),
                slider,
                c,
            ),
        ],
    )
}

/// The slider follows the saved length, except while it is dragged.
fn sync_slider(app: &MusicApp, seconds: u8, window: &mut Window, cx: &mut Context<MusicApp>) {
    let value = f32::from(seconds);
    if app.extras.mix_drag.is_none() && app.extras.mix_length.read(cx).value().start() != value {
        app.extras
            .mix_length
            .update(cx, |slider, cx| slider.set_value(value, window, cx));
    }
}
