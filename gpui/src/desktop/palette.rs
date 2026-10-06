//! Play anything (Ctrl+K): one field over the library pages already
//! loaded, recent searches, the page on screen, YouTube Music's search and
//! a few commands. Local results come with each keystroke; YouTube Music is
//! asked once typing pauses and its answer joins in. Ranking: exact and
//! prefix title matches first, then the library over the catalogue.
//! Enter plays the highlighted result, Shift+Enter opens its page.

use std::collections::HashSet;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::time::Duration;

use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::*;
use ytfast::backend::Command;
use ytfast::model::{Item, ItemKind, Page, Target, Track};

use super::control::{self, FromPage};
use crate::app::MusicApp;
use crate::nav::LibraryTab;

/// Typing pauses this long before YouTube Music is asked.
const DEBOUNCE: Duration = Duration::from_millis(150);
/// Results shown at once.
const SHOWN: usize = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Song,
    Album,
    Playlist,
    Artist,
    Search,
    Command,
}

impl Kind {
    pub fn label(self) -> &'static str {
        match self {
            Kind::Song => "Song",
            Kind::Album => "Album",
            Kind::Playlist => "Playlist",
            Kind::Artist => "Artist",
            Kind::Search => "Search",
            Kind::Command => "Command",
        }
    }
}

/// A command Play anything runs.
#[derive(Clone, Debug)]
pub enum Cmd {
    /// Radio from this song (the top song match of `radio <query>`).
    Radio(Box<Track>),
    Next,
    Pause,
    Play,
    Shuffle,
    Repeat,
    /// Answered by another area that isn't here yet: the note to show.
    Elsewhere(&'static str),
}

/// What choosing a result does.
#[derive(Clone, Debug)]
pub enum Go {
    /// Play it, with the song results as the queue; Shift opens its album.
    Song(Track),
    /// Play an album or playlist; Shift opens its page.
    Collection {
        page: Option<Target>,
        play: Option<Target>,
    },
    Artist(Target),
    Search(String),
    Command(Cmd),
    /// Put this in the field: a command waiting for what follows it.
    Complete(String),
}

#[derive(Clone, Debug)]
pub struct Hit {
    pub key: String,
    pub title: String,
    pub detail: String,
    pub thumbnail: Option<String>,
    pub kind: Kind,
    pub go: Go,
}

pub struct PlayAnything {
    pub input: Entity<InputState>,
    pub query: String,
    pub selected: usize,
    /// The catalogue search last asked for.
    asked: String,
    /// YouTube Music's last answer: its query and results.
    catalogue: Option<(String, Result<Page, String>)>,
    generation: u64,
    hits: Vec<Hit>,
    /// What `hits` were worked out from.
    stamp: u64,
    /// A note under the results ("not available yet").
    pub note: Option<&'static str>,
    pub serial: u64,
    debounce: Option<Task<()>>,
    _subscription: Subscription,
}

impl PlayAnything {
    pub fn hits(&self) -> &[Hit] {
        &self.hits
    }

    /// YouTube Music hasn't answered for what's typed yet.
    pub fn searching(&self) -> bool {
        catalogue_query(&self.query).is_some_and(|q| {
            self.catalogue
                .as_ref()
                .is_none_or(|(answered, _)| *answered != q)
        })
    }

