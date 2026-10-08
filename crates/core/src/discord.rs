//! M34: Discord Rich Presence (Settings, off by default). While a song
//! plays, Discord shows "Listening to <title>" with the artist, the cover,
//! the time bar and a button to the song on YouTube Music.
//!
//! It talks only to the Discord app on this computer, over its local IPC:
//! the `discord-ipc-N` socket in `$XDG_RUNTIME_DIR` (and the Flatpak and Snap
//! folders under it) on Linux, `$TMPDIR` on macOS and the
//! `\\?\pipe\discord-ipc-N` pipe on Windows. The `discord-rich-presence`
//! crate does the connecting and the handshake.
//!
//! Three parts, none of which can hold up the UI or the audio:
//! - [`follow`], a task on the backend runtime, turns the playing state
//!   ([`Now`]) into the presence that should show, and sends only changes
//!   (a new song, play or pause, a seek);
//! - a worker thread owns the connection. It connects when there is
//!   something to show, retries every [`RETRY`] while Discord isn't running
//!   (logging at debug only), and spaces updates out for Discord's rate limit;
//! - [`Handle`] carries the setting and the connection's state to Settings.
//!
//! Pausing clears the presence, as Spotify does. Quitting closes the socket,
//! which makes Discord drop the activity.

use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use discord_rich_presence::activity::{
    Activity, ActivityType, Assets, Button, StatusDisplayType, Timestamps,
};
use discord_rich_presence::{DiscordIpc, DiscordIpcClient};
use tokio::sync::watch;

use crate::desktop::{Flags, Now};

/// The Discord application whose name and icon the presence shows. The
/// maintainer registers an application named "Encore" at
/// https://discord.com/developers/applications and pastes its application id
/// (digits) here. Empty until then: Settings says Discord isn't set up in
/// this build, and nothing connects.
pub const DISCORD_CLIENT_ID: &str = "";

/// How long to wait before looking for Discord again.
const RETRY: Duration = Duration::from_secs(30);
/// The least time between two updates: Discord allows five in twenty seconds.
const MIN_GAP: Duration = if cfg!(test) {
    Duration::from_millis(100)
} else {
    Duration::from_secs(4)
};
/// A start time this far (ms) from where playback should have got to is a seek.
const SEEK_MS: i64 = 1500;
/// An end time this far (ms) off is the decoder refining the duration.
const LENGTH_MS: i64 = 2500;
/// Discord rejects text longer than this (bytes) and shorter than two characters.
const MAX_TEXT: usize = 128;

/// Whether this build has a Discord application to show as.
pub fn configured() -> bool {
    !DISCORD_CLIENT_ID.is_empty()
}

/// How the connection to Discord stands, for Settings.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    /// The setting is off.
    Off,
    /// On, but nothing has needed Discord yet (nothing has played).
    Idle,
    /// On and playing, but Discord isn't running; looking again every 30 s.
    Waiting,
    /// Discord answered.
    Connected,
}

impl Status {
    fn from_u8(value: u8) -> Self {
        match value {
            1 => Self::Idle,
            2 => Self::Waiting,
            3 => Self::Connected,
            _ => Self::Off,
        }
    }
}

/// The setting and the connection's state, shared by Settings and the task.
#[derive(Debug)]
pub struct Handle {
    enabled: watch::Sender<bool>,
    status: AtomicU8,
}

impl Default for Handle {
    fn default() -> Self {
        Self {
            enabled: watch::Sender::new(false),
            status: AtomicU8::new(Status::Off as u8),
        }
    }
}

impl Handle {
    pub fn enabled(&self) -> bool {
        *self.enabled.borrow()
    }

    pub fn set_enabled(&self, on: bool) {
        self.enabled.send_replace(on);
    }

    pub fn status(&self) -> Status {
        Status::from_u8(self.status.load(Ordering::Relaxed))
    }

    fn set_status(&self, status: Status) {
        self.status.store(status as u8, Ordering::Relaxed);
    }
}

