//! Context menus: right-clicking a song, album,
//! playlist or artist (or its ⋮ button) opens its actions. This is the
//! menu's state and what its entries do; `views::menu` draws it.
//!
//! The open menu holds the keyboard (`MusicMenu` key context, keys in
//! `menu_keys`): the arrows and Tab move, Home/End and a first letter jump,
//! Enter or Space chooses, → and ← open and close Add to playlist's
//! submenu (`submenu`), Esc closes and gives the keyboard back. Album,
//! playlist and artist pages start loading as their menu opens, so Play
//! next and Add to queue answer at once.

use std::sync::atomic::{AtomicU64, Ordering};

use encore_core::account::AccountAction;
use encore_core::backend::Command;
use encore_core::model::{Item, ItemKind, LikeStatus, Run, Target, Track};
use gpui_kit::assets::IconName;
use gpui_kit::*;

use super::control::{self, FromPage};
use crate::app::MusicApp;

pub use super::menu_keys::{
    CONTEXT, MenuChoose, MenuClose, MenuDown, MenuFirst, MenuIn, MenuLast, MenuOut, MenuUp,
    bind_keys,
};
use super::menu_keys::{find_letter, give_back, step};
use super::submenu::{Sub, sub_entries};

/// Where a song's menu was opened: what else it can offer.
#[derive(Clone, Debug, PartialEq)]
pub enum Place {
    List,
    /// Up next, at this queue position: Remove from queue.
    UpNext(usize),
    /// The account's own playlist, as this entry: Remove from playlist.
    Own {
        playlist_id: String,
        set_video_id: String,
    },
}

/// What a menu is about.
#[derive(Clone, Debug)]
pub enum Subject {
    Song {
        track: Track,
        place: Place,
    },
    Collection {
        page: Option<Target>,
        /// A card's play target (an album's audio playlist, a mix).
        play: Option<Target>,
        /// Linked artists in its subtitle.
        artists: Vec<Run>,
    },
    Artist {
        page: Target,
    },
}

impl Subject {
    /// The menu for a card or row, if it is music.
    pub fn of_item(item: &Item, place: Place) -> Option<Self> {
        if let Some(track) = &item.track {
            let mut track = track.clone();
            if track.thumbnail.is_none() {
                track.thumbnail.clone_from(&item.thumbnail);
            }
            return Some(Subject::Song { track, place });
        }
        let browse = item
            .target
            .clone()
            .filter(|t| matches!(t, Target::Browse { .. }));
        match item.kind {
            ItemKind::Album | ItemKind::Playlist => Some(Subject::Collection {
                page: browse,
                play: item.play.clone().or_else(|| {
                    item.target
                        .clone()
                        .filter(|t| matches!(t, Target::Watch { .. }))
                }),
                artists: artist_runs(&item.subtitle),
            }),
            ItemKind::Artist => Some(Subject::Artist { page: browse? }),
            _ => None,
        }
    }

    /// The page it depends on, loaded as the menu opens.
    fn page(&self) -> Option<&Target> {
        match self {
            Subject::Collection { page, .. } => page.as_ref(),
            Subject::Artist { page } => Some(page),
            Subject::Song { .. } => None,
        }
    }
}

/// Runs that link to an artist's page.
fn artist_runs(runs: &[Run]) -> Vec<Run> {
    runs.iter()
        .filter(|r| matches!(&r.target, Some(Target::Browse { id, .. }) if id.starts_with("UC")))
        .cloned()
        .collect()
}

/// An open menu.
pub struct Menu {
    pub subject: Subject,
    /// Where it was opened, in window coordinates.
    pub at: Point<Pixels>,
    /// The entry under the pointer or the keyboard.
    pub selected: Option<usize>,
    pub focus: FocusHandle,
    /// Where the keyboard was when it opened; it goes back there.
    back: Option<FocusHandle>,
    /// Add to playlist's submenu, while open.
    pub sub: Option<Sub>,
    /// Tells menus apart, so each one settles in.
    pub serial: u64,
}

/// What an entry does.
#[derive(Clone, Debug)]
pub enum Does {
    PlayNext,
    AddToQueue,
    SongRadio,
    Page(FromPage),
    Play(Target),
    Open(Target),
    Copy(String),
    RemoveQueued(usize),
    /// A change to the account (M3).
    Account(AccountAction),
    /// The Save to playlist dialog for the song (M3).
    AddToPlaylist,
}

/// One entry: its icon, label and section (a line separates sections).
pub struct Entry {
    pub icon: IconName,
    pub label: &'static str,
    pub does: Does,
    pub section: u8,
}

impl Entry {
    /// Add to playlist opens a submenu rather than acting.
    pub fn opens_sub(&self) -> bool {
        matches!(self.does, Does::AddToPlaylist)
    }
}

