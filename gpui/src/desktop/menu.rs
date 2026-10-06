//! Context menus (docs/SPEC.md "Control"): right-clicking a song, album,
//! playlist or artist (or its ⋮ button) opens its actions. This is the
//! menu's state and what its entries do; `views::menu` draws it.
//!
//! The open menu holds the keyboard (`MusicMenu` key context): the arrows
//! and Tab move, Enter or Space chooses, Esc closes. Album, playlist and
//! artist pages start loading as their menu opens, so Play next and Add to
//! queue answer at once.

use std::sync::atomic::{AtomicU64, Ordering};

use gpui_kit::assets::IconName;
use gpui_kit::*;
use ytfast::backend::Command;
use ytfast::model::{Header, Item, ItemKind, Run, Target, Track};

use super::control::{self, FromPage};
use crate::app::MusicApp;

actions!(music_menu, [MenuUp, MenuDown, MenuChoose, MenuClose]);

/// The key context of an open menu.
pub const CONTEXT: &str = "MusicMenu";

pub fn bind_keys(cx: &mut App) {
    let menu = Some(CONTEXT);
    cx.bind_keys([
        KeyBinding::new("up", MenuUp, menu),
        KeyBinding::new("shift-tab", MenuUp, menu),
        KeyBinding::new("down", MenuDown, menu),
        KeyBinding::new("tab", MenuDown, menu),
        KeyBinding::new("enter", MenuChoose, menu),
        KeyBinding::new("space", MenuChoose, menu),
        KeyBinding::new("escape", MenuClose, menu),
    ]);
}

/// Where a song's menu was opened: what else it can offer.
#[derive(Clone, Debug, PartialEq)]
pub enum Place {
    List,
    /// Up next, at this queue position: Remove from queue.
    UpNext(usize),
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

    /// The menu for a page's header: the album, playlist or artist itself.
    #[allow(dead_code, reason = "for a header ⋮ once M1/M3 place one")]
    pub fn of_header(header: &Header, page: &Target) -> Self {
        let artist = header.subscription.is_some()
            || matches!(page, Target::Browse { id, .. } if id.starts_with("UC"));
        if artist {
            Subject::Artist { page: page.clone() }
        } else {
            Subject::Collection {
                page: Some(page.clone()),
                play: header.play.clone(),
                artists: artist_runs(&header.subtitle),
            }
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
}

/// One entry: its icon, label and section (a line separates sections).
pub struct Entry {
    pub icon: IconName,
    pub label: &'static str,
    pub does: Does,
    pub section: u8,
}

const QUEUE: u8 = 0;
#[allow(dead_code, reason = "M3's entries go here")]
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

/// TODO(M3): the account's entries, in the `ACCOUNT` section, once M3's
/// API is on main: for a song Like/Unlike and Add to playlist, for an album
/// or playlist Save to library/Remove from library (from its page header's
/// `library`), for an artist Subscribe/Unsubscribe. Signed out: none.
fn account_entries(_app: &MusicApp, _subject: &Subject) -> Vec<Entry> {
    Vec::new()
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
            serial: SERIAL.fetch_add(1, Ordering::Relaxed),
        });
        cx.notify();
    }

    pub fn close_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.desktop.layers.menu.take().is_some() {
            window.focus(&self.focus, cx);
            cx.notify();
        }
    }

    /// The arrows: moves the highlight by `delta`, wrapping.
    pub fn move_in_menu(&mut self, delta: isize, cx: &mut Context<Self>) {
        let Some(menu) = &self.desktop.layers.menu else {
            return;
        };
        let count = entries(self, &menu.subject).len() as isize;
        if count == 0 {
            return;
        }
        let at = match menu.selected {
            Some(i) => (i as isize + delta).rem_euclid(count),
            None if delta > 0 => 0,
            None => count - 1,
        };
        if let Some(menu) = &mut self.desktop.layers.menu {
            menu.selected = Some(at as usize);
        }
        cx.notify();
    }

    /// The pointer is on entry `i`.
    pub fn hover_in_menu(&mut self, i: usize, cx: &mut Context<Self>) {
        if let Some(menu) = &mut self.desktop.layers.menu
            && menu.selected != Some(i)
        {
            menu.selected = Some(i);
            cx.notify();
        }
    }

    /// Runs entry `i` (the highlighted one for `None`) and closes the menu.
    pub fn choose_in_menu(
        &mut self,
        i: Option<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(menu) = &self.desktop.layers.menu else {
            return;
        };
        let Some(i) = i.or(menu.selected) else {
            return;
        };
        let subject = menu.subject.clone();
        let Some(chosen) = entries(self, &subject).into_iter().nth(i) else {
            return;
        };
        self.close_menu(window, cx);
        log::info!("menu: {} for {}", chosen.label, describe(&subject));
        self.perform(&subject, chosen.does, cx);
    }

    fn perform(&mut self, subject: &Subject, does: Does, cx: &mut Context<Self>) {
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
