//! M2: the queue and where playback is, the player bar's controls, Up next,
//! Now Playing and its lyrics.

use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use gpui_kit::component::slider::{SliderEvent, SliderState};
use gpui_kit::*;
use ytfast::backend::Command;
use ytfast::model::{Lyrics, Playback, Target, Track};

use crate::app::MusicApp;
use crate::nav::View;

/// The seek slider's scale: per mille of the song.
pub const SEEK_SCALE: f32 = 1000.0;

/// A seek is trusted over playback reports this long, or until a report
/// lands near it, so the handle doesn't jump back while mpv catches up.
const SEEK_GRACE: Duration = Duration::from_millis(1500);

/// How long a hand scroll of the lyrics holds the view before it follows
/// the song again.
pub const LYRICS_HOLD: Duration = Duration::from_secs(4);

/// Now Playing's tabs, as YouTube Music names them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Tab {
    #[default]
    UpNext,
    Lyrics,
    Related,
}

/// A queue list's scroll, and the song it last scrolled to, so it follows
/// the current song without fighting a hand scroll.
#[derive(Default)]
pub struct QueueScroll {
    pub handle: UniformListScrollHandle,
    pub followed: Option<usize>,
}

/// The lyrics view's scroll: where it eases to, and when a hand scroll
/// lets it follow the song again.
#[derive(Default)]
pub struct LyricsScroll {
    pub handle: ScrollHandle,
    pub held_until: Option<Instant>,
    /// The song the view was laid out for; a new song starts at the top.
    pub song: Option<String>,
    /// A redraw due when the next line starts, and that line's index.
    pub wake: Option<(usize, Task<()>)>,
}

/// Notified when only the song's position moved. The views that show the
/// position outside the page (the player bar, the mini player) observe it,
/// so a playback report or a clock tick doesn't re-render the whole window.
pub struct Clock;

pub struct Player {
    pub queue: Vec<Track>,
    pub clock: Entity<Clock>,
    pub playback: Playback,
    /// When `playback` arrived; the shown position moves on from there.
    playback_at: Instant,
    pub seek: Entity<SliderState>,
    pub volume: Entity<SliderState>,
    /// The seek slider is held: playback updates don't move it.
    pub seeking: bool,
    /// Where a seek went and when, until playback reports catch up.
    pending_seek: Option<(f64, Instant)>,
    /// The volume slider shows the restored volume (set once, then the
    /// slider leads).
    volume_synced: bool,
    /// The volume before Mute, to go back to.
    pub muted_from: Option<f64>,
    /// What the player bar last drew of the position ([`MusicApp::bar_shows`]).
    pub bar_shown: Option<(u64, Option<i64>)>,
    /// Now Playing fills the page area.
    pub now_playing: bool,
    /// The page view when Now Playing opened: navigating away closes it.
    pub now_playing_over: Option<View>,
    pub tab: Tab,
    /// The Up next panel is open beside the page.
    pub queue_open: bool,
    pub panel_scroll: QueueScroll,
    pub tab_scroll: QueueScroll,
    pub lyrics: HashMap<String, Result<Option<Lyrics>, String>>,
    lyrics_requested: HashSet<String>,
    pub lyrics_scroll: LyricsScroll,
    /// The artist or album link under the pointer: (place, link index).
    pub link_hover: Option<(&'static str, usize)>,
}

impl Player {
    pub fn new(_window: &mut Window, cx: &mut Context<MusicApp>) -> (Self, Vec<Subscription>) {
        let seek = cx.new(|_| SliderState::new().min(0.).max(SEEK_SCALE));
        let volume = cx.new(|_| SliderState::new().min(0.).max(100.).default_value(100.));
        let subscriptions = vec![
            cx.subscribe(&seek, |this, _, event: &SliderEvent, cx| match event {
                SliderEvent::Change(_) => this.player.seeking = true,
                SliderEvent::Release(value) => {
                    let to = f64::from(value.start() / SEEK_SCALE) * this.player.playback.duration;
                    this.seek_to(to, cx);
                }
            }),
            cx.subscribe(&volume, |this, _, event: &SliderEvent, _| {
                let (SliderEvent::Change(value) | SliderEvent::Release(value)) = event;
                this.player.muted_from = None;
                this.send(Command::Volume(f64::from(value.start())));
            }),
        ];
        (
            Self {
                queue: Vec::new(),
                clock: cx.new(|_| Clock),
                playback: Playback::default(),
                playback_at: Instant::now(),
                seek,
                volume,
                seeking: false,
                pending_seek: None,
                volume_synced: false,
                muted_from: None,
                bar_shown: None,
                now_playing: false,
                now_playing_over: None,
                tab: Tab::default(),
                queue_open: false,
                panel_scroll: QueueScroll::default(),
                tab_scroll: QueueScroll::default(),
                lyrics: HashMap::new(),
                lyrics_requested: HashSet::new(),
                lyrics_scroll: LyricsScroll::default(),
                link_hover: None,
            },
            subscriptions,
        )
    }

