//! The interface's handle to a tokio runtime that does all I/O: InnerTube,
//! the browser session, stream resolving and the audio engine. The two sides talk only through
//! [`Command`] (interface → backend) and [`Event`] (backend → interface);
//! every event wakes the window.
//!
//! Playback state belongs to the worker. Results of asynchronous work carry
//! the stamp they were started under, so a late answer never acts on newer
//! state: `generation` changes with the current track, `epoch` with the
//! queue (each play request), and the player's playlist entry ids tell the
//! current file's events from those of replaced or queued ones. Several
//! players (decks of the audio engine, see [`crate::player`]) can run at once (Smooth mixes, Audition: see
//! [`deck`]); their events carry the deck's serial and reach its current
//! role.

mod account;
mod audition;
mod deck;
mod pages;
mod playback;
mod protocol;
mod queue;
mod resume;
mod session;
mod sound;

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use tokio::sync::mpsc;

use crate::equalizer::Equalizer;
use crate::innertube::{ApiError, Client, Stream};
use crate::model::{Account, Page, Playback, Repeat, Sleep, Target, Track, WatchNext};
use crate::parse;
use crate::paths::Paths;
use crate::player::{EndReason, Events, FileOptions, LoadMode, Player, PlayerEvent, Start};
use crate::resolver::{self, Resolver};

pub use protocol::{Command, Event};

#[derive(Clone)]
struct Sink {
    tx: std::sync::mpsc::Sender<Event>,
    wake: Arc<dyn Fn() + Send + Sync>,
    /// The desktop integration's copy of the queue and playback state.
    now: Arc<tokio::sync::watch::Sender<crate::desktop::Now>>,
}

impl Sink {
    fn send(&self, event: Event) {
        crate::desktop::observe(&self.now, &event);
        let _ = self.tx.send(event);
        (self.wake)();
    }
}

pub struct Backend {
    commands: mpsc::UnboundedSender<Command>,
    pub events: std::sync::mpsc::Receiver<Event>,
    pub http: reqwest::Client,
    pub runtime: tokio::runtime::Handle,
    /// The queue and playback state as last sent, for MPRIS, notifications
    /// and the command line (which work with no window open).
    pub now: tokio::sync::watch::Receiver<crate::desktop::Now>,
    resolver: Arc<Resolver>,
    shutdown: mpsc::UnboundedSender<std::sync::mpsc::Sender<()>>,
    _runtime: tokio::runtime::Runtime,
}

impl Backend {
    pub fn start(paths: Paths, wake: impl Fn() + Send + Sync + 'static) -> anyhow::Result<Self> {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .thread_name("encore-io")
            .build()?;
        let (commands, command_rx) = mpsc::unbounded_channel();
        let (shutdown, shutdown_rx) = mpsc::unbounded_channel();
        let (tx, events) = std::sync::mpsc::channel();
        let (now_tx, now) = tokio::sync::watch::channel(crate::desktop::Now::default());
        let sink = Sink {
            tx,
            wake: Arc::new(wake),
            now: Arc::new(now_tx),
        };
        let client = Arc::new(Client::new()?);
        let http = client.http().clone();
        let resolver = Arc::new(Resolver::new(paths.runtime.clone()));
        resolver.use_innertube(client.clone(), &paths);
        // The cookie copy that versions with yt-dlp kept here.
        let _ = std::fs::remove_file(paths.runtime.join("cookies.txt"));
        let worker = Worker::new(client, resolver.clone(), paths, sink);
        runtime.spawn(worker.run(command_rx, shutdown_rx));
        Ok(Self {
            commands,
            events,
            http,
            runtime: runtime.handle().clone(),
            now,
            resolver,
            shutdown,
            _runtime: runtime,
        })
    }

    pub fn send(&self, command: Command) {
        let _ = self.commands.send(command);
    }

    /// A sender for backend commands from other threads (MPRIS, the command line).
    pub fn commands(&self) -> mpsc::UnboundedSender<Command> {
        self.commands.clone()
    }

    /// Whether a song's stream is resolved, so a click on it starts at once.
    pub fn prepared(&self, video_id: &str) -> bool {
        self.resolver.cached(video_id).is_some()
    }

    /// A song's resolved stream URL (or `ENCORE_FAKE_STREAM`'s file), while
    /// it is valid: the waveform decodes it.
    pub fn stream_url(&self, video_id: &str) -> Option<String> {
        self.resolver.cached(video_id).map(|stream| stream.url)
    }