/// What Discord should show.
#[derive(Clone, Debug, PartialEq)]
pub struct Presence {
    pub video_id: String,
    pub title: String,
    pub artist: String,
    /// YouTube's cover; Discord fetches https images itself.
    pub cover: Option<String>,
    /// Unix milliseconds playback started at (now minus the position).
    pub start: i64,
    /// Unix milliseconds the song ends at, once the length is known.
    pub end: Option<i64>,
}

impl Presence {
    /// The presence for the playing song; `None` when nothing plays (a
    /// pause clears it).
    pub fn of(now: &Now, unix_ms: i64) -> Option<Self> {
        let track = now.track()?;
        if !now.playback.playing {
            return None;
        }
        let duration = if now.playback.duration > 0.0 {
            now.playback.duration
        } else {
            track.duration.map_or(0.0, f64::from)
        };
        let start = unix_ms - (now.playback.position.max(0.0) * 1000.0) as i64;
        Some(Self {
            video_id: track.video_id.clone(),
            title: track.title.clone(),
            artist: track.artist_line(),
            cover: track
                .thumbnail
                .clone()
                .filter(|url| url.starts_with("https://")),
            start,
            end: (duration > 0.0).then(|| start + (duration * 1000.0) as i64),
        })
    }

    /// The song's page on YouTube Music.
    pub fn link(&self) -> String {
        format!("https://music.youtube.com/watch?v={}", self.video_id)
    }

    /// Whether Discord needs telling: another song, or a start or end time
    /// that moved (a seek).
    pub fn differs(&self, shown: &Self) -> bool {
        self.video_id != shown.video_id
            || self.title != shown.title
            || self.artist != shown.artist
            || self.cover != shown.cover
            || (self.start - shown.start).abs() > SEEK_MS
            || match (self.end, shown.end) {
                (Some(a), Some(b)) => (a - b).abs() > LENGTH_MS,
                (a, b) => a != b,
            }
    }

    /// The activity: type "Listening", the title as the status line, the
    /// artist under it, the time bar and the button.
    pub fn activity<'a>(&'a self, title: &'a str, artist: &'a str, link: &'a str) -> Activity<'a> {
        let mut times = Timestamps::new().start(self.start);
        if let Some(end) = self.end {
            times = times.end(end);
        }
        let mut activity = Activity::new()
            .activity_type(ActivityType::Listening)
            .status_display_type(StatusDisplayType::Details)
            .details(title)
            .state(artist)
            .timestamps(times)
            .buttons(vec![Button::new("Open on YouTube Music", link)]);
        if let Some(cover) = &self.cover {
            activity = activity.assets(Assets::new().large_image(cover.as_str()).large_text(title));
        }
        activity
    }
}

/// Text Discord accepts: at least two characters, at most 128 bytes.
fn text(value: &str) -> String {
    let mut out: String = value.trim().into();
    if out.len() > MAX_TEXT {
        let mut end = MAX_TEXT - 3;
        while !out.is_char_boundary(end) {
            end -= 1;
        }
        out.truncate(end);
        out.push_str("...");
    }
    while out.chars().count() < 2 {
        out.push('\u{a0}');
    }
    out
}

fn unix_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as i64)
}

enum Message {
    /// Show this, or clear (nothing is playing).
    Show(Option<Presence>),
    /// The setting went off: clear and disconnect.
    Off,
}

/// Follows the playing state and the setting until the runtime ends.
/// Without a Discord application id it does nothing.
pub fn start(runtime: &tokio::runtime::Handle, now: watch::Receiver<Now>, flags: Arc<Flags>) {
    if !configured() {
        log::info!("Discord Rich Presence: no application id in this build");
        return;
    }
    start_with(runtime, now, flags, DISCORD_CLIENT_ID.to_string());
}

