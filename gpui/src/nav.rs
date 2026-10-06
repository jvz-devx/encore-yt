//! Where the app is: the view, and the state of each page it has asked for.
//!
//! Mirrors `View` and `PageState` in the egui app (src/app.rs), which live
//! behind its feature. Keep the two in step until they move into the shared
//! backend crate.

use std::time::Instant;

use ytfast::model::{Page, Target};

// The Library tab chips arrive with M3 (docs/gpui/PLAN.md).
#[allow(dead_code)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LibraryTab {
    Playlists,
    Songs,
    Albums,
    Artists,
    History,
}

impl LibraryTab {
    pub fn target(self) -> Target {
        Target::browse(match self {
            LibraryTab::Playlists => "FEmusic_liked_playlists",
            LibraryTab::Songs => "FEmusic_liked_videos",
            LibraryTab::Albums => "FEmusic_liked_albums",
            LibraryTab::Artists => "FEmusic_library_corpus_track_artists",
            LibraryTab::History => "FEmusic_history",
        })
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum View {
    Home,
    Explore,
    Library(LibraryTab),
    /// Album, artist, playlist, mood, chart, search results…
    Page(Target),
}

impl View {
    pub fn target(&self) -> Target {
        match self {
            View::Home => Target::browse("FEmusic_home"),
            View::Explore => Target::browse("FEmusic_explore"),
            View::Library(tab) => tab.target(),
            View::Page(target) => target.clone(),
        }
    }

    /// Where a target leads: a page to open, or `None` for playback targets.
    pub fn for_target(target: &Target) -> Option<View> {
        match target {
            Target::Browse { id, params: None } if id == "FEmusic_home" => Some(View::Home),
            Target::Browse { id, params: None } if id == "FEmusic_explore" => Some(View::Explore),
            Target::Watch { .. } => None,
            other => Some(View::Page(other.clone())),
        }
    }
}

pub struct PageState {
    pub page: Option<Page>,
    pub loading: bool,
    /// The page shown is the saved copy and a refresh is under way or failed.
    pub cached: bool,
    pub error: Option<String>,
    pub fetched: Option<Instant>,
    /// The newest request for this page; older answers are ignored.
    pub seq: u64,
}
