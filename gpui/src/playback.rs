//! M2: the queue and where playback is, plus the player bar's controls.

use std::time::Instant;

use gpui_kit::component::slider::{SliderEvent, SliderState};
use gpui_kit::*;
use ytfast::backend::Command;
use ytfast::model::{Lyrics, Playback, Track};

use crate::app::MusicApp;

/// The seek slider's scale: per mille of the song.
pub const SEEK_SCALE: f32 = 1000.0;

pub struct Player {
    pub queue: Vec<Track>,
    pub playback: Playback,
    /// When `playback` arrived; the shown position moves on from there.
    playback_at: Instant,
    pub seek: Entity<SliderState>,
    pub volume: Entity<SliderState>,
    /// The seek slider is held: playback updates don't move it.
    seeking: bool,
    /// The volume slider shows the restored volume (set once, then the
    /// slider leads).
    volume_synced: bool,
    /// Now Playing fills the page area.
    pub now_playing: bool,
    /// The Up next panel is open beside the page.
    pub queue_open: bool,
}

impl Player {
    pub fn new(_window: &mut Window, cx: &mut Context<MusicApp>) -> (Self, Vec<Subscription>) {
        let seek = cx.new(|_| SliderState::new().min(0.).max(SEEK_SCALE));
        let volume = cx.new(|_| SliderState::new().min(0.).max(100.).default_value(100.));
        let subscriptions = vec![
            cx.subscribe(&seek, |this, _, event: &SliderEvent, _| match event {
                SliderEvent::Change(_) => this.player.seeking = true,
                SliderEvent::Release(value) => {
                    this.player.seeking = false;
                    let to = f64::from(value.start() / SEEK_SCALE) * this.player.playback.duration;
                    this.send(Command::Seek(to));
                }
            }),
            cx.subscribe(&volume, |this, _, event: &SliderEvent, _| {
                let (SliderEvent::Change(value) | SliderEvent::Release(value)) = event;
                this.send(Command::Volume(f64::from(value.start())));
            }),
        ];
        (
            Self {
                queue: Vec::new(),
                playback: Playback::default(),
                playback_at: Instant::now(),
                seek,
                volume,
                seeking: false,
                volume_synced: false,
                now_playing: false,
                queue_open: false,
            },
            subscriptions,
        )
    }

    /// Where the song is now: the last reported position, moved on by the
    /// time since while it plays.
    pub fn position(&self) -> f64 {
        let mut position = self.playback.position;
        if self.playback.playing && !self.playback.loading {
            position += self.playback_at.elapsed().as_secs_f64();
        }
        if self.playback.duration > 0.0 {
            position = position.min(self.playback.duration);
        }
        position
    }

    pub fn current(&self) -> Option<&Track> {
        self.playback.index.and_then(|i| self.queue.get(i))
    }
}

impl MusicApp {
    pub(crate) fn on_queue(&mut self, queue: Vec<Track>) {
        self.player.queue = queue;
    }

    pub(crate) fn on_playback(
        &mut self,
        playback: Playback,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let player = &mut self.player;
        if !player.seeking && playback.duration > 0.0 {
            let at = (playback.position / playback.duration) as f32 * SEEK_SCALE;
            player
                .seek
                .update(cx, |slider, cx| slider.set_value(at, window, cx));
        }
        if !player.volume_synced {
            player.volume_synced = true;
            let volume = playback.volume as f32;
            player
                .volume
                .update(cx, |slider, cx| slider.set_value(volume, window, cx));
        }
        player.playback = playback;
        player.playback_at = Instant::now();
    }

    pub(crate) fn on_lyrics(&mut self, _id: String, _result: Result<Option<Lyrics>, String>) {
        // M2: Now Playing lyrics.
    }
}

/// Shortcuts for this area, in the "Music" key context.
pub fn bind_keys(_cx: &mut App) {}

/// Handlers for this area's actions, on the window's root element.
pub fn on_actions(root: Div, _cx: &mut Context<MusicApp>) -> Div {
    root
}