const QUEUE: u8 = 0;
const ACCOUNT: u8 = 1;
const GO: u8 = 2;
const SHARE: u8 = 3;
const EDIT: u8 = 4;

fn entry(icon: IconName, label: &'static str, does: Does, section: u8) -> Entry {
    Entry {
        icon,
        label,
        does,
        section,
    }
}

/// The menu's entries now, in sections.
pub fn entries(app: &MusicApp, subject: &Subject) -> Vec<Entry> {
    let mut out = Vec::new();
    match subject {
        Subject::Song { track, place } => {
            out.push(entry(
                IconName::ListStart,
                "Play next",
                Does::PlayNext,
                QUEUE,
            ));
            out.push(entry(
                IconName::ListEnd,
                "Add to queue",
                Does::AddToQueue,
                QUEUE,
            ));
            out.push(entry(
                IconName::Radio,
                "Start radio",
                Does::SongRadio,
                QUEUE,
            ));
            out.extend(account_entries(app, subject));
            if let Some(album) = track.album.as_ref().and_then(|a| a.target.clone()) {
                out.push(entry(IconName::Disc3, "Go to album", Does::Open(album), GO));
            }
            if let Some(artist) = track.artists.iter().find_map(|a| a.target.clone()) {
                out.push(entry(
                    IconName::UserRound,
                    "Go to artist",
                    Does::Open(artist),
                    GO,
                ));
            }
            let link = control::song_link(track);
            out.push(entry(IconName::Link, "Copy link", Does::Copy(link), SHARE));
            if let Place::UpNext(i) = place
                && app.player.playback.index != Some(*i)
            {
                out.push(entry(
                    IconName::ListX,
                    "Remove from queue",
                    Does::RemoveQueued(*i),
                    EDIT,
                ));
            }
            if let Place::Own {
                playlist_id,
                set_video_id,
            } = place
                && app.account.signed_in()
            {
                out.push(entry(
                    IconName::Trash,
                    "Remove from playlist",
                    Does::Account(AccountAction::Remove {
                        playlist_id: playlist_id.clone(),
                        set_video_id: set_video_id.clone(),
                    }),
                    EDIT,
                ));
            }
        }
        Subject::Collection {
            page,
            play,
            artists,
        } => {
            if page.is_some() {
                let from = |how| Does::Page(how);
                out.push(entry(
                    IconName::ListStart,
                    "Play next",
                    from(FromPage::Next),
                    QUEUE,
                ));
                out.push(entry(
                    IconName::ListEnd,
                    "Add to queue",
                    from(FromPage::Queue),
                    QUEUE,
                ));
                out.push(entry(
                    IconName::Shuffle,
                    "Shuffle play",
                    from(FromPage::Shuffle),
                    QUEUE,
                ));
                out.push(entry(
                    IconName::Radio,
                    "Start radio",
                    from(FromPage::Radio),
                    QUEUE,
                ));
            } else if let Some(play) = play {
                // A mix: it plays, nothing more.
                out.push(entry(
                    IconName::Play,
                    "Play",
                    Does::Play(play.clone()),
                    QUEUE,
                ));
            }
            out.extend(account_entries(app, subject));
            if let Some(artist) = artists.iter().find_map(|r| r.target.clone()) {
                out.push(entry(
                    IconName::UserRound,
                    "Go to artist",
                    Does::Open(artist),
                    GO,
                ));
            }
            if let Some(link) = control::page_link(page.as_ref(), play.as_ref()) {
                out.push(entry(IconName::Link, "Copy link", Does::Copy(link), SHARE));
            }
        }
        Subject::Artist { page } => {
            out.push(entry(
                IconName::Radio,
                "Start radio",
                Does::Page(FromPage::Radio),
                QUEUE,
            ));
            out.extend(account_entries(app, subject));
            if let Some(link) = control::page_link(Some(page), None) {
                out.push(entry(IconName::Link, "Copy link", Does::Copy(link), SHARE));
            }
        }
    }
    out
}

