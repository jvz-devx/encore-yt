//! Which audio engine plays a song: the one chosen (`Kind`, from
//! `YTFAST_PLAYER` or the setting), except that songs the Rust engine can't
//! play go to mpv: formats it has no decoder for (HE-AAC, itag 139) and
//! songs it failed on during this run. Each is logged once per song.
//!
//! A deck plays one engine, so a song on the other engine than the main
//! deck's starts as a new song (no gapless change); Smooth mixes and
//! Audition start their deck on the song's engine.

use super::*;

impl super::Worker {
    /// The engine for `video_id` in format `itag`.
    pub(super) fn engine_for(&mut self, video_id: &str, itag: u32) -> Kind {
        if self.player != Kind::Rust || self.fallbacks.contains(video_id) {
            return if self.player == Kind::Rust {
                Kind::Mpv
            } else {
                self.player
            };
        }
        if Kind::Rust.plays(itag) {
            return Kind::Rust;
        }
        log::info!(
            "{video_id} plays on mpv: the Rust player has no decoder for {}",
            resolver::describe(itag)
        );
        self.remember_fallback(video_id);
        Kind::Mpv
    }

    /// The Rust engine failed on `video_id`: it plays on mpv from now on.
    pub(super) fn rust_failed(&mut self, video_id: &str, error: &str) {
        if !self.fallbacks.contains(video_id) {
            log::warn!("{video_id} plays on mpv after the Rust player failed: {error}");
            self.remember_fallback(video_id);
        }
    }

    /// The main deck's failure on the current song, when it is the Rust
    /// engine's: the song's retry goes to mpv.
    pub(super) fn main_deck_failed(&mut self, error: &str) {
        if self.mpv.as_ref().is_some_and(|m| m.kind() == Kind::Rust)
            && let Some(id) = self.current().map(|t| t.video_id.clone())
        {
            self.rust_failed(&id, error);
        }
    }

    fn remember_fallback(&mut self, video_id: &str) {
        if self.fallbacks.len() > 500 {
            self.fallbacks.clear();
        }
        self.fallbacks.insert(video_id.to_owned());
    }

    /// Settings: the engine new songs start on (saved for next time).
    pub(super) fn set_player(&mut self, kind: Kind) {
        let kind = if kind.available() { kind } else { Kind::Mpv };
        log::info!("audio player: {}", kind.label());
        self.player = kind;
        self.state.player = kind;
        self.update_settings(|s| s.player = Some(kind));
        self.emit(true);
    }
}