fn start_with(
    runtime: &tokio::runtime::Handle,
    now: watch::Receiver<Now>,
    flags: Arc<Flags>,
    client_id: String,
) {
    let (tx, rx) = mpsc::channel();
    let worker = flags.clone();
    let spawned = std::thread::Builder::new()
        .name("discord".into())
        .spawn(move || Worker::new(client_id, rx, worker).run());
    if let Err(error) = spawned {
        log::warn!("no Discord Rich Presence: {error}");
        return;
    }
    runtime.spawn(follow(now, flags, tx));
}

async fn follow(mut now: watch::Receiver<Now>, flags: Arc<Flags>, tx: mpsc::Sender<Message>) {
    let mut enabled = flags.discord.enabled.subscribe();
    let mut shown: Option<Presence> = None;
    let mut was_on = false;
    loop {
        let on = *enabled.borrow_and_update();
        let next = if on {
            Presence::of(&now.borrow_and_update(), unix_ms())
        } else {
            now.borrow_and_update();
            None
        };
        let message = match (on, was_on) {
            (false, true) => Some(Message::Off),
            (true, false) => {
                flags.discord.set_status(Status::Idle);
                Some(Message::Show(next.clone()))
            }
            (true, true) if update_needed(&next, &shown) => Some(Message::Show(next.clone())),
            _ => None,
        };
        if let Some(message) = message {
            if tx.send(message).is_err() {
                return;
            }
            shown = next;
        }
        was_on = on;
        tokio::select! {
            changed = now.changed() => if changed.is_err() { break },
            changed = enabled.changed() => if changed.is_err() { break },
        }
    }
    let _ = tx.send(Message::Off);
}

fn update_needed(next: &Option<Presence>, shown: &Option<Presence>) -> bool {
    match (next, shown) {
        (None, None) => false,
        (Some(next), Some(shown)) => next.differs(shown),
        _ => true,
    }
}

/// Owns the connection to Discord.
struct Worker {
    client_id: String,
    rx: mpsc::Receiver<Message>,
    flags: Arc<Flags>,
    client: Option<DiscordIpcClient>,
    /// What should show, and whether Discord has been told yet.
    want: Option<Presence>,
    dirty: bool,
    last_sent: Option<Instant>,
    /// When to look for Discord again after it wasn't there.
    retry_at: Option<Instant>,
}

impl Worker {
    fn new(client_id: String, rx: mpsc::Receiver<Message>, flags: Arc<Flags>) -> Self {
        Self {
            client_id,
            rx,
            flags,
            client: None,
            want: None,
            dirty: false,
            last_sent: None,
            retry_at: None,
        }
    }

    fn run(mut self) {
        loop {
            let message = match self.wait() {
                Some(timeout) => match self.rx.recv_timeout(timeout) {
                    Ok(message) => Some(message),
                    Err(RecvTimeoutError::Timeout) => None,
                    Err(RecvTimeoutError::Disconnected) => break,
                },
                None => match self.rx.recv() {
                    Ok(message) => Some(message),
                    Err(_) => break,
                },
            };
            let mut messages: Vec<Message> = message.into_iter().collect();
            messages.extend(self.rx.try_iter());
            for message in messages {
                match message {
                    Message::Show(presence) => {
                        self.want = presence;
                        self.dirty = true;
                    }
                    Message::Off => {
                        self.want = None;
                        self.dirty = false;
                        self.disconnect();
                        self.flags.discord.set_status(Status::Off);
                    }
                }
            }
            self.apply();
        }
        self.disconnect();
    }

    /// How long to wait for the next message: until a retry or the end of
    /// the pause between updates; for ever when there's nothing to do.
    fn wait(&self) -> Option<Duration> {
        if !self.dirty {
            return None;
        }
        let due = match (&self.client, self.retry_at, self.last_sent) {
            (None, Some(at), _) => at,
            (Some(_), _, Some(sent)) => sent + MIN_GAP,
            _ => return Some(Duration::ZERO),
        };
        Some(due.saturating_duration_since(Instant::now()))
    }

