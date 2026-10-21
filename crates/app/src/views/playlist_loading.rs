//! Playlist metadata progress, separate from the audio transport's buffering.

use encore_core::backend::Command;
use encore_core::model::PlaylistLoad;
use gpui_kit::assets::IconName;
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::*;

use crate::app::MusicApp;
use crate::theme::{self, Type, radius, size, space};

use super::widgets::{self, Pill};

fn counts(load: &PlaylistLoad) -> Vec<String> {
    let mut parts = Vec::new();
    if let Some(counts) = &load.counts {
        if counts.unavailable > 0 {
            parts.push(format!("{} unavailable", counts.unavailable));
        }
        if counts.unparsed > 0 {
            parts.push(format!("{} couldn't be read", counts.unparsed));
        }
        if counts.overlapping_entries > 0 {
            parts.push(format!(
                "{} overlapping entries skipped",
                counts.overlapping_entries
            ));
        }
        if load.complete
            && counts.expected.is_some_and(|total| {
                counts.rows.saturating_sub(counts.overlapping_entries) != total
            })
        {
            parts.push(format!(
                "{} listed by YouTube",
                counts.expected.unwrap_or(0)
            ));
        }
    }
    parts
}

pub(super) fn summary(load: &PlaylistLoad) -> String {
    let state = if load.cached { "cached" } else { "ready" };
    let mut parts = vec![format!("{} songs {state}", load.entries)];
    if load.unique_tracks != load.entries {
        parts.push(format!("{} unique", load.unique_tracks));
    }
    parts.extend(counts(load));
    parts.join(" · ")
}

fn progress(load: &PlaylistLoad) -> String {
    let checked = load.counts.as_ref().map_or(0, |counts| {
        counts.rows.saturating_sub(counts.overlapping_entries)
    });
    let mut message = match load.counts.as_ref().and_then(|counts| counts.expected) {
        Some(total) => format!("Loading playlist… {checked} of {total} entries checked"),
        None if checked > 0 => format!("Loading playlist… {checked} entries checked"),
        None => "Loading playlist…".into(),
    };
    let details = counts(load);
    if !details.is_empty() {
        message.push_str(&format!(" · {}", details.join(" · ")));
    }
    message
}

pub(super) fn banner(app: &MusicApp, cx: &mut Context<MusicApp>) -> Option<impl IntoElement> {
    let load = app
        .player
        .playback
        .playlist_load
        .as_ref()
        .filter(|load| !load.complete)?;
    let c = theme::colors(cx);
    let text = progress(load);
    let label = text.clone();
    Some(h_flex()
        .debug_selector(|| "playlist-loading".into())
        .mx(size::GUTTER).mb(space::MD).px(space::MD).py(space::SM)
        .gap(space::MD).rounded(radius::MD).bg(c.raised)
        .child(widgets::icon(IconName::ListMusic, size::ICON_SM, c.text_muted))
        .child(v_flex().flex_1().min_w_0()
            .child(div().type_small().tabular().debug_selector(move || format!("playlist-progress:{label}")).child(text))
            .child(div().type_small().text_color(c.text_muted).child("The current song can keep playing. Shuffle starts when the complete list is ready.")))
        .child(widgets::pill_button("cancel-playlist-loading", "Cancel", None, Pill::Secondary, &c)
            .debug_selector(|| "cancel-playlist-loading".into())
            .on_click(cx.listener(|this, _, _, _| this.send(Command::CancelPlaylistLoad)))))
}

#[cfg(test)]
mod tests {
    use super::summary;
    use encore_core::model::{PlaylistCounts, PlaylistLoad};

    #[test]
    fn summaries_do_not_claim_older_caches_have_known_unavailable_counts() {
        let cached = PlaylistLoad {
            entries: 1246,
            unique_tracks: 1228,
            cached: true,
            complete: true,
            ..Default::default()
        };
        assert_eq!(summary(&cached), "1246 songs cached · 1228 unique");
        let diagnosed = PlaylistLoad {
            cached: false,
            counts: Some(PlaylistCounts {
                rows: 1282,
                unavailable: 36,
                expected: Some(1282),
                ..Default::default()
            }),
            ..cached
        };
        assert_eq!(
            summary(&diagnosed),
            "1246 songs ready · 1228 unique · 36 unavailable"
        );
    }
}