    /// Saves the session and stops playback. Runs when the backend is
    /// dropped; call it first if the process ends any other way.
    pub fn shutdown(&self) {
        let (done, wait) = std::sync::mpsc::channel();
        if self.shutdown.send(done).is_ok() {
            let _ = wait.recv_timeout(Duration::from_secs(2));
        }
    }
}

impl Drop for Backend {
    fn drop(&mut self) {
        self.shutdown();
    }
}

enum Internal {
    Connected(Account),
    AuthFailed,
    Started {
        generation: u64,
        stream: anyhow::Result<Stream>,
    },
    /// The next song resolved, with its player response when it was fetched.
    NextReady {
        generation: u64,
        /// Its queue entry.
        id: u64,
        video_id: String,
        stream: Stream,
        player: Option<sound::PlayerInfo>,
    },
    Watch {
        generation: u64,
        info: WatchNext,
    },
    Queue {
        epoch: u64,
        result: Result<WatchNext, String>,
    },
    /// More queue: a long playlist's next page, or the autoplay radio.
    Extended {
        epoch: u64,
        tracks: Vec<Track>,
        then_play: bool,
        autoplay: bool,
    },
    /// A track failed twice; `online` says whether YouTube is reachable.
    Failed {
        generation: u64,
        title: String,
        error: String,
        online: bool,
    },
    /// The connection is back after a failure while offline.
    Online {
        generation: u64,
    },
    /// A song's player response (loudness, play tracking).
    Player {
        video_id: String,
        info: sound::PlayerInfo,
    },
    /// The sleep timer's clock, for the timer `stamp`.
    SleepTick {
        stamp: u64,
    },
    /// Equalizer edits stopped (no newer one than `stamp`).
    EqualizerSettled {
        stamp: u64,
    },
    /// Audition and Smooth mixes: the volume clock, previews' streams.
    Deck(deck::Message),
}

/// The track queued in the player behind the current one.
#[derive(Clone)]
struct Appended {
    /// Its queue entry.
    id: u64,
    itag: u32,
    /// The player's playlist entry id.
    entry: i64,
    /// The loudness gain it was queued with.
    gain: Option<f64>,
}

struct Worker {
    client: Arc<Client>,
    resolver: Arc<Resolver>,
    paths: Paths,
    sink: Sink,
    internal_tx: mpsc::UnboundedSender<Internal>,
    internal_rx: Option<mpsc::UnboundedReceiver<Internal>>,
    /// Events of every player (deck), tagged with its serial.
    player_tx: Events,
    player_rx: Option<mpsc::UnboundedReceiver<(u64, PlayerEvent)>>,
    /// The main deck: the current song plays on it.
    main: Option<Arc<Player>>,
    last_connect: Option<Instant>,

    queue: queue::Queue,
    /// Position in the play order of the current track.
    pos: Option<usize>,
    appended: Option<Appended>,
    /// The player's playlist entry id of the current track.
    current_entry: Option<i64>,
    /// Bumped whenever the current track changes.
    generation: u64,
    /// Bumped whenever a play request replaces the queue.
    epoch: u64,
    retried: bool,
    /// Songs that failed one after the other; at three
    /// playback stops instead of skipping on through the queue.
    failed_in_row: u32,
    reported: bool,
    /// An autoplay radio fetch for the current epoch is in flight.
    extending: bool,
    /// Next was asked for while the radio was still loading.
    advance_pending: bool,
    /// A track failed while offline; it plays when the connection returns.
    waiting_for_network: bool,
    /// Account writes, made one at a time in the order asked (`account.rs`).
    account_writes: Option<mpsc::Sender<account::Write>>,
    state: Playback,
    last_emit: Instant,
    /// The player's pause and idle states: playing means neither.
    paused: bool,
    idle: bool,
    /// Where the current song starts when it next loads: a restored
    /// session's position, or a seek made before Play.
    resume_at: Option<f64>,
    /// The current song's resolve and the next song's prefetch, stopped
    /// when they no longer apply.
    resolving: Option<tokio::task::AbortHandle>,
    prefetching: Option<tokio::task::AbortHandle>,
    /// When the current song was asked for, to log how long it took to start.
    asked: Instant,
    /// Loudness and play tracking from player responses, by video id.
    players: HashMap<String, sound::PlayerInfo>,
    /// The equalizer every deck was set to as a whole; band edits since
    /// then went to the running decks.
    af: Option<Equalizer>,
    /// Bumped by every equalizer change.
    eq_stamp: u64,
    /// Bumped by every sleep timer change; its clock stops on a stale one.
    sleep_stamp: Arc<AtomicU64>,
    /// The sleep timer's fade: the share of the volume playing (1 = none).
    fade: f64,
    last_save: Instant,
    session_writer: Option<resume::Writer>,
    /// The other decks: Smooth mixes and Audition.
    decks: deck::Decks,
}

