//! The equalizer, the sleep timer, loudness levelling and smooth mixes: each
//! sent to the backend, which applies and saves it; the views show the
//! state the backend reports, or an edit made here until a report catches up.

use std::time::{Duration, Instant};

use gpui_kit::*;
use ytfast::backend::Command;
use ytfast::equalizer::{Equalizer, Preset};
use ytfast::model::{Mixes, Sleep};

use crate::app::MusicApp;

/// An edit here is trusted over playback reports this long.
const EDIT_GRACE: Duration = Duration::from_millis(1200);

impl MusicApp {
    /// The equalizer as shown: an edit in progress, or the backend's.
    pub fn equalizer(&self) -> Equalizer {
        match &self.extras.eq_edit {
            Some((eq, at)) if at.elapsed() < EDIT_GRACE || self.extras.eq_band.is_some() => {
                eq.clone()
            }
            _ => self.player.playback.equalizer.clone(),
        }
    }

    /// Applies `equalizer` live; the backend saves it once edits stop.
    pub fn set_equalizer(&mut self, equalizer: Equalizer, cx: &mut Context<Self>) {
        if equalizer == self.equalizer() {
            return;
        }
        log::info!(
            "equalizer: {} {} [{}]",
            equalizer.preset.label(),
            if equalizer.enabled { "on" } else { "off" },
            equalizer
                .gains
                .iter()
                .map(|g| format!("{g:+.1}"))
                .collect::<Vec<_>>()
                .join(" ")
        );
        self.extras.eq_edit = Some((equalizer.clone(), Instant::now()));
        self.send(Command::Equalizer(equalizer));
        cx.notify();
    }

    /// Picks a preset (turning the equalizer on).
    pub(crate) fn eq_preset(&mut self, preset: Preset, cx: &mut Context<Self>) {
        let eq = self.equalizer().with_preset(preset);
        self.set_equalizer(eq, cx);
    }

    /// Play anything's `eq <preset>`: a preset by the start of its name
    /// ("bass", "late"), or `on`/`off`. False when nothing matches.
    #[allow(dead_code, reason = "Play anything (M4) calls it")]
    pub(crate) fn eq_command(&mut self, words: &str, cx: &mut Context<Self>) -> bool {
        let words = words.trim().to_lowercase();
        let eq = self.equalizer();
        let enabled = match words.as_str() {
            "on" => Some(true),
            "off" | "bypass" => Some(false),
            _ => None,
        };
        if let Some(enabled) = enabled {
            self.set_equalizer(Equalizer { enabled, ..eq }, cx);
            return true;
        }
        let preset = Preset::ALL.into_iter().find(|p| {
            let label = p.label().to_lowercase();
            !words.is_empty() && (label.starts_with(&words) || label.replace(' ', "") == words)
        });
        match preset {
            Some(preset) => {
                self.eq_preset(preset, cx);
                true
            }
            None => false,
        }
    }

    /// Sets (`Some`) or cancels the sleep timer.
    pub(crate) fn sleep(&mut self, choice: Option<Sleep>, cx: &mut Context<Self>) {
        log::info!("sleep timer set: {choice:?}");
        self.extras.sleep_open = false;
        self.send(Command::SleepTimer(choice));
        cx.notify();
    }

    /// Play anything's `sleep 30`, `sleep end` or `sleep off`. False when
    /// the words aren't a timer.
    #[allow(dead_code, reason = "Play anything (M4) calls it")]
    pub(crate) fn sleep_command(&mut self, words: &str, cx: &mut Context<Self>) -> bool {
        let words = words.trim().to_lowercase();
        let choice = match words.as_str() {
            "end" | "end of song" => Some(Sleep::EndOfSong),
            "off" | "cancel" => None,
            _ => match words.trim_end_matches(['m', ' ']).parse::<u32>() {
                Ok(minutes) if minutes > 0 => Some(Sleep::Minutes(minutes)),
                _ => return false,
            },
        };
        self.sleep(choice, cx);
        true
    }

    /// Loudness levelling between songs on or off (saved by the backend).
    pub(crate) fn set_normalize(&mut self, on: bool, cx: &mut Context<Self>) {
        log::info!("loudness levelling {on}");
        self.player.playback.normalize = on;
        self.send(Command::Normalize(on));
        cx.notify();
    }

    /// Smooth mixes on or off and their length (saved by the backend).
    pub(crate) fn set_mixes(&mut self, mixes: Mixes, cx: &mut Context<Self>) {
        let mixes = Mixes {
            seconds: mixes.seconds.clamp(Mixes::SHORTEST, Mixes::LONGEST),
            ..mixes
        };
        log::info!("smooth mixes {} ({} s)", mixes.on, mixes.seconds);
        self.player.playback.mixes = mixes;
        self.send(Command::Mixes(mixes));
        cx.notify();
    }
}