    /// Where the song is now: the last reported position, moved on by the
    /// time since while it plays (at most two seconds: past that, something
    /// is stuck).
    pub fn position(&self) -> f64 {
        let mut position = self.playback.position;
        if self.playback.playing && !self.playback.loading {
            position += self.playback_at.elapsed().as_secs_f64().min(2.0);
        }
        if self.playback.duration > 0.0 {
            position = position.min(self.playback.duration);
        }
        position
    }

    pub fn current(&self) -> Option<&Track> {
        self.playback.index.and_then(|i| self.queue.get(i))
    }

    /// The current song's lyrics: `None` until asked for and answered.
    pub fn current_lyrics(&self) -> Option<&Result<Option<Lyrics>, String>> {
        self.lyrics.get(&self.current()?.video_id)
    }
}

impl MusicApp {
    pub(crate) fn on_queue(&mut self, queue: Vec<Track>) {
        self.player.queue = queue;
    }

    /// Takes in a playback report; whether anything but the position
    /// changed (else only [`MusicApp::position_moved`] is due).
    pub(crate) fn on_playback(
        &mut self,
        mut playback: Playback,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let player = &mut self.player;
        let song_changed = playback.index != player.playback.index
            || player.current().map(|t| &t.video_id)
                != playback
                    .index
                    .and_then(|i| player.queue.get(i))
                    .map(|t| &t.video_id);
        if let Some((to, at)) = player.pending_seek {
            let caught_up = (playback.position - to).abs() < 1.5;
            if caught_up || at.elapsed() > SEEK_GRACE || song_changed {
                player.pending_seek = None;
            } else {
                playback.position = to;
            }
        }
        if !player.volume_synced {
            player.volume_synced = true;
            let volume = playback.volume as f32;
            player
                .volume
                .update(cx, |slider, cx| slider.set_value(volume, window, cx));
        }
        if song_changed {
            player.link_hover = None;
            if let Some((i, track)) = playback.index.and_then(|i| Some((i, player.queue.get(i)?))) {
                log::info!("now playing {} (queue {})", track.video_id, i + 1);
            }
        }
        let changed = !same_but_position(&player.playback, &playback);
        player.playback = playback;
        player.playback_at = Instant::now();
        // Stage draws the seek slider inside the app's views.
        if self.extras.stage.open {
            self.sync_seek(window, cx);
        }
        self.prefetch_lyrics();
        changed
    }

    /// Moves the seek slider to the position. The player bar does this as
    /// it renders: a slider change asks for a window frame of its own, so a
    /// playback report doesn't move it.
    pub(crate) fn sync_seek(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let player = &self.player;
        let duration = player.playback.duration;
        if player.seeking || duration <= 0.0 {
            return;
        }
        let at = (player.position() / duration) as f32 * SEEK_SCALE;
        player
            .seek
            .update(cx, |slider, cx| slider.set_value(at, window, cx));
    }

