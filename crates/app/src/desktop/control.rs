//! What the keyboard, the context menus and Play anything ask of the app
//! beyond a plain backend command: relative seeks and volume steps, the
//! layers over the window and closing the topmost, an album's or
//! playlist's songs fetched through the page machinery before they are
//! queued, and Copy link.

use std::time::{Duration, Instant};

use encore_core::backend::Command;
use encore_core::model::{Header, Target, Track};
use gpui_kit::*;

use super::menu::Menu;
use super::palette::PlayAnything;
use crate::app::MusicApp;

/// The most songs one page gives Play next or Add to queue.
const MOST: usize = 500;
/// A page that hasn't come within this long is given up on.
const PATIENCE: Duration = Duration::from_secs(30);
/// How often waiting page actions look again.
const POLL: Duration = Duration::from_millis(200);
/// How long a note such as "Link copied" stays.
const TOAST: Duration = Duration::from_millis(2200);

/// The layers over the window, topmost last: the shortcuts sheet, Play
/// anything and a context menu; and a short note at the bottom.
#[derive(Default)]
pub struct Layers {
    pub help: bool,
    pub palette: Option<PlayAnything>,
    pub menu: Option<Menu>,
    /// A short note ("Link copied"), its stamp, and whether it confirms
    /// something done (a check mark) or only informs.
    pub toast: Option<(SharedString, u64, bool)>,
    toasts: u64,
    pending: Vec<Pending>,
    poll: Option<Task<()>>,
}

/// What to do with an album's, playlist's or artist's page.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FromPage {
    Play,
    /// Play next, keeping the page's order.
    Next,
    /// Add to queue, keeping the page's order.
    Queue,
    Shuffle,
    Radio,
}

struct Pending {
    page: Target,
    how: FromPage,
    since: Instant,
}

impl MusicApp {
    /// ← and →: seek from where the song is now.
    pub fn seek_by(&mut self, seconds: f64, cx: &mut Context<Self>) {
        let duration = self.player.playback.duration;
        if self.player.current().is_none() || duration <= 0.0 {
            return;
        }
        let to = (self.player.position() + seconds).clamp(0.0, (duration - 1.0).max(0.0));
        self.seek_to(to, cx);
    }

    /// `+` and `-`: the volume by `step` points; the slider follows.
    pub fn volume_by(&mut self, step: f64, window: &mut Window, cx: &mut Context<Self>) {
        let volume = (self.player.playback.volume + step).clamp(0.0, 100.0);
        log::info!("key: volume {volume:.0}");
        self.player.muted_from = None;
        self.player.playback.volume = volume;
        self.player
            .volume
            .update(cx, |slider, cx| slider.set_value(volume as f32, window, cx));
        self.send(Command::Volume(volume));
        cx.notify();
    }