    /// Tells Discord what should show, when it's time.
    fn apply(&mut self) {
        if !self.dirty {
            return;
        }
        if self.want.is_none() && self.client.is_none() {
            self.dirty = false;
            return;
        }
        if self.client.is_none()
            && (self.retry_at.is_some_and(|at| at > Instant::now()) || !self.connect())
        {
            return;
        }
        if self.last_sent.is_some_and(|sent| sent.elapsed() < MIN_GAP) {
            return;
        }
        let Some(client) = self.client.as_mut() else {
            return;
        };
        let sent = match &self.want {
            Some(presence) => {
                let (title, artist, link) = (
                    text(&presence.title),
                    text(&presence.artist),
                    presence.link(),
                );
                client.set_activity(presence.activity(&title, &artist, &link))
            }
            None => client.clear_activity(),
        };
        self.last_sent = Some(Instant::now());
        match sent {
            Ok(()) => self.dirty = false,
            Err(error) => {
                log::debug!("Discord: update failed, will reconnect: {error}");
                self.client = None;
                self.retry_at = Some(Instant::now() + RETRY);
                self.flags.discord.set_status(Status::Waiting);
            }
        }
    }

    fn connect(&mut self) -> bool {
        let mut client = DiscordIpcClient::new(self.client_id.as_str());
        match client.connect() {
            Ok(()) => {
                log::info!("Discord Rich Presence: connected");
                self.client = Some(client);
                self.retry_at = None;
                self.flags.discord.set_status(Status::Connected);
                true
            }
            Err(error) => {
                log::debug!("Discord isn't running ({error}); looking again in 30 s");
                self.retry_at = Some(Instant::now() + RETRY);
                self.flags.discord.set_status(Status::Waiting);
                false
            }
        }
    }

    fn disconnect(&mut self) {
        if let Some(mut client) = self.client.take() {
            let _ = client.clear_activity();
            let _ = client.close();
        }
        self.retry_at = None;
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "test assertions report fixture failures"
)]
mod tests {
    use super::*;
    use crate::model::{Run, Track};

    fn now(playing: bool, position: f64) -> Now {
        let mut now = Now::default();
        now.queue.push(Track {
            video_id: "abc123".into(),
            title: "Song".into(),
            artists: vec![Run {
                text: "Band".into(),
                target: None,
            }],
            album: None,
            thumbnail: Some("https://lh3.googleusercontent.com/cover".into()),
            duration: Some(200),
            like: None,
            set_video_id: None,
        });
        now.playback.index = Some(0);
        now.playback.playing = playing;
        now.playback.position = position;
        now
    }

    fn payload(presence: &Presence) -> serde_json::Value {
        let (title, artist, link) = (
            text(&presence.title),
            text(&presence.artist),
            presence.link(),
        );
        serde_json::to_value(presence.activity(&title, &artist, &link)).unwrap()
    }

    #[test]
    fn activity_lists_the_song_with_times_cover_and_button() {
        let presence = Presence::of(&now(true, 30.0), 1_000_000).unwrap();
        let json = payload(&presence);
        assert_eq!(json["type"], 2);
        assert_eq!(json["details"], "Song");
        assert_eq!(json["state"], "Band");
        assert_eq!(json["timestamps"]["start"], 970_000);
        assert_eq!(json["timestamps"]["end"], 1_170_000);
        assert_eq!(
            json["assets"]["large_image"],
            "https://lh3.googleusercontent.com/cover"
        );
        assert_eq!(json["buttons"][0]["label"], "Open on YouTube Music");
        assert_eq!(
            json["buttons"][0]["url"],
            "https://music.youtube.com/watch?v=abc123"
        );
    }

    #[test]
    fn a_pause_or_no_song_shows_nothing() {
        assert!(Presence::of(&now(false, 5.0), 0).is_none());
        assert!(Presence::of(&Now::default(), 0).is_none());
    }

    #[test]
    fn only_a_new_song_or_a_seek_is_news() {
        let shown = Presence::of(&now(true, 30.0), 1_000_000).unwrap();
        // A second later, a second further on: the same start.
        let later = Presence::of(&now(true, 31.0), 1_001_000).unwrap();
        assert!(!later.differs(&shown));
        let seeked = Presence::of(&now(true, 90.0), 1_001_000).unwrap();
        assert!(seeked.differs(&shown));
        let mut other = shown.clone();
        other.video_id = "other".into();
        other.title = "Other".into();
        assert!(other.differs(&shown));
    }

