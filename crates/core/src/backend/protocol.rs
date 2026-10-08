//! Commands and events crossing the backend/UI boundary.

use crate::equalizer::Equalizer;
use crate::model::{Account, Lyrics, Page, Playback, Sleep, Target, Track};
use crate::parse::More;

pub enum Command {
    /// Discover devices while the picker is open; false stops refreshing.
    CastScan(bool),
    CastConnect {
        id: String,
        kind: crate::casting::Kind,
        takeover: bool,
    },
    CastDisconnect,
    /// Open a page: cached copy first, then fresh. `seq` identifies the
    /// request; only the newest one's answer is used.
    Page {
        target: Target,
        seq: u64,
    },
    /// Load the next part of a page (`shelf: None`) or of one of its shelves.
    More {
        key: String,
        token: String,
        search: bool,
        shelf: Option<usize>,
    },
    Suggest(String),
    /// Lyrics for a song (timed when anyone has them). `browse_id` is its
    /// YouTube Music lyrics page if known; `duration` in seconds, 0 if unknown.
    Lyrics {
        track: Track,
        browse_id: Option<String>,
        duration: f64,
    },
    /// Read the recent searches; answered with [`Event::Searches`].
    LoadSearches,
    /// Save the recent searches, newest first.
    SaveSearches(Vec<String>),
    /// Play `tracks`, starting at `start`, as the queue.
    PlayTracks {
        tracks: Vec<Track>,
        start: usize,
    },
    /// Play a song radio, playlist, album or mix through watch-next.
    PlayTarget(Target),
    TogglePause,
    Next,
    Previous,
    Seek(f64),
    Volume(f64),
    ToggleShuffle,
    CycleRepeat,
    Autoplay(bool),
    /// Jump to a queue position (play order).
    JumpTo(usize),
    Reconnect,
    /// Resolve a song the pointer rests on, ahead of a likely click.
    Prepare(String),
    /// Use this browser profile's YouTube session from now on, and reconnect.
    UseProfile(String),
    /// Act as this channel of the account from now on (a page id from
    /// [`Event::Channels`]; `None` for the account's own channel), and
    /// reconnect.
    UseChannel(Option<String>),
    /// Sign in with a Netscape cookie file: copied into the config
    /// directory (answered with [`Event::CookiesSaved`]), then used.
    ImportCookies(std::path::PathBuf),
    /// Sign in with a pasted `Cookie` header, saved like an imported file.
    PasteCookies(String),
    /// Look through the browser profiles for a YouTube sign-in, answered
    /// with [`Event::BrowserScan`]. No request goes to YouTube.
    ScanBrowsers,
    /// Settings: song-change notifications on or off (saved for next time).
    Notifications(bool),
    /// Settings: Discord Rich Presence on or off (saved for next time).
    Discord(bool),
    /// A change to the signed-in account; `op` stamps the answer. On success
    /// the `refresh` pages are asked for again once YouTube Music shows it.
    AccountEdit {
        op: u64,
        edit: crate::account::Edit,
        refresh: Vec<Target>,
    },
    /// Fetch a song's rating on the account (answered with `Event::Likes`).
    LikeStatus(String),
    /// Insert songs right after the current one (before earlier additions).
    PlayNext(Vec<Track>),
    /// Queue songs after earlier additions, before the rest of the list.
    AddToQueue(Vec<Track>),
    /// Remove the song at a queue position (play order); not the current one.
    RemoveFromQueue(usize),
    /// Move the song at queue position `from` to `to` (play order; `to`
    /// counts after it is taken out).
    MoveInQueue {
        from: usize,
        to: usize,
    },
    /// Remove every song after the current one.
    ClearUpcoming,
    /// Set (`Some`) or cancel the sleep timer.
    SleepTimer(Option<Sleep>),
    Equalizer(Equalizer),
    /// Turn loudness levelling between songs on or off.
    Normalize(bool),
    /// Most-replayed heat for a song (by video id), asked once per song;
    /// answered with [`Event::Heat`].
    Heat(String),
    /// Audition: preview `track` over the ducked current song, from `start`
    /// seconds in (its best part; `None` plays from a third of the way in),
    /// until [`Command::EndAudition`]. Holding another song switches to it.
    /// Never touches the queue, the session or history.
    Audition {
        track: Track,
        start: Option<f64>,
    },
    /// The held song was let go: it fades out and the current song comes back.
    EndAudition,
    /// Settings: Smooth mixes on radios and mixes, and their length.
    Mixes(crate::model::Mixes),
    /// Search YouTube Music for Play anything (Ctrl+K): answered with
    /// [`Event::QuickResults`], never saved to disk.
    QuickSearch(String),
}

pub enum Event {
    Cast(crate::casting::State),
    Account(Account),
    Page {
        key: String,
        seq: u64,
        result: Result<Box<Page>, String>,
        cached: bool,
    },
    /// The answer to a continuation `token`.
    More {
        key: String,
        shelf: Option<usize>,
        token: String,
        result: Result<More, String>,
    },
    Suggestions {
        input: String,
        items: Vec<String>,
    },
    /// Lyrics for the song with video id `id`.
    Lyrics {
        id: String,
        result: Result<Option<Lyrics>, String>,
    },
    /// The saved recent searches, newest first.
    Searches(Vec<String>),
    /// The queue in play order.
    Queue(Vec<Track>),
    Playback(Playback),
    /// A readable error for the error strip.
    Error(String),
    /// The browser profiles signed in to YouTube, and the one in use.
    Profiles {
        list: Vec<crate::auth::Profile>,
        current: Option<String>,
    },
    /// The signed-in account's YouTube channels, the one requests act as
    /// marked `current`; empty while signed out.
    Channels(Vec<crate::model::Channel>),
    /// The answer to `Command::ImportCookies` or `PasteCookies`: the saved
    /// cookie file, which the backend then connects with, or why not.
    CookiesSaved(Result<crate::auth::Profile, String>),
    /// The answer to `Command::ScanBrowsers`.
    BrowserScan(crate::auth::BrowserScan),
    /// The answer to `Command::AccountEdit` number `op`.
    AccountEdited {
        op: u64,
        result: Result<crate::account::Done, crate::account::Failure>,
    },
    /// Ratings as YouTube Music returned them: (video id, rating).
    Likes(Vec<(String, crate::model::LikeStatus)>),
    /// Pages to fetch again after an account change.
    AccountRefresh(Vec<Target>),
    /// A song's most-replayed heat; `None` when it has none or the request failed.
    Heat {
        id: String,
        heat: Option<crate::heat::Heat>,
    },
    /// The answer to `Command::QuickSearch` for `query`.
    QuickResults {
        query: String,
        result: Result<Box<Page>, String>,
    },
}