/// The account's entries, in the `ACCOUNT` section: for a song Like or
/// Unlike and Add to playlist, for an album or playlist Save to library or
/// Remove from library (from its page header, once loaded), for an artist
/// Subscribe or Unsubscribe. Signed out: none.
fn account_entries(app: &MusicApp, subject: &Subject) -> Vec<Entry> {
    if !app.account.signed_in() {
        return Vec::new();
    }
    let marks = &app.account.state.marks;
    let header = subject.page().and_then(|t| {
        app.pages
            .states
            .get(&t.key())?
            .page
            .as_ref()?
            .header
            .as_ref()
    });
    let mut out = Vec::new();
    match subject {
        Subject::Song { track, .. } => {
            let liked = marks.like(track) == LikeStatus::Like;
            let status = if liked {
                LikeStatus::Indifferent
            } else {
                LikeStatus::Like
            };
            let rate = Does::Account(AccountAction::Rate {
                track: track.clone(),
                status,
            });
            let label = if liked { "Unlike" } else { "Like" };
            out.push(entry(IconName::ThumbsUp, label, rate, ACCOUNT));
            out.push(entry(
                IconName::ListPlus,
                "Add to playlist",
                Does::AddToPlaylist,
                ACCOUNT,
            ));
        }
        Subject::Collection { .. } => {
            if let Some((h, library)) = header.and_then(|h| Some((h, h.library.as_ref()?))) {
                let saved = marks.saved(library);
                let save = Does::Account(AccountAction::Save {
                    playlist_id: library.playlist_id.clone(),
                    title: h.title.clone(),
                    save: !saved,
                });
                out.push(if saved {
                    entry(IconName::Check, "Remove from library", save, ACCOUNT)
                } else {
                    entry(IconName::Plus, "Save to library", save, ACCOUNT)
                });
            }
        }
        Subject::Artist { .. } => {
            if let Some((h, sub)) = header.and_then(|h| Some((h, h.subscription.as_ref()?))) {
                let subscribed = marks.subscribed(sub);
                let does = Does::Account(AccountAction::Subscribe {
                    channel_id: sub.channel_id.clone(),
                    name: h.title.clone(),
                    subscribe: !subscribed,
                });
                let label = if subscribed {
                    "Unsubscribe"
                } else {
                    "Subscribe"
                };
                out.push(entry(IconName::Bell, label, does, ACCOUNT));
            }
        }
    }
    out
}

static SERIAL: AtomicU64 = AtomicU64::new(0);

impl MusicApp {
    /// Opens `subject`'s menu at `at` (window coordinates); it takes the
    /// keyboard, and a page it depends on starts loading.
    pub fn open_menu(
        &mut self,
        subject: Subject,
        at: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // A menu opened over another gives the keyboard back to the first
        // one's origin.
        let back = match &self.desktop.layers.menu {
            Some(open) => open.back.clone(),
            None => window.focused(cx),
        };
        self.close_layers(window, cx);
        if let Some(page) = subject.page() {
            self.ensure_page(page.clone(), false);
        }
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        log::info!("menu opened for {}", describe(&subject));
        self.desktop.layers.menu = Some(Menu {
            subject,
            at,
            selected: None,
            focus,
            back,
            sub: None,
            serial: SERIAL.fetch_add(1, Ordering::Relaxed),
        });
        cx.notify();
    }