    /// YouTube Music couldn't be asked.
    pub fn failed(&self) -> bool {
        matches!(&self.catalogue, Some((_, Err(_))))
    }
}

/// What YouTube Music is searched for: the words after `radio`, nothing for
/// `sleep …` and `eq …`, else the whole text (two letters or more).
fn catalogue_query(query: &str) -> Option<String> {
    let query = query.trim();
    let (word, rest) = split_command(query);
    let wanted = match word.as_str() {
        "radio" if !rest.is_empty() => rest,
        "sleep" | "eq" if !rest.is_empty() => return None,
        _ => query,
    };
    (wanted.chars().count() >= 2).then(|| wanted.to_owned())
}

/// The first word, lowercased, and the rest.
fn split_command(query: &str) -> (String, &str) {
    let (word, rest) = query.split_once(char::is_whitespace).unwrap_or((query, ""));
    (word.to_lowercase(), rest.trim())
}

static SERIAL: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

impl MusicApp {
    /// Ctrl+K: opens Play anything empty, or closes it.
    pub fn toggle_play_anything(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.desktop.layers.palette.is_some() {
            self.close_play_anything(window, cx);
            return;
        }
        self.close_layers(window, cx);
        let input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Play anything: songs, albums, artists, commands")
        });
        let subscription = cx.subscribe_in(&input, window, Self::on_play_anything_input);
        input.update(cx, |state, cx| state.focus(window, cx));
        log::info!("Play anything opened");
        self.desktop.layers.palette = Some(PlayAnything {
            input,
            query: String::new(),
            selected: 0,
            asked: String::new(),
            catalogue: None,
            generation: 0,
            hits: Vec::new(),
            stamp: 0,
            note: None,
            serial: SERIAL.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
            debounce: None,
            _subscription: subscription,
        });
        self.refresh_hits();
        cx.notify();
    }

    pub fn close_play_anything(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.desktop.layers.palette.take().is_some() {
            window.focus(&self.focus, cx);
            cx.notify();
        }
    }

    fn on_play_anything_input(
        &mut self,
        state: &Entity<InputState>,
        event: &InputEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            InputEvent::Change => {
                let query = state.read(cx).value().to_string();
                if let Some(pa) = &mut self.desktop.layers.palette {
                    pa.query = query;
                    pa.selected = 0;
                    pa.note = None;
                }
                self.refresh_hits();
                self.ask_catalogue_later(cx);
            }
            InputEvent::PressEnter { shift, .. } => {
                let selected = self.desktop.layers.palette.as_ref().map(|p| p.selected);
                if let Some(i) = selected {
                    self.choose_hit(i, *shift, window, cx);
                }
            }
            InputEvent::Focus | InputEvent::Blur => {}
        }
        cx.notify();
    }

    /// The arrows: moves the highlight, wrapping.
    pub fn move_in_palette(&mut self, delta: isize, cx: &mut Context<Self>) {
        let Some(pa) = &mut self.desktop.layers.palette else {
            return;
        };
        let n = pa.hits.len() as isize;
        if n == 0 {
            return;
        }
        pa.selected = (pa.selected as isize + delta).rem_euclid(n) as usize;
        let song = match pa.hits.get(pa.selected).map(|h| &h.go) {
            Some(Go::Song(track)) => Some(track.video_id.clone()),
            _ => None,
        };
        if let Some(id) = song {
            // Ready before Enter.
            self.send(Command::Prepare(id));
        }
        cx.notify();
    }

    /// Asks YouTube Music once typing pauses.
    fn ask_catalogue_later(&mut self, cx: &mut Context<Self>) {
        let Some(pa) = &mut self.desktop.layers.palette else {
            return;
        };
        let Some(query) = catalogue_query(&pa.query) else {
            pa.debounce = None;
            return;
        };
        if query == pa.asked {
            return;
        }
        pa.debounce = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(DEBOUNCE).await;
            let _ = this.update(cx, |this, cx| {
                let Some(pa) = &mut this.desktop.layers.palette else {
                    return;
                };
                if catalogue_query(&pa.query).as_deref() == Some(query.as_str()) {
                    pa.asked = query.clone();
                    log::info!("Play anything: asking YouTube Music for {query:?}");
                    this.send(Command::QuickSearch(query));
                    cx.notify();
                }
            });
        }));
    }

    /// YouTube Music's answer; only the one for the latest search is kept.
    pub(crate) fn quick_results(&mut self, query: String, result: Result<Box<Page>, String>) {
        let Some(pa) = &mut self.desktop.layers.palette else {
            return;
        };
        if query != pa.asked {
            return;
        }
        match &result {
            Ok(page) => log::info!(
                "Play anything: {} shelves from YouTube Music for {query:?}",
                page.shelves.len()
            ),
            Err(e) => log::warn!("Play anything: YouTube Music failed: {e}"),
        }
        pa.catalogue = Some((query, result.map(|page| *page)));
        pa.generation += 1;
        self.refresh_hits();
    }

    /// Works the results out again when what they come from changed.
    pub fn refresh_hits(&mut self) {
        let Some(pa) = &self.desktop.layers.palette else {
            return;
        };
        let stamp = self.hits_stamp(pa);
        if stamp == pa.stamp {
            return;
        }
        let hits = rank(self, pa);
        if let Some(pa) = &mut self.desktop.layers.palette {
            pa.hits = hits;
            pa.stamp = stamp;
            pa.selected = pa.selected.min(pa.hits.len().saturating_sub(1));
        }
    }

    /// Changes whenever the results could: the text, YouTube Music's
    /// answer, the library and the page on screen.
    fn hits_stamp(&self, pa: &PlayAnything) -> u64 {
        let mut h = DefaultHasher::new();
        pa.query.hash(&mut h);
        pa.generation.hash(&mut h);
        self.pages.search.recent.len().hash(&mut h);
        let view = self.pages.view.target();
        view.key().hash(&mut h);
        for target in library_targets().chain(std::iter::once(view)) {
            if let Some(state) = self.pages.states.get(&target.key()) {
                state.fetched.hash(&mut h);
                state
                    .page
                    .as_ref()
                    .map(|p| p.shelves.iter().map(|s| s.items.len()).sum::<usize>())
                    .hash(&mut h);
            }
        }
        h.finish() | 1
    }

    /// Runs result `index`; `open` (Shift+Enter or Shift+click) opens its
    /// page instead of playing it.
    pub fn choose_hit(
        &mut self,
        index: usize,
        open: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(pa) = &self.desktop.layers.palette else {
            return;
        };
        let Some(hit) = pa.hits.get(index).cloned() else {
            return;
        };
        log::info!(
            "Play anything: chose {:?} ({}){}",
            hit.title,
            hit.kind.label(),
            if open { ", to open" } else { "" }
        );
        let songs: Vec<Track> = pa
            .hits
            .iter()
            .filter_map(|h| match &h.go {
                Go::Song(t) => Some(t.clone()),
                _ => None,
            })
            .collect();
        match hit.go {
            Go::Complete(text) => {
                if let Some(pa) = &self.desktop.layers.palette {
                    pa.input
                        .update(cx, |state, cx| state.set_value(text, window, cx));
                }
                return;
            }
            Go::Command(Cmd::Elsewhere(note)) => {
                if let Some(pa) = &mut self.desktop.layers.palette {
                    pa.note = Some(note);
                }
                return;
            }
            go => {
                self.close_play_anything(window, cx);
                self.run_hit(go, songs, open, window, cx);
            }
        }
        cx.notify();
    }

    fn run_hit(
        &mut self,
        go: Go,
        songs: Vec<Track>,
        open: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match go {
            Go::Song(track) => match (open, track.album.as_ref().and_then(|a| a.target.clone())) {
                (true, Some(album)) => self.open_link(album, cx),
                _ => {
                    let start = songs
                        .iter()
                        .position(|t| t.video_id == track.video_id)
                        .unwrap_or(0);
                    self.send(Command::PlayTracks {
                        tracks: songs,
                        start,
                    });
                }
            },
            Go::Collection { page, play } => match (page, play) {
                (Some(page), _) if open => self.open_link(page, cx),
                (_, Some(play)) => self.activate(play, cx),
                (Some(page), None) => self.play_from_page(page, FromPage::Play, cx),
                (None, None) => {}
            },
            Go::Artist(target) => self.open_link(target, cx),
            Go::Search(query) => self.run_search(query, window, cx),
            Go::Command(cmd) => self.run_command(cmd),
            Go::Complete(_) => {}
        }
    }

    fn run_command(&mut self, cmd: Cmd) {
        let playing = self.player.playback.playing;
        match cmd {
            Cmd::Radio(track) => self.send(Command::PlayTarget(control::song_radio(&track))),
            Cmd::Next => self.send(Command::Next),
            Cmd::Pause if playing => self.send(Command::TogglePause),
            Cmd::Play if !playing && !self.player.queue.is_empty() => {
                self.send(Command::TogglePause)
            }
            Cmd::Pause | Cmd::Play => {}
            Cmd::Shuffle => self.send(Command::ToggleShuffle),
            Cmd::Repeat => self.send(Command::CycleRepeat),
            Cmd::Elsewhere(_) => {}
        }
    }
}

