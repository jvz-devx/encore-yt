//! M29: a song menu's Add to playlist opens a submenu beside it: the
//! account's playlists from Library (the first few), New playlist, and All
//! playlists for the full picker with its search. → or a hover opens it,
//! ← or Esc closes it.

use gpui_kit::assets::IconName;
use gpui_kit::*;
use ytfast::account::{AccountAction, Dialog};
use ytfast::model::Track;

use super::menu::{Subject, entries};
use crate::app::MusicApp;
use crate::nav::LibraryTab;

/// Playlists listed in the submenu before All playlists.
const SHOWN: usize = 8;

/// What a submenu entry does.
#[derive(Clone, Debug)]
pub enum SubDoes {
    Add(String),
    New,
    All,
}

pub struct SubEntry {
    pub icon: IconName,
    pub label: SharedString,
    pub does: SubDoes,
    /// A line goes above the first entry of a new section.
    pub section: u8,
}

/// The open submenu: which entry it hangs from and its own highlight.
pub struct Sub {
    pub parent: usize,
    pub selected: Option<usize>,
}

/// Add to playlist's entries: playlists while Library's are known, then
/// New playlist and All playlists.
pub fn sub_entries(app: &MusicApp) -> Vec<SubEntry> {
    let playlists = app.playlist_matches().unwrap_or_default();
    let mut out: Vec<SubEntry> = playlists
        .into_iter()
        .take(SHOWN)
        .map(|p| SubEntry {
            icon: IconName::ListMusic,
            label: p.title.into(),
            does: SubDoes::Add(p.id),
            section: 0,
        })
        .collect();
    out.push(SubEntry {
        icon: IconName::Plus,
        label: "New playlist".into(),
        does: SubDoes::New,
        section: 1,
    });
    out.push(SubEntry {
        icon: IconName::ListPlus,
        label: "All playlists".into(),
        does: SubDoes::All,
        section: 1,
    });
    out
}

impl MusicApp {
    /// Whether the open menu's entry `i` opens a submenu.
    pub fn opens_sub(&self, i: usize) -> bool {
        let Some(menu) = &self.desktop.layers.menu else {
            return false;
        };
        entries(self, &menu.subject)
            .get(i)
            .is_some_and(|e| e.opens_sub())
    }

    /// Opens the submenu of entry `parent`, its first entry highlighted
    /// when the keyboard opened it.
    pub fn open_sub(&mut self, parent: usize, keyboard: bool, cx: &mut Context<Self>) {
        self.ensure_page(LibraryTab::Playlists.target(), false);
        if let Some(menu) = &mut self.desktop.layers.menu {
            menu.selected = Some(parent);
            let same = menu.sub.as_ref().is_some_and(|s| s.parent == parent);
            if !same || keyboard {
                menu.sub = Some(Sub {
                    parent,
                    selected: keyboard.then_some(0),
                });
            }
            cx.notify();
        }
    }

    /// Closes the submenu; its parent keeps the highlight. False when none
    /// was open.
    pub fn close_sub(&mut self, cx: &mut Context<Self>) -> bool {
        let closed = self
            .desktop
            .layers
            .menu
            .as_mut()
            .is_some_and(|m| m.sub.take().is_some());
        if closed {
            cx.notify();
        }
        closed
    }

    /// The pointer is on submenu entry `i`.
    pub fn hover_in_sub(&mut self, i: usize, cx: &mut Context<Self>) {
        if let Some(sub) = self
            .desktop
            .layers
            .menu
            .as_mut()
            .and_then(|m| m.sub.as_mut())
            && sub.selected != Some(i)
        {
            sub.selected = Some(i);
            cx.notify();
        }
    }

    /// Runs submenu entry `i` (the highlighted one for `None`) and closes
    /// the menu.
    pub fn choose_in_sub(&mut self, i: Option<usize>, window: &mut Window, cx: &mut Context<Self>) {
        let Some(menu) = &self.desktop.layers.menu else {
            return;
        };
        let Some(i) = i.or(menu.sub.as_ref().and_then(|s| s.selected)) else {
            return;
        };
        let Subject::Song { track, .. } = menu.subject.clone() else {
            return;
        };
        let Some(chosen) = sub_entries(self).into_iter().nth(i) else {
            return;
        };
        self.close_menu(window, cx);
        log::info!("menu: Add to playlist → {}", chosen.label);
        add(self, chosen.does, track, window, cx);
    }
}

fn add(
    app: &mut MusicApp,
    does: SubDoes,
    track: Track,
    window: &mut Window,
    cx: &mut Context<MusicApp>,
) {
    match does {
        SubDoes::Add(playlist_id) => app.account_act(
            AccountAction::Add {
                playlist_id,
                tracks: vec![track],
            },
            cx,
        ),
        SubDoes::New => {
            let dialog = Dialog::NewPlaylist {
                title: String::new(),
                description: String::new(),
                tracks: vec![track],
            };
            app.open_account_dialog(dialog, window, cx);
        }
        SubDoes::All => crate::account::add_to_playlist(app, vec![track], window, cx),
    }
}