    /// Closes the menu; the keyboard goes back to where it was.
    pub fn close_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(menu) = self.desktop.layers.menu.take() {
            give_back(menu.back, &self.focus, window, cx);
            cx.notify();
        }
    }

    /// Esc: the submenu first, then the menu.
    pub fn escape_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.close_sub(cx) {
            self.close_menu(window, cx);
        }
    }

    /// The arrows: moves the highlight by `delta`, wrapping (in the
    /// submenu while it has the keyboard).
    pub fn move_in_menu(&mut self, delta: isize, cx: &mut Context<Self>) {
        let sub_count = self.sub_count();
        let main_count = self.main_count();
        let Some(menu) = &mut self.desktop.layers.menu else {
            return;
        };
        match &mut menu.sub {
            Some(sub) if sub.selected.is_some() => {
                sub.selected = step(sub.selected, delta, sub_count);
            }
            _ => {
                menu.sub = None;
                menu.selected = step(menu.selected, delta, main_count);
            }
        }
        cx.notify();
    }

    /// Home and End: the first or last entry.
    pub fn menu_end(&mut self, last: bool, cx: &mut Context<Self>) {
        let sub_count = self.sub_count();
        let main_count = self.main_count();
        let Some(menu) = &mut self.desktop.layers.menu else {
            return;
        };
        let at = |count: usize| (count > 0).then(|| if last { count - 1 } else { 0 });
        match &mut menu.sub {
            Some(sub) if sub.selected.is_some() => sub.selected = at(sub_count),
            _ => {
                menu.sub = None;
                menu.selected = at(main_count);
            }
        }
        cx.notify();
    }

    /// →: opens the highlighted entry's submenu.
    pub fn menu_in(&mut self, cx: &mut Context<Self>) {
        let Some(menu) = &self.desktop.layers.menu else {
            return;
        };
        if menu.sub.as_ref().is_some_and(|s| s.selected.is_some()) {
            return;
        }
        if let Some(i) = menu.selected
            && self.opens_sub(i)
        {
            self.open_sub(i, true, cx);
        }
    }

    /// Type-ahead: the next entry starting with `letter`.
    pub fn menu_letter(&mut self, letter: char, cx: &mut Context<Self>) -> bool {
        let Some(menu) = &self.desktop.layers.menu else {
            return false;
        };
        let in_sub = menu.sub.as_ref().filter(|s| s.selected.is_some());
        let found = match in_sub {
            Some(sub) => {
                let labels = sub_entries(self);
                find_letter(
                    labels.iter().map(|e| e.label.as_ref()),
                    sub.selected,
                    letter,
                )
                .map(|i| (true, i))
            }
            None => {
                let labels = entries(self, &menu.subject);
                find_letter(labels.iter().map(|e| e.label), menu.selected, letter)
                    .map(|i| (false, i))
            }
        };
        let Some((in_sub, i)) = found else {
            return false;
        };
        if let Some(menu) = &mut self.desktop.layers.menu {
            match (&mut menu.sub, in_sub) {
                (Some(sub), true) => sub.selected = Some(i),
                _ => {
                    menu.sub = None;
                    menu.selected = Some(i);
                }
            }
        }
        cx.notify();
        true
    }

    fn main_count(&self) -> usize {
        self.desktop
            .layers
            .menu
            .as_ref()
            .map_or(0, |m| entries(self, &m.subject).len())
    }

    fn sub_count(&self) -> usize {
        if self
            .desktop
            .layers
            .menu
            .as_ref()
            .is_some_and(|m| m.sub.is_some())
        {
            sub_entries(self).len()
        } else {
            0
        }
    }

    /// The pointer is on entry `i`: it takes the highlight, and opens its
    /// submenu or closes another's.
    pub fn hover_in_menu(&mut self, i: usize, cx: &mut Context<Self>) {
        if self.opens_sub(i) {
            self.open_sub(i, false, cx);
            return;
        }
        if let Some(menu) = &mut self.desktop.layers.menu
            && (menu.selected != Some(i) || menu.sub.is_some())
        {
            menu.selected = Some(i);
            menu.sub = None;
            cx.notify();
        }
    }

    /// Runs entry `i` (the highlighted one for `None`) and closes the
    /// menu; an entry with a submenu opens it instead.
    pub fn choose_in_menu(
        &mut self,
        i: Option<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(menu) = &self.desktop.layers.menu else {
            return;
        };
        if i.is_none() && menu.sub.as_ref().is_some_and(|s| s.selected.is_some()) {
            self.choose_in_sub(None, window, cx);
            return;
        }
        let keyboard = i.is_none();
        let Some(i) = i.or(menu.selected) else {
            return;
        };
        let subject = menu.subject.clone();
        let Some(chosen) = entries(self, &subject).into_iter().nth(i) else {
            return;
        };
        if chosen.opens_sub() {
            self.open_sub(i, keyboard, cx);
            return;
        }
        self.close_menu(window, cx);
        log::info!("menu: {} for {}", chosen.label, describe(&subject));
        self.perform(&subject, chosen.does, window, cx);
    }

    fn perform(
        &mut self,
        subject: &Subject,
        does: Does,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match (does, subject) {
            (Does::PlayNext, Subject::Song { track, .. }) => {
                self.send(Command::PlayNext(vec![track.clone()]));
                self.toast("Playing next", cx);
            }
            (Does::AddToQueue, Subject::Song { track, .. }) => {
                self.send(Command::AddToQueue(vec![track.clone()]));
                self.toast("Added to queue", cx);
            }
            (Does::SongRadio, Subject::Song { track, .. }) => {
                self.send(Command::PlayTarget(control::song_radio(track)));
            }
            (
                Does::Page(how),
                Subject::Collection {
                    page: Some(page), ..
                }
                | Subject::Artist { page },
            ) => {
                self.play_from_page(page.clone(), how, cx);
                match how {
                    FromPage::Next => self.toast("Playing next", cx),
                    FromPage::Queue => self.toast("Added to queue", cx),
                    _ => {}
                }
            }
            (Does::Play(target), _) => self.activate(target, cx),
            (Does::Open(target), _) => self.open_link(target, cx),
            (Does::Copy(link), _) => self.copy_link(link, cx),
            (Does::RemoveQueued(i), _) => self.edit_queue(Command::RemoveFromQueue(i), cx),
            (Does::Account(action), _) => self.account_act(action, cx),
            (Does::AddToPlaylist, Subject::Song { track, .. }) => {
                crate::account::add_to_playlist(self, vec![track.clone()], window, cx);
            }
            _ => {}
        }
        cx.notify();
    }
}

/// A subject, for the log.
fn describe(subject: &Subject) -> String {
    match subject {
        Subject::Song { track, .. } => format!("song {:?}", track.title),
        Subject::Collection { page, play, .. } => page
            .as_ref()
            .or(play.as_ref())
            .map_or("a mix".into(), Target::key),
        Subject::Artist { page } => format!("artist {}", page.key()),
    }
}