    /// `/` and Ctrl+F: the search field takes the keyboard.
    pub fn focus_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.close_layers(window, cx);
        if let Some(field) = &self.pages.search.field {
            log::info!("key: search");
            field.input.update(cx, |state, cx| state.focus(window, cx));
        }
    }

    /// Esc: closes the topmost of a menu, Play anything, the shortcuts,
    /// Now Playing and Up next. `false` when nothing was open.
    pub fn close_top_layer(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if self.close_account_layer(window, cx) {
            return true;
        }
        let layers = &self.desktop.layers;
        if layers.menu.is_some() {
            self.close_menu(window, cx);
        } else if layers.palette.is_some() {
            self.close_play_anything(window, cx);
        } else if layers.help {
            self.show_shortcuts(false, window, cx);
        } else if self.player.now_playing {
            self.show_now_playing(false, cx);
        } else if self.player.queue_open {
            self.toggle_up_next(cx);
        } else {
            return false;
        }
        true
    }

    /// Closes the menu, Play anything and the shortcuts.
    pub fn close_layers(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.desktop.layers.menu.is_some() {
            self.close_menu(window, cx);
        }
        if self.desktop.layers.palette.is_some() {
            self.close_play_anything(window, cx);
        }
        self.desktop.layers.help = false;
    }

    pub fn show_shortcuts(&mut self, open: bool, window: &mut Window, cx: &mut Context<Self>) {
        if open {
            self.close_layers(window, cx);
            window.focus(&self.focus, cx);
            log::info!("showing the keyboard shortcuts");
        }
        self.desktop.layers.help = open;
        cx.notify();
    }

    /// Puts `link` on the clipboard and says so.
    pub fn copy_link(&mut self, link: String, cx: &mut Context<Self>) {
        log::info!("copied a link: {link}");
        cx.write_to_clipboard(ClipboardItem::new_string(link));
        self.toast("Link copied", cx);
    }

    /// Confirms something done, above the player bar for a moment.
    pub fn toast(&mut self, text: impl Into<SharedString>, cx: &mut Context<Self>) {
        self.show_note(text.into(), true, cx);
    }

    /// Says something plainly (no check mark), for a moment.
    pub fn notice(&mut self, text: impl Into<SharedString>, cx: &mut Context<Self>) {
        self.show_note(text.into(), false, cx);
    }

    fn show_note(&mut self, text: SharedString, done: bool, cx: &mut Context<Self>) {
        let layers = &mut self.desktop.layers;
        layers.toasts += 1;
        let stamp = layers.toasts;
        layers.toast = Some((text, stamp, done));
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(TOAST).await;
            let _ = this.update(cx, |this, cx| {
                if this.desktop.layers.toast.as_ref().map(|t| t.1) == Some(stamp) {
                    this.desktop.layers.toast = None;
                    cx.notify();
                }
            });
        })
        .detach();
        cx.notify();
    }

    /// Plays, queues, shuffles or starts the radio of a page's music,
    /// loading the page (and the rest of a long playlist) first.
    pub fn play_from_page(&mut self, page: Target, how: FromPage, cx: &mut Context<Self>) {
        self.ensure_page(page.clone(), false);
        let pending = Pending {
            page,
            how,
            since: Instant::now(),
        };
        if self.run_pending(&pending) {
            return;
        }
        self.desktop.layers.pending.push(pending);
        if self.desktop.layers.poll.is_none() {
            self.desktop.layers.poll = Some(cx.spawn(async move |this, cx| {
                loop {
                    cx.background_executor().timer(POLL).await;
                    let more = this.update(cx, |this, cx| this.poll_pending(cx));
                    if !matches!(more, Ok(true)) {
                        break;
                    }
                }
            }));
        }
    }

    /// Carries on with waiting page actions; `true` while some still wait.
    fn poll_pending(&mut self, cx: &mut Context<Self>) -> bool {
        for pending in std::mem::take(&mut self.desktop.layers.pending) {
            if self.run_pending(&pending) {
                cx.notify();
            } else if pending.since.elapsed() > PATIENCE {
                self.error = Some("That took too long to load. Try again.".into());
                cx.notify();
            } else {
                self.desktop.layers.pending.push(pending);
            }
        }
        let waiting = !self.desktop.layers.pending.is_empty();
        if !waiting {
            self.desktop.layers.poll = None;
        }
        waiting
    }

    /// Does what `pending` asks once its page is here; `false` while it
    /// still has to wait (for the page or the rest of a long playlist).
    fn run_pending(&mut self, pending: &Pending) -> bool {
        let key = pending.page.key();
        let Some(state) = self.pages.states.get(&key) else {
            return false;
        };
        let Some(page) = &state.page else {
            if let (false, Some(error)) = (state.loading, &state.error) {
                self.error = Some(format!("Couldn't load that: {error}"));
                return true;
            }
            return false;
        };
        // A saved copy may be out of date: wait for the fresh page.
        if state.loading {
            return false;
        }
        let header = page.header.as_ref();
        let target = match pending.how {
            FromPage::Radio => header.and_then(|h| h.radio.clone().or_else(|| playlist_radio(h))),
            FromPage::Play => header.and_then(|h| h.play.clone()),
            FromPage::Shuffle => header.and_then(|h| h.shuffle.clone()),
            FromPage::Next | FromPage::Queue => None,
        };
        if let Some(target) = target {
            log::info!("{key}: {:?} through its play target", pending.how);
            self.send(Command::PlayTarget(target));
            return true;
        }
        if pending.how == FromPage::Radio {
            self.error = Some("There's no radio for this.".into());
            return true;
        }
        // A playlist's own list when it has one: Suggestions after it on
        // the page aren't in the playlist.
        let own = encore_core::account::entries(page);
        let shelves: Vec<usize> = page
            .shelves
            .iter()
            .enumerate()
            .filter(|(_, s)| own.is_none_or(|own| std::ptr::eq(*s, own)))
            .map(|(i, _)| i)
            .collect();
        let mut tracks: Vec<Track> = shelves
            .iter()
            .flat_map(|i| &page.shelves[*i].items)
            .filter_map(|i| i.track.clone())
            .collect();
        let rest = shelves
            .iter()
            .rev()
            .find(|i| page.shelves[**i].continuation.is_some())
            .copied();
        if let Some(shelf) = rest
            && tracks.len() < MOST
        {
            // A long playlist comes in parts: fetch the rest first.
            self.more(&key, Some(shelf));
            return false;
        }
        tracks.truncate(MOST);
        if tracks.is_empty() {
            self.error = Some("There are no songs to play here.".into());
            return true;
        }
        log::info!("{key}: {:?} with {} songs", pending.how, tracks.len());
        self.send(match pending.how {
            FromPage::Next => Command::PlayNext(tracks),
            FromPage::Queue => Command::AddToQueue(tracks),
            FromPage::Shuffle => {
                shuffle(&mut tracks);
                Command::PlayTracks { tracks, start: 0 }
            }
            FromPage::Play | FromPage::Radio => Command::PlayTracks { tracks, start: 0 },
        });
        true
    }
}

