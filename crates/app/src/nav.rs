//! Where the app is: the view, and the state of each page it has asked for.

use std::collections::HashSet;
use std::time::Instant;

use encore_core::model::{Page, Target};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LibraryTab {
    Playlists,
    Songs,
    Albums,
    Artists,
    /// What the account played, by day.
    History,
}

impl LibraryTab {
    pub const ALL: [LibraryTab; 5] = [
        LibraryTab::Playlists,
        LibraryTab::Songs,
        LibraryTab::Albums,
        LibraryTab::Artists,
        LibraryTab::History,
    ];

    pub fn label(self) -> &'static str {
        match self {
            LibraryTab::Playlists => "Playlists",
            LibraryTab::Songs => "Songs",
            LibraryTab::Albums => "Albums",
            LibraryTab::Artists => "Artists",
            LibraryTab::History => "History",
        }
    }

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
    /// What fetches this page (the account logic refetches through it).
    pub target: Target,
    pub page: Option<Page>,
    pub loading: bool,
    /// The page shown is the saved copy and a refresh is under way or failed.
    pub cached: bool,
    pub error: Option<String>,
    /// Continuations in flight: `None` for the page, `Some(i)` for shelf i.
    pub more_loading: HashSet<Option<usize>>,
    pub fetched: Option<Instant>,
    /// The newest request for this page; older answers are ignored.
    pub seq: u64,
    /// A chip's in-place reload in flight: its continuation and chip index.
    pub reload: Option<(String, usize)>,
}