    #[test]
    fn text_fits_what_discord_accepts() {
        assert_eq!(text("A").chars().count(), 2);
        let long = text(&"é".repeat(200));
        assert!(long.len() <= MAX_TEXT && long.ends_with("..."));
    }

    #[cfg(unix)]
    #[test]
    fn talks_to_a_discord_socket() {
        use std::io::{Read, Write};
        use std::os::unix::net::UnixListener;

        // Set the child's environment before it starts. Mutating this process's
        // environment is unsafe while the parallel test runner has live threads.
        if std::env::var_os("ENCORE_DISCORD_TEST_CHILD").is_none() {
            let dir =
                std::env::temp_dir().join(format!("encore-discord-test-{}", std::process::id()));
            std::fs::create_dir_all(&dir).unwrap();
            let status = std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "discord::tests::talks_to_a_discord_socket",
                    "--nocapture",
                ])
                .env("ENCORE_DISCORD_TEST_CHILD", "1")
                .env("XDG_RUNTIME_DIR", &dir)
                .status()
                .unwrap();
            std::fs::remove_dir_all(&dir).unwrap();
            assert!(status.success());
            return;
        }
        let dir = std::path::PathBuf::from(std::env::var_os("XDG_RUNTIME_DIR").unwrap());
        std::fs::create_dir_all(&dir).unwrap();
        let socket = dir.join("discord-ipc-0");
        let _ = std::fs::remove_file(&socket);
        let listener = UnixListener::bind(&socket).unwrap();

        // A fake Discord: answers the handshake, records every frame.
        let (frames_tx, frames) = mpsc::channel::<(u32, serde_json::Value)>();
        std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            loop {
                let mut header = [0u8; 8];
                if stream.read_exact(&mut header).is_err() {
                    return;
                }
                let op = u32::from_le_bytes(header[..4].try_into().unwrap());
                let len = u32::from_le_bytes(header[4..].try_into().unwrap());
                let mut body = vec![0u8; len as usize];
                stream.read_exact(&mut body).unwrap();
                let json: serde_json::Value = serde_json::from_slice(&body).unwrap_or_default();
                if op == 0 {
                    let ready = br#"{"cmd":"DISPATCH","evt":"READY"}"#;
                    let mut reply = 1u32.to_le_bytes().to_vec();
                    reply.extend((ready.len() as u32).to_le_bytes());
                    reply.extend(ready);
                    stream.write_all(&reply).unwrap();
                }
                let _ = frames_tx.send((op, json));
            }
        });

        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        let (now_tx, now_rx) = watch::channel(Now::default());
        let flags = Arc::new(Flags::default());
        flags.discord.set_enabled(true);
        start_with(runtime.handle(), now_rx, flags.clone(), "424242".into());

        now_tx.send_replace(now(true, 12.0));
        let wait = Duration::from_secs(10);
        let (op, hello) = frames.recv_timeout(wait).unwrap();
        assert_eq!((op, hello["client_id"].as_str()), (0, Some("424242")));
        let (op, set) = frames.recv_timeout(wait).unwrap();
        assert_eq!((op, set["cmd"].as_str()), (1, Some("SET_ACTIVITY")));
        let activity = &set["args"]["activity"];
        assert_eq!(activity["details"], "Song");
        assert_eq!(activity["type"], 2);
        assert_eq!(flags.discord.status(), Status::Connected);

        // Pausing clears it.
        now_tx.send_replace(now(false, 20.0));
        let (op, clear) = frames.recv_timeout(wait).unwrap();
        assert_eq!(op, 1);
        assert!(clear["args"]["activity"].is_null());

        // Turning it off closes the connection.
        flags.discord.set_enabled(false);
        runtime.shutdown_timeout(Duration::from_secs(2));
    }
}