/// Where a result comes from, in the order they rank among equals.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Source {
    Command,
    Library,
    Page,
    Recent,
    Catalogue,
    /// A command whose name starts with what's typed: offered after music.
    Suggestion,
}

struct Ranked {
    hit: Hit,
    source: Source,
    /// 0 exact title, 1 prefix, 2 word prefix, 3 inside, 4 every word
    /// somewhere, 5 only YouTube Music's word for it.
    tier: u8,
    order: usize,
}

fn tier(title: &str, detail: &str, q: &str) -> Option<u8> {
    let title = title.to_lowercase();
    if title == q {
        return Some(0);
    }
    if title.starts_with(q) {
        return Some(1);
    }
    if title.contains(q) {
        let starts_word = title.match_indices(q).any(|(i, _)| {
            title[..i]
                .chars()
                .next_back()
                .is_none_or(|c| !c.is_alphanumeric())
        });
        return Some(if starts_word { 2 } else { 3 });
    }
    let all = format!("{title} {}", detail.to_lowercase());
    q.split_whitespace()
        .all(|word| all.contains(word))
        .then_some(4)
}

/// A result for a card or row, if it is music.
fn hit_for(item: &Item) -> Option<Hit> {
    let detail: String = item.subtitle.iter().map(|r| r.text.as_str()).collect();
    if let Some(track) = &item.track {
        let artists = track.artist_line();
        return Some(Hit {
            key: format!("song:{}", track.video_id),
            title: item.title.clone(),
            detail: if artists.is_empty() { detail } else { artists },
            thumbnail: item.thumbnail.clone().or_else(|| track.thumbnail.clone()),
            kind: Kind::Song,
            go: Go::Song(track.clone()),
        });
    }
    let target = item.target.clone();
    let (kind, go) = match item.kind {
        ItemKind::Album | ItemKind::Playlist => (
            if item.kind == ItemKind::Album {
                Kind::Album
            } else {
                Kind::Playlist
            },
            Go::Collection {
                page: target
                    .clone()
                    .filter(|t| matches!(t, Target::Browse { .. })),
                play: item
                    .play
                    .clone()
                    .or_else(|| target.clone().filter(|t| matches!(t, Target::Watch { .. }))),
            },
        ),
        ItemKind::Artist => (Kind::Artist, Go::Artist(target.clone()?)),
        _ => return None,
    };
    Some(Hit {
        key: format!("page:{}", target?.key()),
        title: item.title.clone(),
        detail,
        thumbnail: item.thumbnail.clone(),
        kind,
        go,
    })
}

