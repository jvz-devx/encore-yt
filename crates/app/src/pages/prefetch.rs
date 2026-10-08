//! M28: prefetch on hover. When the pointer rests on a playlist, album,
//! artist or mood card or row (`DWELL`), its page is fetched into the page
//! cache, so a click shows it at once (or joins the fetch still under way).
//! Resting on a card's Play button or on a song row resolves the first song
//! through the resolver's speculative path (`Command::Prepare`), never a
//! playback slot.
//!
//! The budget keeps YouTube's rate limits far away: one hover fetch at a
//! time (the newest waiting one wins), at most `FETCHES_PER_MINUTE` fetches
//! and `RESOLVES_PER_MINUTE` resolves a minute, nothing for a page that is
//! loaded and fresh or loading already, and each item at most once a session
//! unless its copy went stale. Turning the wheel cancels what is pending:
//! the pointer then rests on whatever scrolled under it, which it hasn't
//! chosen. Settings → Playback turns all of it off (`prefetch.json`).

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use encore_core::backend::Command;
use encore_core::model::{Item, ItemKind, Target};
use encore_core::paths::Paths;
use gpui_kit::*;
use serde::{Deserialize, Serialize};

use super::STALE;
use crate::app::MusicApp;
use crate::nav::View;

/// How long the pointer rests on an item before it counts as interest.
pub const DWELL: Duration = Duration::from_millis(250);
/// Hover fetches (browse requests) allowed in any minute.
pub const FETCHES_PER_MINUTE: usize = 20;
/// Hover resolves (stream look-ups) allowed in any minute.
pub const RESOLVES_PER_MINUTE: usize = 10;
const MINUTE: Duration = Duration::from_secs(60);
/// A hover fetch with no answer after this no longer holds up the next one.
const GIVE_UP: Duration = Duration::from_secs(30);
/// How often the OS is asked whether the connection is metered.
const METERED_EVERY: Duration = MINUTE;

/// What resting on an item asks for.
#[derive(Clone, Debug, PartialEq)]
pub enum Want {
    /// A page, into the page cache.
    Page(Target),
    /// A song's stream.
    Song(String),
    /// The first song of a page (an album's or playlist's Play), resolved
    /// once that page is loaded.
    FirstSong(Target),
}

impl Want {
    /// The page a card or row opens, if it opens one worth fetching.
    pub fn page(item: &Item) -> Option<Self> {
        if item.track.is_some() {
            return None;
        }
        let target = item.target.as_ref()?;
        matches!(View::for_target(target), Some(View::Page(_))).then(|| Want::Page(target.clone()))
    }

    /// What its Play button starts first: a song, or an album's or
    /// playlist's first song. Artists' and others' Play is a shuffle or a
    /// radio, whose first song isn't known ahead.
    pub fn play(item: &Item) -> Option<Self> {
        if let Some(track) = &item.track {
            return Some(Want::Song(track.video_id.clone()));
        }
        if let Some(Target::Watch {
            video_id: Some(id), ..
        }) = &item.play
        {
            return Some(Want::Song(id.clone()));
        }
        match item.kind {
            ItemKind::Album | ItemKind::Playlist => match Want::page(item)? {
                Want::Page(target) => Some(Want::FirstSong(target)),
                _ => None,
            },
            _ => None,
        }
    }
}

/// Settings → Playback's switch, in `~/.config/encore-yt/prefetch.json`.
#[derive(Clone, Debug, Serialize, Deserialize)]
struct Prefs {
    #[serde(default = "on")]
    hover: bool,
}

fn on() -> bool {
    true
}

/// A check last made at `last` is due again at `now`.
fn check_due(last: Option<Instant>, now: Instant) -> bool {
    last.is_none_or(|t| now.saturating_duration_since(t) >= METERED_EVERY)
}

/// Hover intent and its budget.
pub struct Prefetch {
    /// Load pages and songs the pointer rests on (Settings → Playback).
    pub on: bool,
    path: PathBuf,
    clock: BackgroundExecutor,
    /// The page item under the pointer, and its pending wait.
    page_hover: Option<(Want, Task<()>)>,
    /// The Play button or song under the pointer, and its pending wait.
    song_hover: Option<(Want, Task<()>)>,
    /// The hover fetch under way: its page key and when it went out.
    in_flight: Option<(String, Instant)>,
    /// The newest page asked for while another was under way.
    queued: Option<Target>,
    fetches: VecDeque<Instant>,
    resolves: VecDeque<Instant>,
    /// Pages fetched on hover this session, and when.
    fetched: HashMap<String, Instant>,
    /// Songs resolved on hover this session.
    resolved: HashSet<String>,
    /// A page whose first song is wanted once it arrives.
    first_song_of: Option<String>,
    /// The OS says the connection is metered (checked at most once a
    /// minute, off the UI thread): nothing is prefetched on it.
    metered: bool,
    metered_checked: Option<Instant>,
}