/// An album's or playlist's radio when its header offers none: YouTube
/// Music's `RDAMPL` mix of its playlist (a mix already is one).
fn playlist_radio(header: &Header) -> Option<Target> {
    let list = match &header.play {
        Some(Target::Watch {
            playlist_id: Some(list),
            ..
        }) => list.clone(),
        _ => header.library.as_ref()?.playlist_id.clone(),
    };
    let playlist_id = if list.starts_with("RD") {
        list
    } else {
        format!("RDAMPL{list}")
    };
    Some(Target::Watch {
        video_id: None,
        playlist_id: Some(playlist_id),
        params: None,
    })
}

/// Fisher-Yates with a small xorshift seeded from the clock: good enough
/// to shuffle a page's songs.
fn shuffle<T>(items: &mut [T]) {
    let mut seed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0x2545_f491, |d| d.as_nanos() as u64)
        | 1;
    for i in (1..items.len()).rev() {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        items.swap(i, (seed % (i as u64 + 1)) as usize);
    }
}

/// A song's radio, as YouTube Music's autoplay continues with.
pub fn song_radio(track: &Track) -> Target {
    Target::Watch {
        video_id: Some(track.video_id.clone()),
        playlist_id: Some(format!("RDAMVM{}", track.video_id)),
        params: Some("wAEB".into()),
    }
}

/// The YouTube Music link to a song.
pub fn song_link(track: &Track) -> String {
    format!("{MUSIC}/watch?v={}", track.video_id)
}

const MUSIC: &str = "https://music.youtube.com";

/// The YouTube Music link to an album's, playlist's or artist's page, or
/// to a mix's playlist.
pub fn page_link(page: Option<&Target>, play: Option<&Target>) -> Option<String> {
    match (page, play) {
        (Some(Target::Browse { id, .. }), _) => Some(match id.strip_prefix("VL") {
            Some(list) => format!("{MUSIC}/playlist?list={list}"),
            None if id.starts_with("UC") => format!("{MUSIC}/channel/{id}"),
            None => format!("{MUSIC}/browse/{id}"),
        }),
        (
            _,
            Some(Target::Watch {
                playlist_id: Some(list),
                ..
            }),
        ) => Some(format!("{MUSIC}/playlist?list={list}")),
        _ => None,
    }
}
