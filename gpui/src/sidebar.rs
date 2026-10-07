//! The sidebar's library: the account's playlists (Library → Playlists,
//! read from the page cache), the collections started here lately, and the
//! one playing now. Drawn by `views::sidebar`.

mod recent;

use std::path::PathBuf;

use gpui_kit::*;
use ytfast::backend::Command;
use ytfast::model::{Item, ItemKind, Run, Shelf, ShelfStyle, Target};

use crate::app::MusicApp;
use crate::nav::{LibraryTab, View};

pub use recent::playlist_id;

/// Liked music's page.
pub const LIKED: &str = "VLLM";

pub struct Sidebar {
    /// Albums, playlists and radios started from this app, newest first.
    pub recent: Vec<Item>,
    /// The playlist id of the collection playing now, while the queue is
    /// one that a `PlayTarget` started.
    pub playing_from: Option<String>,
    /// `recent.json` in the cache directory.
    path: PathBuf,
}

impl Sidebar {
    pub fn new(cache: &std::path::Path) -> Self {
        let path = cache.join("recent.json");
        Self {
            recent: recent::load(&path),
            playing_from: None,
            path,
        }
    }

    fn remember(&mut self, item: Item) {
        recent::push(&mut self.recent, item);
        recent::save(&self.path, &self.recent);
    }
}

impl MusicApp {
    /// Every command passes through here on its way to the backend:
    /// collections that start playing go to the top of Recently played.
    pub(crate) fn sidebar_sent(&mut self, command: &Command) {
        match command {
            Command::PlayTarget(target) => {
                self.sidebar.playing_from = playlist_id(target).map(str::to_string);
                if let Some(item) = recent::describe(self, target) {
                    self.sidebar.remember(item);
                }
            }
            Command::PlayTracks { .. } => self.sidebar.playing_from = None,
            _ => {}
        }
    }

    /// The account changed (or was first known): signed in, the sidebar
    /// lists Library → Playlists; signed out, Explore's shortcuts.
    pub(crate) fn sidebar_account(&mut self, changed: bool) {
        let target = if self.account.signed_in() {
            LibraryTab::Playlists.target()
        } else {
            View::Explore.target()
        };
        // The page in view was just asked for again (account.rs).
        let force = changed && self.pages.view.target() != target;
        self.ensure_page(target, force);
    }

    /// The account's playlists with Liked music first, as YouTube Music's
    /// sidebar lists them; `None` while they're loading for the first time.
    /// Library → Playlists doesn't always list Liked music, so it is added
    /// when missing (with its cover from any page that showed it).
    pub fn library_playlists(&self) -> Option<Vec<Item>> {
        let state = self
            .pages
            .states
            .get(&LibraryTab::Playlists.target().key())?;
        let page = state.page.as_ref()?;
        let (liked, rest): (Vec<&Item>, Vec<&Item>) = page
            .shelves
            .iter()
            .flat_map(|s| s.items.iter())
            .filter(|i| i.target.is_some())
            .partition(|i| is_liked(i));
        let liked = liked
            .first()
            .map(|i| (*i).clone())
            .unwrap_or_else(|| self.liked_music());
        Some(
            std::iter::once(liked)
                .chain(rest.into_iter().cloned())
                .collect(),
        )
    }

    fn liked_music(&self) -> Item {
        let seen = self
            .pages
            .states
            .values()
            .filter_map(|s| s.page.as_ref())
            .flat_map(|p| p.shelves.iter().flat_map(|s| s.items.iter()))
            .find(|i| is_liked(i) && i.thumbnail.is_some());
        Item {
            kind: ItemKind::Playlist,
            title: "Liked music".into(),
            subtitle: vec![Run {
                text: "Auto playlist".into(),
                target: None,
            }],
            thumbnail: seen.and_then(|i| i.thumbnail.clone()),
            target: Some(Target::browse(LIKED)),
            play: Some(Target::Watch {
                video_id: None,
                playlist_id: Some("LM".into()),
                params: None,
            }),
            track: None,
            index: None,
            stripe: None,
            editable: None,
        }
    }

    /// Explore's own buttons (New releases, Charts, Moods & genres), or
    /// the same three before Explore has loaded.
    pub fn explore_shortcuts(&self) -> Vec<(String, Target)> {
        let buttons = self
            .pages
            .states
            .get(&View::Explore.target().key())
            .and_then(|s| s.page.as_ref())
            .and_then(|p| p.shelves.iter().find(|s| s.style == ShelfStyle::Buttons));
        match buttons {
            Some(shelf) => shortcuts(shelf),
            None => [
                ("New releases", "FEmusic_new_releases"),
                ("Charts", "FEmusic_charts"),
                ("Moods & genres", "FEmusic_moods_and_genres"),
            ]
            .into_iter()
            .map(|(label, id)| (label.to_string(), Target::browse(id)))
            .collect(),
        }
    }
}

fn is_liked(item: &Item) -> bool {
    matches!(&item.target, Some(Target::Browse { id, .. }) if id == LIKED)
}

fn shortcuts(shelf: &Shelf) -> Vec<(String, Target)> {
    shelf
        .items
        .iter()
        .filter_map(|i| Some((i.title.clone(), i.target.clone()?)))
        .collect()
}

impl MusicApp {
    /// A sidebar row chosen: its page opens, or a radio plays again.
    pub fn choose_library_item(&mut self, item: &Item, cx: &mut Context<Self>) {
        match (&item.target, &item.play) {
            (Some(target), _) => self.activate(target.clone(), cx),
            (None, Some(play)) => {
                self.send(Command::PlayTarget(play.clone()));
                self.sidebar.remember(item.clone());
                cx.notify();
            }
            (None, None) => {}
        }
    }

    /// The sidebar's New playlist.
    ///
    /// HOOK(M3 create playlist): once `crate::account` has the New playlist
    /// dialog (`open_account_dialog(Dialog::NewPlaylist { .. })`, as Library
    /// → Playlists' button opens it), open it here instead of the notice.
    pub fn sidebar_new_playlist(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        self.notice("New playlist isn't available yet", cx);
    }
}

impl MusicApp {
    /// A playlist page's Play when it has no play target of its own (Liked
    /// music): its songs went to the queue, and the page to Recently played.
    pub(crate) fn sidebar_played_page(&mut self, key: &str) {
        let Some(item) = recent::from_page(self, key) else {
            return;
        };
        self.sidebar.playing_from = item.play.as_ref().and_then(playlist_id).map(str::to_string);
        self.sidebar.remember(item);
    }
}
