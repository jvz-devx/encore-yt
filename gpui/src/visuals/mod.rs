//! M8 visuals spike: wgpu effects inside the GPUI window, and audio analysis
//! of what mpv plays. Findings and numbers: gpui/NOTES-visuals.md.
//!
//! Hidden behind `YTFAST_GPUI_VISUALS_SPIKE=1`, which replaces the page area
//! with [`spike::VisualsSpike`] once a song plays: the animated cover backdrop (rendered by our
//! own wgpu device in `gpu`, shown as a GPUI image), 32 spectrum bands from
//! mpv's PipeWire stream (`spectrum`) and the song's waveform (`waveform`).
//! `YTFAST_GPUI_VISUALS_RES=WxH` sets the backdrop's render size (default
//! 640x360; GPUI scales it to the page), `YTFAST_GPUI_VISUALS_FPS=N` its
//! frame rate (default 60; 0 for every display frame).

mod gpu;
mod pipewire;
mod spectrum;
mod spike;
mod waveform;

use gpui_kit::*;

use crate::app::MusicApp;

struct Spike(Entity<spike::VisualsSpike>);

impl Global for Spike {}

/// The spike in place of the page, when `YTFAST_GPUI_VISUALS_SPIKE=1`.
pub fn page(app: &MusicApp, cx: &mut Context<MusicApp>) -> Option<AnyElement> {
    if std::env::var_os("YTFAST_GPUI_VISUALS_SPIKE").is_none_or(|v| v != "1") {
        return None;
    }
    let playback = &app.player.playback;
    let entity = match cx.try_global::<Spike>() {
        Some(spike) => spike.0.clone(),
        // The page stays until a song plays, so one can be picked.
        None if !playback.playing => return None,
        None => {
            let socket = app.paths.runtime.join("mpv.sock");
            let entity = cx.new(|_| spike::VisualsSpike::new(socket));
            cx.set_global(Spike(entity.clone()));
            entity
        }
    };
    let track = playback.index.and_then(|i| app.player.queue.get(i));
    let input = spike::Input {
        video_id: track.map(|t| t.video_id.clone()),
        cover: track.and_then(|t| t.thumbnail.clone()),
        playing: playback.playing && !playback.loading,
        position: playback.position,
        duration: playback.duration,
    };
    entity.update(cx, |spike, _| spike.input = input);
    Some(entity.into_any_element())
}