impl Prefetch {
    pub fn new(paths: &Paths, clock: BackgroundExecutor) -> Self {
        let path = paths.config.join("prefetch.json");
        let on = encore_core::paths::read_json::<Prefs>(&path).is_none_or(|p| p.hover);
        Self {
            on,
            path,
            clock,
            page_hover: None,
            song_hover: None,
            in_flight: None,
            queued: None,
            fetches: VecDeque::new(),
            resolves: VecDeque::new(),
            fetched: HashMap::new(),
            resolved: HashSet::new(),
            first_song_of: None,
            metered: false,
            metered_checked: None,
        }
    }

    /// What the OS said about a metered connection (the tests say it).
    #[cfg(test)]
    pub fn set_metered(&mut self, metered: bool) {
        self.metered = metered;
    }

    /// Whether the metered check is due at `now`.
    fn metered_due(&self, now: Instant) -> bool {
        check_due(self.metered_checked, now)
    }

    /// Whether page `key` was fetched on hover (for the click timing).
    pub fn hover_fetched(&self, key: &str) -> bool {
        self.fetched.contains_key(key)
    }

    fn save(&self) {
        let prefs = Prefs { hover: self.on };
        let written = serde_json::to_vec_pretty(&prefs)
            .map_err(std::io::Error::other)
            .and_then(|bytes| encore_core::paths::write_atomic(&self.path, &bytes));
        if let Err(e) = written {
            log::warn!("couldn't save {}: {e}", self.path.display());
        }
    }

    fn slot(&mut self, want: &Want) -> &mut Option<(Want, Task<()>)> {
        match want {
            Want::Page(_) => &mut self.page_hover,
            Want::Song(_) | Want::FirstSong(_) => &mut self.song_hover,
        }
    }

    /// Takes a turn from `spent` if fewer than `limit` went in the last
    /// minute.
    fn spend(spent: &mut VecDeque<Instant>, limit: usize, now: Instant) -> bool {
        while spent
            .front()
            .is_some_and(|t| now.saturating_duration_since(*t) >= MINUTE)
        {
            spent.pop_front();
        }
        if spent.len() >= limit {
            return false;
        }
        spent.push_back(now);
        true
    }
}

impl MusicApp {
    /// An item's hover changed: resting on it for `DWELL` acts on `want`.
    pub fn hover_intent(&mut self, want: Want, hovered: bool, cx: &mut Context<Self>) {
        let prefetch = &mut self.pages.prefetch;
        if !prefetch.on {
            return;
        }
        if hovered {
            self.check_metered(cx);
        }
        let prefetch = &mut self.pages.prefetch;
        let slot = prefetch.slot(&want);
        if !hovered {
            if slot.as_ref().is_some_and(|(w, _)| *w == want) {
                // Dropping the wait cancels it.
                *slot = None;
            }
            return;
        }
        let wait = cx.spawn({
            let want = want.clone();
            async move |this, cx| {
                cx.background_executor().timer(DWELL).await;
                let _ = this.update(cx, |this, _| this.intent_rested(want));
            }
        });
        *slot = Some((want, wait));
    }

    /// Asks the OS about a metered connection when the last answer is a
    /// minute old; the answer arrives later and applies to the next hover.
    fn check_metered(&mut self, cx: &mut Context<Self>) {
        let prefetch = &mut self.pages.prefetch;
        let now = prefetch.clock.now();
        if !prefetch.metered_due(now) {
            return;
        }
        prefetch.metered_checked = Some(now);
        // The headless tests have no system bus to ask.
        if cfg!(test) {
            return;
        }
        let ask = cx
            .background_executor()
            .spawn(async { encore_core::metered::is_metered() });
        cx.spawn(async move |this, cx| {
            let metered = ask.await;
            let _ = this.update(cx, |this, _| this.pages.prefetch.metered = metered);
        })
        .detach();
    }

    /// The wheel turned: what rests under the pointer now scrolled there.
    pub fn intent_scrolled(&mut self) {
        let prefetch = &mut self.pages.prefetch;
        prefetch.page_hover = None;
        prefetch.song_hover = None;
    }

    /// The pointer rested on an item for `DWELL`.
    fn intent_rested(&mut self, want: Want) {
        if self.pages.prefetch.metered {
            return;
        }
        // Still under the pointer: the slot keeps it until the pointer
        // leaves, for `prefetch_landed`.
        let slot = self.pages.prefetch.slot(&want);
        if !slot.as_ref().is_some_and(|(w, _)| *w == want) {
            return;
        }
        match want {
            Want::Page(target) => self.hover_fetch(target),
            Want::Song(id) => self.hover_resolve(id),
            Want::FirstSong(target) => {
                let key = target.key();
                match self.first_song(&key) {
                    Some(id) => self.hover_resolve(id),
                    None => {
                        // The card's own rest asked for the page already,
                        // or this asks now; its first song follows it.
                        self.pages.prefetch.first_song_of = Some(key);
                        self.hover_fetch(target);
                    }
                }
            }
        }
    }