    /// Redraws what shows the position. Stage, and Now Playing's waveform
    /// unless the effects layer paints it, draw it inside the app's views;
    /// otherwise only the player bar and the mini player do, and they watch
    /// [`Clock`]. While the effects layer draws the seek bar, its frames
    /// show the position, and the shell redraws the bar in one of them once
    /// what the bar shows changed ([`MusicApp::bar_shows`]): a tick needs
    /// no frame of its own.
    pub(crate) fn position_moved(&mut self, cx: &mut Context<Self>) {
        let in_views = self.player.now_playing && !crate::visuals::paints_waveform(self);
        if in_views || self.extras.stage.open {
            cx.notify();
        } else if self.extras.mini_open() || !crate::visuals::position_moved(self.bar_moved(cx), cx)
        {
            self.player.clock.update(cx, |_, cx| cx.notify());
        }
    }

    /// Whether the player bar would show the position differently from
    /// what it last drew.
    fn bar_moved(&self, cx: &App) -> bool {
        self.player.bar_shown != Some(self.bar_shows(cx))
    }

    /// What the player bar draws of the position itself: the elapsed time's
    /// second and, with the ridge (unless the effects layer draws it), its
    /// playhead to the pixel.
    pub(crate) fn bar_shows(&self, cx: &App) -> (u64, Option<i64>) {
        let position = self.player.position();
        let duration = self.player.playback.duration;
        let ridge = self
            .current_heat()
            .filter(|_| duration > 0.0 && !crate::visuals::paints_bar(cx))
            .and_then(|_| crate::visuals::seek_width(cx))
            .map(|width| (position / duration * f64::from(f32::from(width))) as i64);
        (position as u64, ridge)
    }

    pub(crate) fn on_lyrics(&mut self, id: String, result: Result<Option<Lyrics>, String>) {
        self.player.lyrics.insert(id, result);
    }

    /// Seeks to `seconds`, and shows it at once.
    pub fn seek_to(&mut self, seconds: f64, cx: &mut Context<Self>) {
        log::info!("seek to {seconds:.1}s");
        let player = &mut self.player;
        player.seeking = false;
        player.pending_seek = Some((seconds, Instant::now()));
        player.playback.position = seconds;
        player.playback_at = Instant::now();
        self.send(Command::Seek(seconds));
        cx.notify();
    }

    /// Mute keeps the volume to go back to; Unmute restores it.
    pub fn toggle_mute(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let player = &mut self.player;
        let to = match player.muted_from.take() {
            Some(before) => before,
            None if player.playback.volume > 0.0 => {
                player.muted_from = Some(player.playback.volume);
                0.0
            }
            // Already silent from the slider: Unmute to a sensible level.
            None => 50.0,
        };
        player.playback.volume = to;
        player
            .volume
            .update(cx, |slider, cx| slider.set_value(to as f32, window, cx));
        self.send(Command::Volume(to));
        cx.notify();
    }

    /// Opens or closes Now Playing (only with something in the queue).
    pub fn show_now_playing(&mut self, open: bool, cx: &mut Context<Self>) {
        let open = open && !self.player.queue.is_empty();
        self.player.now_playing = open;
        self.player.now_playing_over = open.then(|| self.pages.view.clone());
        self.player.tab_scroll.followed = None;
        if open && self.player.tab == Tab::Lyrics {
            self.request_current_lyrics();
        }
        cx.notify();
    }

    pub fn set_tab(&mut self, tab: Tab, cx: &mut Context<Self>) {
        self.player.tab = tab;
        match tab {
            Tab::UpNext => self.player.tab_scroll.followed = None,
            Tab::Lyrics => {
                self.player.lyrics_scroll.song = None;
                self.request_current_lyrics();
            }
            Tab::Related => {
                if let Some(id) = self.player.playback.related.clone() {
                    self.ensure_page(Target::browse(id), false);
                }
            }
        }
        cx.notify();
    }