impl Worker {
    fn new(client: Arc<Client>, resolver: Arc<Resolver>, paths: Paths, sink: Sink) -> Self {
        let (internal_tx, internal_rx) = mpsc::unbounded_channel();
        let (player_tx, player_rx) = mpsc::unbounded_channel();
        let settings = crate::settings::Settings::load(&paths);
        Self {
            client,
            resolver,
            paths,
            sink,
            internal_tx,
            internal_rx: Some(internal_rx),
            player_tx,
            player_rx: Some(player_rx),
            main: None,
            last_connect: None,
            queue: queue::Queue::default(),
            pos: None,
            appended: None,
            current_entry: None,
            generation: 0,
            epoch: 0,
            retried: false,
            failed_in_row: 0,
            reported: false,
            extending: false,
            advance_pending: false,
            waiting_for_network: false,
            account_writes: None,
            state: Playback {
                volume: 100.0,
                autoplay: true,
                normalize: settings.normalizes(),
                equalizer: settings.equalizer,
                mixes: settings.mixes,
                ..Playback::default()
            },
            last_emit: Instant::now(),
            paused: false,
            idle: true,
            resume_at: None,
            resolving: None,
            prefetching: None,
            asked: Instant::now(),
            players: HashMap::new(),
            af: None,
            eq_stamp: 0,
            sleep_stamp: Arc::default(),
            fade: 1.0,
            last_save: Instant::now(),
            session_writer: None,
            decks: deck::Decks::new(settings.mixes),
        }
    }

    async fn run(
        mut self,
        mut commands: mpsc::UnboundedReceiver<Command>,
        mut shutdown: mpsc::UnboundedReceiver<std::sync::mpsc::Sender<()>>,
    ) {
        let (Some(mut internal), Some(mut player_events)) =
            (self.internal_rx.take(), self.player_rx.take())
        else {
            log::error!("backend started without its event receivers");
            return;
        };
        self.restore_session().await;
        self.connect();
        let mut stopped = None;
        loop {
            tokio::select! {
                command = commands.recv() => match command {
                    Some(command) => self.command(command).await,
                    None => break,
                },
                Some(message) = internal.recv() => self.internal(message).await,
                Some((serial, event)) = player_events.recv() => self.deck_event(serial, event).await,
                Some(done) = shutdown.recv() => {
                    stopped = Some(done);
                    break;
                }
            }
        }
        self.save_session(true);
        self.flush_session().await;
        if let Some(done) = stopped {
            // The shutdown caller may have reached its timeout already.
            let _ = done.send(());
        }
    }

    // ---- commands ----