fn command_hit(key: &str, title: impl Into<String>, detail: &str, go: Go) -> Hit {
    Hit {
        key: format!("command:{key}"),
        title: title.into(),
        detail: detail.into(),
        thumbnail: None,
        kind: Kind::Command,
        go,
    }
}

/// Commands answered by another area (M6, M3) until it lands here.
const LATER: &str = "Not available yet";

/// Commands for what's typed: exact ones (and their arguments) rank first,
/// ones the text only begins are offered after the music.
fn commands(query: &str, out: &mut Vec<Ranked>) {
    let (word, rest) = split_command(query);
    let mut push = |hit: Hit, source: Source, tier: u8| {
        let order = out.len();
        out.push(Ranked {
            hit,
            source,
            tier,
            order,
        });
    };
    // TODO(M6): sleep, eq and mini call M6's sleep timer, equalizer and
    // mini player once they are on main.
    let elsewhere = |what| Go::Command(Cmd::Elsewhere(what));
    match word.as_str() {
        "sleep" if !rest.is_empty() => {
            let title = match rest.parse::<u32>() {
                Ok(m) if m > 0 => format!("Sleep in {m} minute{}", if m == 1 { "" } else { "s" }),
                _ if "end".starts_with(&rest.to_lowercase()) => {
                    "Sleep at the end of the song".to_string()
                }
                _ => "Sleep timer: sleep <minutes> or sleep end".to_string(),
            };
            let go = elsewhere("The sleep timer isn't available yet.");
            push(command_hit("sleep", title, LATER, go), Source::Command, 0);
            return;
        }
        "eq" if !rest.is_empty() => {
            let preset = ytfast::equalizer::Preset::ALL
                .into_iter()
                .find(|p| p.label().to_lowercase().starts_with(&rest.to_lowercase()));
            let title = match preset {
                Some(p) => format!("Equalizer: {}", p.label()),
                None => format!("Equalizer: {rest}"),
            };
            let go = elsewhere("The equalizer isn't available yet.");
            push(command_hit("eq", title, LATER, go), Source::Command, 0);
            return;
        }
        _ => {}
    }
    let simple: [(&str, &str, &str, Go); 7] = [
        (
            "like",
            "Like",
            LATER,
            // TODO(M3): like the playing song through M3's API.
            elsewhere("Liking isn't available yet."),
        ),
        (
            "next",
            "Next song",
            "Skip to the next song in Up next",
            Go::Command(Cmd::Next),
        ),
        (
            "pause",
            "Pause",
            "Pause the song playing now",
            Go::Command(Cmd::Pause),
        ),
        (
            "play",
            "Play",
            "Carry on with the paused song",
            Go::Command(Cmd::Play),
        ),
        (
            "shuffle",
            "Shuffle",
            "Turn shuffle on or off",
            Go::Command(Cmd::Shuffle),
        ),
        (
            "repeat",
            "Repeat",
            "Off, all or one",
            Go::Command(Cmd::Repeat),
        ),
        (
            "mini",
            "Mini player",
            LATER,
            elsewhere("The mini player isn't available yet."),
        ),
    ];
    let single = rest.is_empty();
    for (name, title, detail, go) in simple {
        if single && word == name {
            push(command_hit(name, title, detail, go), Source::Command, 0);
        } else if single && word.chars().count() >= 2 && name.starts_with(&word) {
            push(command_hit(name, title, detail, go), Source::Suggestion, 1);
        }
    }
    if single && word.chars().count() >= 2 {
        for (name, title, detail) in [
            ("radio", "Start a radio", "radio <song or artist>"),
            ("sleep", "Sleep timer", "sleep <minutes> or sleep end"),
            ("eq", "Equalizer preset", "eq <preset>"),
        ] {
            if name.starts_with(&word) {
                let exact = word == name;
                let source = if exact {
                    Source::Command
                } else {
                    Source::Suggestion
                };
                let go = Go::Complete(format!("{name} "));
                push(
                    command_hit(name, title, detail, go),
                    source,
                    u8::from(!exact),
                );
            }
        }
    }
}

