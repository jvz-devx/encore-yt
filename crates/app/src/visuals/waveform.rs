//! The song's waveform under Now Playing: its loudness outline as thin
//! bars, the part already played in `signal`. A click seeks there.
//!
//! While the effects layer paints the panel ([`super::fills_panel`]) it
//! paints the waveform too, in the box this view leaves
//! ([`Slot::Waveform`]): the position then moves without re-rendering the
//! app's views.
//!
//! The outline comes from `encore_visuals::waveform` on a background task
//! (the audio engine's decoder, cached per video id) once the song plays;
//! until then a faint line holds its place.

use std::cell::Cell;
use std::rc::Rc;

use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::slots::{Slot, slot};
use crate::app::MusicApp;
use crate::theme::Colors;

/// The newest outline, and the song being decoded.
#[derive(Default)]
struct Waveforms {
    shown: Option<(String, Vec<f32>)>,
    loading: Option<String>,
}

impl Global for Waveforms {}

/// The waveform of the playing song, `width` wide and `height` tall.
pub fn waveform(
    app: &MusicApp,
    width: Pixels,
    height: Pixels,
    c: &Colors,
    cx: &mut Context<MusicApp>,
) -> impl IntoElement {
    let id = app.player.current().map(|t| t.video_id.clone());
    let playback = &app.player.playback;
    if let Some(id) = &id
        && playback.playing
        && !playback.loading
    {
        request(app, id, cx);
    }
    let duration = playback.duration;
    let bounds = Rc::new(Cell::new(None::<Bounds<Pixels>>));
    let seen = bounds.clone();
    let painted = super::paints_waveform(app);
    let own = (!painted).then(|| {
        let values = id.as_deref().and_then(|id| outline(id, cx));
        let progress = if duration > 0. {
            (app.player.position() / duration) as f32
        } else {
            0.
        };
        let colors = colors(c);
        canvas(
            |_, _, _| (),
            move |b, (), window, _| paint(b, values.as_deref(), progress, colors, window),
        )
        .size_full()
    });
    div()
        .id("waveform")
        .relative()
        .w(width)
        .h(height)
        .flex_none()
        .cursor_pointer()
        .child(
            canvas(move |b, _, _| seen.set(Some(b)), |_, _, _, _| ())
                .absolute()
                .inset_0(),
        )
        .children(own)
        .when(painted, |d| d.child(slot(Slot::Waveform)))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, e: &MouseDownEvent, _, cx| {
                let Some(b) = bounds.get() else { return };
                let at = ((e.position.x - b.left()) / b.size.width).clamp(0., 1.);
                this.seek_to(f64::from(at) * duration, cx);
            }),
        )
}

/// Asks for the playing song's outline (for the player bar's seek bar).
pub(super) fn ensure(app: &MusicApp, cx: &mut Context<MusicApp>) {
    let playback = &app.player.playback;
    let Some(track) = app.player.current() else {
        return;
    };
    // Looked at without `default_global`, which would wake the global's
    // observers on every frame.
    let known = cx.try_global::<Waveforms>().is_some_and(|w| {
        w.loading.as_deref() == Some(track.video_id.as_str())
            || w.shown
                .as_ref()
                .is_some_and(|(id, _)| *id == track.video_id)
    });
    if !known && playback.playing && !playback.loading {
        request(app, &track.video_id, cx);
    }
}

/// The outline of `id`, once decoded.
pub(super) fn outline(id: &str, cx: &App) -> Option<Vec<f32>> {
    let (shown, values) = cx.try_global::<Waveforms>()?.shown.as_ref()?;
    (shown == id).then(|| values.clone())
}

pub(super) fn has_outline(id: &str, cx: &App) -> bool {
    cx.try_global::<Waveforms>()
        .and_then(|w| w.shown.as_ref())
        .is_some_and(|(shown, _)| shown == id)
}

/// Starts decoding `id` unless it is shown or on its way.
fn request(app: &MusicApp, id: &str, cx: &mut Context<MusicApp>) {
    let state = cx.default_global::<Waveforms>();
    let shown = state.shown.as_ref().is_some_and(|(s, _)| s == id);
    if shown || state.loading.as_deref() == Some(id) {
        return;
    }
    // The stream that plays (resolved, or the fake stream's file).
    let Some(url) = app.backend.live().and_then(|b| b.stream_url(id)) else {
        return;
    };
    state.loading = Some(id.to_owned());
    let cache = app.paths.cache.clone();
    let id = id.to_owned();
    cx.spawn(async move |this, cx| {
        let job = id.clone();
        let result = cx
            .background_spawn(async move { encore_visuals::waveform::load(&url, &cache, &job) })
            .await;
        let _ = this.update(cx, |_, cx| {
            let state = cx.default_global::<Waveforms>();
            if state.loading.as_deref() == Some(id.as_str()) {
                state.loading = None;
            }
            match result {
                Ok(values) => state.shown = Some((id, values)),
                Err(e) => log::warn!("visuals: no waveform for {id}: {e:#}"),
            }
            cx.notify();
        });
    })
    .detach();
}

/// The played and the unplayed bars' colours.
pub(super) fn colors(c: &Colors) -> (Hsla, Hsla) {
    (c.signal, c.text.opacity(0.24))
}

pub(super) fn paint(
    bounds: Bounds<Pixels>,
    values: Option<&[f32]>,
    progress: f32,
    (played, rest): (Hsla, Hsla),
    window: &mut Window,
) {
    let centre = bounds.center().y;
    let Some(values) = values else {
        // Not decoded yet: a faint line where it will be.
        let line = Bounds::new(
            point(bounds.left(), centre - px(1.)),
            size(bounds.size.width, px(2.)),
        );
        window.paint_quad(fill(line, rest).corner_radii(Corners::all(px(1.))));
        return;
    };
    let n = values.len().max(1) as f32;
    let step = bounds.size.width / n;
    let width = (step * 0.6).max(px(1.));
    for (i, value) in values.iter().enumerate() {
        let height = (bounds.size.height * *value).max(width);
        let x = bounds.left() + step * i as f32 + (step - width) / 2.;
        let bar = Bounds::new(point(x, centre - height / 2.), size(width, height));
        let color = if (i as f32 + 0.5) / n <= progress {
            played
        } else {
            rest
        };
        window.paint_quad(fill(bar, color).corner_radii(Corners::all(width / 2.)));
    }
}
