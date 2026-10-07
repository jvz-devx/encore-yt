//! Most replayed: YouTube's replay heat for the playing song, asked once per
//! song, drawn as the seek bar's ridge; P jumps to its peak.

use encore_core::backend::Command;
use encore_core::heat::Heat;
use gpui_kit::*;

use crate::app::MusicApp;

impl MusicApp {
    /// Asks once for the playing song's heat.
    pub(super) fn request_heat(&mut self) {
        let Some(id) = self.player.current().map(|t| t.video_id.clone()) else {
            return;
        };
        if self.extras.heat_requested.insert(id.clone()) {
            log::info!("most replayed: asking for {id}");
            self.send(Command::Heat(id));
        }
    }

    pub(crate) fn on_heat(&mut self, id: String, heat: Option<Heat>) {
        match heat
            .as_ref()
            .and_then(|h| h.peak.map(|p| (h.markers.len(), p)))
        {
            Some((markers, peak)) => log::info!(
                "most replayed for {id}: {markers} markers, peak at {:.1}s ({:.1}s to {:.1}s)",
                peak.at,
                peak.start,
                peak.end
            ),
            None => log::info!("most replayed for {id}: none"),
        }
        self.extras.heat.insert(id, heat);
    }

    /// The playing song's heat, once known (and if it has any).
    pub fn current_heat(&self) -> Option<&Heat> {
        let id = &self.player.current()?.video_id;
        self.extras.heat.get(id)?.as_ref()
    }

    /// P: to the start of the most replayed part.
    pub fn jump_to_peak(&mut self, cx: &mut Context<Self>) {
        let Some(peak) = self.current_heat().and_then(|h| h.peak) else {
            log::info!("jump to the most replayed part: not known for this song");
            return;
        };
        log::info!(
            "jump to the most replayed part: {:.1}s (peak {:.1}s)",
            peak.start,
            peak.at
        );
        self.seek_to(peak.start, cx);
    }
}