/// The library pages loaded so far: Library's sections and Liked Music.
fn library_targets() -> impl Iterator<Item = Target> {
    LibraryTab::ALL
        .into_iter()
        .filter(|t| *t != LibraryTab::History)
        .map(LibraryTab::target)
        .chain(std::iter::once(Target::browse("VLLM")))
}

fn recent_hit(q: &str) -> Hit {
    Hit {
        key: format!("search:{q}"),
        title: q.to_string(),
        detail: "Recent search".into(),
        thumbnail: None,
        kind: Kind::Search,
        go: Go::Search(q.to_string()),
    }
}

/// The results for what's typed, best first.
fn rank(app: &MusicApp, pa: &PlayAnything) -> Vec<Hit> {
    let query = pa.query.trim();
    let recent = &app.pages.search.recent;
    if query.is_empty() {
        return recent.iter().take(5).map(|q| recent_hit(q)).collect();
    }
    let mut ranked: Vec<Ranked> = Vec::new();
    commands(query, &mut ranked);
    let (word, rest) = split_command(query);
    let radio = word == "radio" && !rest.is_empty();
    if (word == "sleep" || word == "eq") && !rest.is_empty() {
        return ranked.into_iter().map(|r| r.hit).collect();
    }
    let term = if radio { rest } else { query }.to_lowercase();
    let add = |item: &Item, source: Source, ranked: &mut Vec<Ranked>| {
        let Some(hit) = hit_for(item) else { return };
        let tier = match tier(&hit.title, &hit.detail, &term) {
            Some(t) => t,
            None if source == Source::Catalogue => 5,
            None => return,
        };
        let order = ranked.len();
        ranked.push(Ranked {
            hit,
            source,
            tier,
            order,
        });
    };
    let page_of =
        |target: &Target| -> Option<&Page> { app.pages.states.get(&target.key())?.page.as_ref() };
    let view = app.pages.view.target();
    let mut library_keys = Vec::new();
    for target in library_targets() {
        library_keys.push(target.key());
        if let Some(page) = page_of(&target) {
            // Liked Music's own list, not the Suggestions after it.
            let items: Vec<&Item> = match ytfast::account::entries(page) {
                Some(own) if target == Target::browse("VLLM") => own.items.iter().collect(),
                _ => page.shelves.iter().flat_map(|s| &s.items).collect(),
            };
            for item in items {
                add(item, Source::Library, &mut ranked);
            }
        }
    }
    if !library_keys.contains(&view.key())
        && let Some(page) = page_of(&view)
    {
        for item in page.shelves.iter().flat_map(|s| &s.items) {
            add(item, Source::Page, &mut ranked);
        }
    }
    if !radio {
        for q in recent {
            if let Some(tier) = tier(q, "", &term).filter(|t| *t <= 2) {
                let order = ranked.len();
                ranked.push(Ranked {
                    hit: recent_hit(q),
                    source: Source::Recent,
                    tier,
                    order,
                });
            }
        }
    }
    if let (Some((answered, Ok(page))), Some(wanted)) = (&pa.catalogue, catalogue_query(query))
        && (wanted.starts_with(answered.as_str()) || answered.starts_with(&wanted))
    {
        // YouTube Music's best: the top result, then a few of each kind.
        let mut taken = [0usize; 4];
        for item in page.shelves.iter().flat_map(|s| &s.items) {
            let slot = match (item.track.is_some(), item.kind) {
                (true, _) => 0,
                (false, ItemKind::Album) => 1,
                (false, ItemKind::Artist) => 2,
                (false, ItemKind::Playlist) => 3,
                _ => continue,
            };
            if taken[slot] < [6, 3, 2, 2][slot] {
                taken[slot] += 1;
                add(item, Source::Catalogue, &mut ranked);
            }
        }
    }
    // Exact and prefix title matches first, then by source, then closeness.
    ranked.sort_by_key(|r| (r.tier > 1, r.source, r.tier, r.order));
    let mut seen = HashSet::new();
    let mut hits: Vec<Hit> = ranked
        .into_iter()
        .filter(|r| seen.insert(r.hit.key.clone()))
        .map(|r| r.hit)
        .collect();
    if radio {
        hits.retain(|h| h.kind == Kind::Song);
        if let Some(Go::Song(track)) = hits.first().map(|h| h.go.clone()) {
            hits.insert(
                0,
                Hit {
                    key: format!("radio:{}", track.video_id),
                    title: format!("Start radio from “{}”", track.title),
                    detail: track.artist_line(),
                    thumbnail: track.thumbnail.clone(),
                    kind: Kind::Command,
                    go: Go::Command(Cmd::Radio(Box::new(track))),
                },
            );
        }
    }
    hits.truncate(SHOWN);
    hits
}