    /// The player bar's Up next button: the panel beside the page, or the
    /// Up next tab while Now Playing is open.
    pub fn toggle_up_next(&mut self, cx: &mut Context<Self>) {
        if self.player.now_playing {
            self.set_tab(Tab::UpNext, cx);
            return;
        }
        self.player.queue_open = !self.player.queue_open;
        self.player.panel_scroll.followed = None;
        cx.notify();
    }

    /// Opens a link from the player (an artist, an album): Now Playing
    /// makes way for the page.
    pub fn open_link(&mut self, target: Target, cx: &mut Context<Self>) {
        self.player.now_playing = false;
        self.player.now_playing_over = None;
        self.activate(target, cx);
    }

    /// Edits the queue: shown at once, the backend's queue follows.
    pub fn edit_queue(&mut self, command: Command, cx: &mut Context<Self>) {
        let player = &mut self.player;
        let current = player.playback.index;
        let queue = &mut player.queue;
        match command {
            Command::MoveInQueue { from, to } if from < queue.len() => {
                let track = queue.remove(from);
                let to = to.min(queue.len());
                queue.insert(to, track);
                player.playback.index = current.map(|c| moved_index(c, from, to));
            }
            Command::RemoveFromQueue(at) if at < queue.len() && current != Some(at) => {
                queue.remove(at);
                player.playback.index = current.map(|c| if at < c { c - 1 } else { c });
            }
            Command::ClearUpcoming => {
                if let Some(c) = current {
                    queue.truncate(c + 1);
                }
            }
            Command::Autoplay(on) => player.playback.autoplay = on,
            _ => {}
        }
        log::info!(
            "queue edited ({}): {}",
            edit_name(&command),
            queue
                .iter()
                .take(8)
                .map(|t| t.title.as_str())
                .collect::<Vec<_>>()
                .join(" | ")
        );
        self.send(command);
        cx.notify();
    }

    /// Asks once for the lyrics of the song playing now.
    pub fn request_current_lyrics(&mut self) {
        let Some(track) = self.player.current().cloned() else {
            return;
        };
        if !self.player.lyrics_requested.insert(track.video_id.clone()) {
            return;
        }
        if let Some(lyrics) = fake_lyrics() {
            self.on_lyrics(track.video_id, Ok(Some(lyrics)));
            return;
        }
        let playback = &self.player.playback;
        let duration = track.duration.map(f64::from).unwrap_or(playback.duration);
        let browse_id = playback.lyrics.clone();
        self.send(Command::Lyrics {
            track,
            browse_id,
            duration,
        });
    }

    /// Forgets a failed lyrics answer for the current song and asks again.
    pub fn retry_lyrics(&mut self, cx: &mut Context<Self>) {
        if let Some(id) = self.player.current().map(|t| t.video_id.clone()) {
            self.player.lyrics.remove(&id);
            self.player.lyrics_requested.remove(&id);
            self.request_current_lyrics();
        }
        cx.notify();
    }

