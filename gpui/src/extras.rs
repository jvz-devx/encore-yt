//! M6: equalizer, sleep timer, loudness levelling, smooth mixes, audition,
//! the most-replayed ridge, Stage and the mini player.

use std::collections::HashMap;

use gpui_kit::*;
use ytfast::heat::Heat;

use crate::app::MusicApp;

pub struct Extras {
    /// Most-replayed heat by video id, asked once per song.
    pub heat: HashMap<String, Option<Heat>>,
}

impl Extras {
    pub fn new(_window: &mut Window, _cx: &mut Context<MusicApp>) -> (Self, Vec<Subscription>) {
        (
            Self {
                heat: HashMap::new(),
            },
            Vec::new(),
        )
    }
}

impl MusicApp {
    pub(crate) fn on_heat(&mut self, id: String, heat: Option<Heat>) {
        self.extras.heat.insert(id, heat);
    }
}