    /// The first song of loaded page `key`.
    fn first_song(&self, key: &str) -> Option<String> {
        let page = self.pages.states.get(key)?.page.as_ref()?;
        page.shelves
            .iter()
            .flat_map(|s| s.items.iter())
            .find_map(|i| i.track.as_ref())
            .map(|t| t.video_id.clone())
    }

    /// Fetches page `target` on hover, within the budget.
    fn hover_fetch(&mut self, target: Target) {
        let key = target.key();
        let now = self.pages.prefetch.clock.now();
        if let Some(state) = self.pages.states.get(&key) {
            let fresh = !state.cached
                && state.error.is_none()
                && state.fetched.is_some_and(|t| t.elapsed() <= STALE);
            if state.loading || fresh {
                return;
            }
        }
        let prefetch = &mut self.pages.prefetch;
        if prefetch
            .fetched
            .get(&key)
            .is_some_and(|t| now.saturating_duration_since(*t) <= STALE)
        {
            return;
        }
        if let Some((_, sent)) = &prefetch.in_flight {
            if now.saturating_duration_since(*sent) < GIVE_UP {
                prefetch.queued = Some(target);
                return;
            }
            prefetch.in_flight = None;
        }
        if !Prefetch::spend(&mut prefetch.fetches, FETCHES_PER_MINUTE, now) {
            log::debug!("hover fetch of {key} skipped: a minute's budget is spent");
            return;
        }
        log::debug!("hover fetch {key}");
        prefetch
            .fetched
            .retain(|_, at| now.saturating_duration_since(*at) <= STALE);
        prefetch.fetched.insert(key.clone(), now);
        prefetch.in_flight = Some((key, now));
        self.ensure_page(target, false);
    }

    /// Resolves song `id` on hover, within the budget.
    fn hover_resolve(&mut self, id: String) {
        let prepared = self.backend.live().is_some_and(|b| b.prepared(&id));
        let prefetch = &mut self.pages.prefetch;
        if prepared || prefetch.resolved.contains(&id) {
            return;
        }
        let now = prefetch.clock.now();
        if !Prefetch::spend(&mut prefetch.resolves, RESOLVES_PER_MINUTE, now) {
            log::debug!("hover resolve of {id} skipped: a minute's budget is spent");
            return;
        }
        log::debug!("hover resolve {id}");
        remember_resolved(&mut prefetch.resolved, id.clone());
        self.backend.send(Command::Prepare(id));
    }

    /// A page's answer came (not its saved copy): the next hover fetch may
    /// go, and a Play button still under the pointer gets its first song.
    pub(super) fn prefetch_landed(&mut self, key: &str) {
        let prefetch = &mut self.pages.prefetch;
        if prefetch.first_song_of.as_deref() == Some(key) {
            prefetch.first_song_of = None;
            let still = matches!(
                &prefetch.song_hover,
                Some((Want::FirstSong(t), _)) if t.key() == key
            );
            if still && let Some(id) = self.first_song(key) {
                self.hover_resolve(id);
            }
        }
        let prefetch = &mut self.pages.prefetch;
        if prefetch.in_flight.as_ref().is_some_and(|(k, _)| k == key) {
            prefetch.in_flight = None;
            if let Some(next) = prefetch.queued.take() {
                self.hover_fetch(next);
            }
        }
    }

    /// Settings → Playback: "Load pages when you point at them".
    pub fn set_prefetch(&mut self, on: bool, cx: &mut Context<Self>) {
        let prefetch = &mut self.pages.prefetch;
        prefetch.on = on;
        if !on {
            prefetch.page_hover = None;
            prefetch.song_hover = None;
            prefetch.queued = None;
            prefetch.first_song_of = None;
        }
        prefetch.save();
        cx.notify();
    }
}

fn remember_resolved(resolved: &mut HashSet<String>, id: String) {
    const MAX_RESOLVED: usize = 512;
    if resolved.len() >= MAX_RESOLVED
        && let Some(old) = resolved.iter().next().cloned()
    {
        resolved.remove(&old);
    }
    resolved.insert(id);
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "test assertions report fixture failures"
)]
mod tests {
    use std::time::{Duration, Instant};

    use super::{MINUTE, check_due};

    #[test]
    fn hover_resolution_history_is_bounded() {
        let mut resolved = std::collections::HashSet::new();
        for n in 0..1024 {
            super::remember_resolved(&mut resolved, n.to_string());
        }
        assert_eq!(resolved.len(), 512);
        assert!(resolved.contains("1023"));
    }

    #[test]
    fn the_metered_check_runs_once_a_minute() {
        let start = Instant::now();
        assert!(check_due(None, start));
        assert!(!check_due(Some(start), start + Duration::from_secs(59)));
        assert!(check_due(Some(start), start + MINUTE));
    }
}