    /// Lyrics are asked for ahead of the Lyrics tab: once YouTube Music has
    /// named the song's lyrics page, or a moment into the song if it hasn't
    /// (then only LRCLIB may have them), or at once while the tab shows.
    fn prefetch_lyrics(&mut self) {
        let player = &self.player;
        let showing = player.now_playing && player.tab == Tab::Lyrics;
        if showing || player.playback.lyrics.is_some() || player.playback.position >= 3.0 {
            self.request_current_lyrics();
        }
    }
}

/// Whether two playback reports differ only in the position. The fields
/// are listed out, so a new one has to be placed here.
fn same_but_position(a: &Playback, b: &Playback) -> bool {
    let Playback {
        index,
        playing,
        loading,
        position: _,
        duration,
        volume,
        shuffle,
        repeat,
        autoplay,
        format,
        lyrics,
        related,
        next_ready,
        sleep,
        normalize,
        gain,
        equalizer,
        audition,
        mixes,
        player,
        engine,
    } = a;
    *index == b.index
        && *playing == b.playing
        && *loading == b.loading
        && *duration == b.duration
        && *volume == b.volume
        && *shuffle == b.shuffle
        && *repeat == b.repeat
        && *autoplay == b.autoplay
        && *format == b.format
        && *lyrics == b.lyrics
        && *related == b.related
        && *next_ready == b.next_ready
        && *sleep == b.sleep
        && *normalize == b.normalize
        && *gain == b.gain
        && *equalizer == b.equalizer
        && *audition == b.audition
        && *mixes == b.mixes
        && *player == b.player
        && *engine == b.engine
}

/// A queue edit, for the log.
fn edit_name(command: &Command) -> String {
    match command {
        Command::MoveInQueue { from, to } => format!("move {from} to {to}"),
        Command::RemoveFromQueue(at) => format!("remove {at}"),
        Command::ClearUpcoming => "clear upcoming".into(),
        Command::Autoplay(on) => format!("autoplay {on}"),
        _ => "other".into(),
    }
}

/// Where the song at `c` ends up when the one at `from` moves to `to`.
fn moved_index(c: usize, from: usize, to: usize) -> usize {
    if c == from {
        return to;
    }
    let c = if from < c { c - 1 } else { c };
    if to <= c { c + 1 } else { c }
}

/// An error as the strip shows it: the plain sentence, and the technical
/// detail behind Copy details (yt-dlp's or mpv's own words), if any.
pub fn split_error(error: &str) -> (String, Option<String>) {
    // "Couldn't play “Title”, skipped it. ERROR: …"
    if let Some(end) = error.find("skipped it.") {
        let end = end + "skipped it.".len();
        let detail = error[end..].trim();
        return (
            error[..end].to_string(),
            (!detail.is_empty()).then(|| detail.to_string()),
        );
    }
    // "Couldn't start playback: …": the cause after the first colon that
    // isn't inside a quoted title.
    let after_quote = error.rfind('”').map_or(0, |i| i + '”'.len_utf8());
    if let Some(colon) = error[after_quote..].find(": ") {
        let colon = after_quote + colon;
        return (
            format!("{}.", error[..colon].trim_end_matches('.')),
            Some(error[colon + 2..].trim().to_string()),
        );
    }
    (error.to_string(), None)
}

/// "Opus 256 kbps · Premium" from "Opus 256 kbps · Premium (itag 774)":
/// the itag stays in the tooltip.
pub fn short_format(full: &str) -> String {
    full.split(" (itag").next().unwrap_or(full).to_string()
}

/// Shortcuts for this area, in the "Music" key context.
pub fn bind_keys(_cx: &mut App) {}

/// Handlers for this area's actions, on the window's root element.
pub fn on_actions(root: Div, _cx: &mut Context<MusicApp>) -> Div {
    root
}

/// For checks with `YTFAST_FAKE_STREAM`: `YTFAST_GPUI_FAKE_LYRICS=<file.lrc>`
/// gives every song those timed lyrics, so nothing is fetched.
fn fake_lyrics() -> Option<Lyrics> {
    let path = std::env::var_os("YTFAST_GPUI_FAKE_LYRICS")?;
    let text = std::fs::read_to_string(&path)
        .inspect_err(|e| log::warn!("YTFAST_GPUI_FAKE_LYRICS: {e}"))
        .ok()?;
    let lines = ytfast::lyrics::parse_lrc(&text);
    let plain = lines.iter().map(|l| l.text.as_str()).collect::<Vec<_>>();
    Some(Lyrics {
        text: plain.join("\n"),
        source: Some("Test lyrics".into()),
        lines,
    })
}