    async fn command(&mut self, command: Command) {
        match command {
            Command::Page { target, seq } => self.load_page(target, seq),
            Command::More {
                key,
                token,
                search,
                shelf,
            } => {
                let client = self.client.clone();
                let sink = self.sink.clone();
                let internal = self.internal_tx.clone();
                tokio::spawn(async move {
                    let result = if search {
                        client.search_continuation(&token).await
                    } else {
                        client.continuation(&token).await
                    };
                    if matches!(result, Err(ApiError::Auth)) {
                        let _ = internal.send(Internal::AuthFailed);
                    }
                    let result = result.map(|v| parse::more(&v)).map_err(|e| e.to_string());
                    sink.send(Event::More {
                        key,
                        shelf,
                        token,
                        result,
                    });
                });
            }
            Command::Suggest(input) => {
                let client = self.client.clone();
                let sink = self.sink.clone();
                tokio::spawn(async move {
                    if let Ok(value) = client.suggestions(&input).await {
                        sink.send(Event::Suggestions {
                            items: parse::suggestions(&value),
                            input,
                        });
                    }
                });
            }
            Command::Lyrics {
                track,
                browse_id,
                duration,
            } => {
                let client = self.client.clone();
                let sink = self.sink.clone();
                tokio::spawn(async move {
                    let result = crate::lyrics::fetch(&client, &track, browse_id, duration).await;
                    sink.send(Event::Lyrics {
                        id: track.video_id,
                        result,
                    });
                });
            }
            Command::LoadSearches => {
                let path = self.paths.searches_file();
                let sink = self.sink.clone();
                tokio::spawn(async move {
                    sink.send(Event::Searches(crate::searches::load(&path).await));
                });
            }
            // Saved in order, here: a later list never lands before an earlier one.
            Command::SaveSearches(list) => {
                if let Err(error) = crate::searches::save(&self.paths.searches_file(), &list).await
                {
                    log::warn!("saving recent searches: {error}");
                }
            }
            Command::PlayTracks { tracks, start } => {
                self.new_epoch();
                self.set_queue(tracks, start);
                if let Some(pos) = self.pos {
                    self.start(pos).await;
                }
            }
            Command::PlayTarget(target) => {
                let epoch = self.new_epoch();
                self.decks.radio = deck::is_radio(&target);
                self.state.loading = true;
                self.emit(true);
                let client = self.client.clone();
                let tx = self.internal_tx.clone();
                tokio::spawn(async move {
                    let result = client.next(&target).await.map(|v| parse::watch_next(&v));
                    if matches!(result, Err(ApiError::Auth)) {
                        let _ = tx.send(Internal::AuthFailed);
                    }
                    let result = result.map_err(|e| e.to_string()).and_then(|info| {
                        if info.tracks.is_empty() {
                            Err("Nothing to play here".to_owned())
                        } else {
                            Ok(info)
                        }
                    });
                    let mut token = result
                        .as_ref()
                        .ok()
                        .and_then(|info| info.continuation.clone());
                    let mut total = result.as_ref().map(|info| info.tracks.len()).unwrap_or(0);
                    let _ = tx.send(Internal::Queue { epoch, result });
                    // Long playlists and albums: fetch the rest of the queue.
                    while let Some(t) = token.take().filter(|_| total < 500) {
                        let Ok(value) = client.next_continuation(&t).await else {
                            break;
                        };
                        let more = parse::watch_next(&value);
                        if more.tracks.is_empty() {
                            break;
                        }
                        total += more.tracks.len();
                        token = more.continuation.clone();
                        let extended = Internal::Extended {
                            epoch,
                            tracks: more.tracks,
                            then_play: false,
                            autoplay: false,
                        };
                        if tx.send(extended).is_err() {
                            break;
                        }
                    }
                });
            }
            Command::TogglePause => {
                if self.state.loading {
                    // Resolving: nothing to pause yet.
                } else if let (Some(player), false) = (self.main.clone(), self.idle) {
                    if self.state.playing {
                        // Pausing in a blend ends it: the new song pauses alone.
                        self.finish_blend().await;
                    }
                    player.set_pause(self.state.playing);
                } else if let Some(pos) = self.pos {
                    // Nothing loaded (a restored session, or the queue
                    // ended): play, from where a restored song was.
                    let at = self.resume_at.take();
                    self.start_at(pos, at).await;
                }
            }
            Command::Next => self.next(false).await,
            Command::Previous => {
                if self.state.position > 3.0 || self.pos == Some(0) {
                    self.seek(0.0).await;
                } else if let Some(pos) = self.pos {
                    self.start(pos - 1).await;
                }
            }
            Command::Seek(seconds) => self.seek(seconds).await,
            Command::Volume(volume) => {
                self.state.volume = volume.clamp(0.0, 100.0);
                self.apply_volumes().await;
                self.emit(true);
                self.save_session(false);
            }
            Command::ToggleShuffle => {
                self.state.shuffle = !self.state.shuffle;
                let shuffle = self.state.shuffle;
                self.edit_queue(|queue, pos, _| {
                    if let Some(pos) = pos {
                        if shuffle {
                            queue.shuffle(pos);
                        } else {
                            queue.unshuffle(pos);
                        }
                    }
                })
                .await;
            }
            Command::CycleRepeat => {
                self.state.repeat = match self.state.repeat {
                    Repeat::Off => Repeat::All,
                    Repeat::All => Repeat::One,
                    Repeat::One => Repeat::Off,
                };
                self.apply_loop().await;
                self.requeue_next().await;
                self.emit(true);
                self.save_session(true);
            }
            Command::Autoplay(on) => {
                self.state.autoplay = on;
                self.emit(true);
                self.save_session(true);
                self.maybe_extend();
            }
            Command::JumpTo(pos) => {
                if pos < self.queue.len() {
                    self.start(pos).await;
                }
            }
            Command::Reconnect => self.connect(),
            Command::UseProfile(profile) => {
                if let Err(error) = self
                    .change_settings(move |s| s.browser_profile = Some(profile))
                    .await
                {
                    self.sink.send(Event::Error(format!(
                        "Couldn't save the account choice: {error}"
                    )));
                }
                self.connect();
            }
            Command::UseChannel(page_id) => {
                if let Err(error) = self
                    .change_settings(move |s| s.channel = Some(page_id.unwrap_or_default()))
                    .await
                {
                    self.sink.send(Event::Error(format!(
                        "Couldn't save the channel choice: {error}"
                    )));
                }
                self.connect();
            }
            Command::ImportCookies(path) => {
                let saved =
                    tokio::task::spawn_blocking(move || crate::auth::import_cookie_file(&path))
                        .await
                        .unwrap_or_else(|error| {
                            Err(anyhow::anyhow!("cookie import worker stopped: {error}"))
                        });
                self.save_cookies(saved).await;
            }
            Command::PasteCookies(text) => {
                let saved =
                    tokio::task::spawn_blocking(move || crate::auth::store_cookie_header(&text))
                        .await
                        .unwrap_or_else(|error| {
                            Err(anyhow::anyhow!("cookie import worker stopped: {error}"))
                        });
                self.save_cookies(saved).await;
            }
            Command::ScanBrowsers => self.scan_browsers(),
            Command::Notifications(on) => {
                if let Err(error) = self.change_settings(move |s| s.notifications = on).await {
                    self.sink.send(Event::Error(format!(
                        "Couldn't save the notification setting: {error}"
                    )));
                }
            }
            Command::Discord(on) => {
                if let Err(error) = self.change_settings(move |s| s.discord = on).await {
                    self.sink.send(Event::Error(format!(
                        "Couldn't save the Discord setting: {error}"
                    )));
                }
            }
            Command::Heat(video_id) => {
                let http = self.client.http().clone();
                let sink = self.sink.clone();
                tokio::spawn(async move {
                    let heat = match crate::heat::fetch(&http, &video_id).await {
                        Ok(heat) => heat,
                        Err(error) => {
                            log::warn!("most replayed for {video_id}: {error}");
                            None
                        }
                    };
                    sink.send(Event::Heat { id: video_id, heat });
                });
            }
            Command::AccountEdit { op, edit, refresh } => self.account_edit(op, edit, refresh),
            Command::LikeStatus(video_id) => self.like_status(video_id),
            Command::Prepare(video_id) => self.resolver.prepare(&video_id),
            Command::PlayNext(tracks) => self.add(tracks, true).await,
            Command::AddToQueue(tracks) => self.add(tracks, false).await,
            Command::RemoveFromQueue(at) => {
                if self.pos != Some(at) {
                    self.edit_queue(|queue, _, _| {
                        queue.remove(at);
                    })
                    .await;
                }
            }
            Command::MoveInQueue { from, to } => {
                self.edit_queue(|queue, pos, shuffled| queue.move_entry(from, to, pos, shuffled))
                    .await;
            }
            Command::ClearUpcoming => {
                if self.pos.is_some() {
                    // Pages of the list and radio still on their way don't refill it.
                    self.epoch += 1;
                    self.extending = false;
                    self.advance_pending = false;
                    self.edit_queue(|queue, pos, _| {
                        if let Some(pos) = pos {
                            queue.clear_after(pos);
                        }
                    })
                    .await;
                }
            }
            Command::SleepTimer(choice) => self.set_sleep(choice).await,
            Command::Equalizer(equalizer) => self.set_equalizer(equalizer).await,
            Command::Normalize(on) => self.set_normalize(on).await,
            Command::Audition { track, start } => self.audition(track, start).await,
            Command::EndAudition => self.end_audition().await,
            Command::Mixes(mixes) => self.set_mixes(mixes).await,
            Command::QuickSearch(query) => {
                let client = self.client.clone();
                let sink = self.sink.clone();
                tokio::spawn(async move {
                    let result = client
                        .search(&query, None)
                        .await
                        .map(|v| Box::new(parse::page(&v)))
                        .map_err(|e| e.to_string());
                    sink.send(Event::QuickResults { query, result });
                });
            }
        }
    }

    /// Sends the playback state; position-only updates at most four times a second.
    fn emit(&mut self, always: bool) {
        if !always && self.last_emit.elapsed() < Duration::from_millis(250) {
            return;
        }
        self.last_emit = Instant::now();
        self.state.next_ready = self.appended.is_some() || self.decks.cued.is_some();
        self.sink.send(Event::Playback(self.state.clone()));
    }
}
