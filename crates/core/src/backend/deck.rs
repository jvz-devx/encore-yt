//! More than one player at once ("decks"): Smooth mixes, and the volume of
//! every deck (ducking under an audition, blends, the sleep timer's fade).
//!
//! The current song always plays on `Worker::main`, the main deck, and the
//! main deck's events drive playback as they always did. With Smooth mixes
//! on, when the song after the current one is part of a radio, a mix or
//! autoplay, it is cued paused and silent on a second deck instead of being
//! appended behind the current song. Shortly before the current song ends
//! the cued deck starts and becomes the main deck at once: the song change
//! happens there, with every stamp of a gapless change (generation, queue
//! position, the new main deck's playlist entry id). The old main deck plays
//! out as the tail, fading, and its events only end the blend. The two
//! fades are equal power: amplitudes `sin` and `cos` of the progress, which
//! is read from the incoming song's own position, so pausing or buffering
//! holds the blend. When the blend is done the tail is stopped and kept as
//! the spare deck for the next cue.
//!
//! A player's volume maps cubically to amplitude, so a deck playing at an amplitude share `a`
//! of the user's volume `V` is set to `V·∛a`. Per-song loudness gains stay
//! per-file options on each deck.

use std::collections::HashSet;
use std::f64::consts::FRAC_PI_2;

use super::*;
use crate::model::Mixes;

/// The amplitude the current song keeps under an audition (−14 dB).
pub(super) const DUCK: f64 = 0.2;
/// How long ducking and an audition's fades take.
pub(super) const FADE: Duration = Duration::from_millis(250);
/// The volume clock's period while anything fades.
const TICK: Duration = Duration::from_millis(20);

pub(super) enum Message {
    /// The volume clock, for the clock `stamp`.
    Tick { stamp: u64 },
    /// An audition's stream, for the request `stamp`.
    AuditionReady {
        stamp: u64,
        stream: anyhow::Result<Stream>,
    },
    /// No audition since request `stamp` for a while: stop its deck.
    AuditionIdle { stamp: u64 },
}

/// An amplitude share moving from one value to another.
#[derive(Clone, Copy)]
pub(super) struct Ramp {
    from: f64,
    to: f64,
    start: Instant,
    length: Duration,
}

impl Ramp {
    pub fn steady(value: f64) -> Self {
        Self {
            from: value,
            to: value,
            start: Instant::now(),
            length: Duration::ZERO,
        }
    }

    /// From wherever it is now to `to`, over `length`.
    pub fn toward(self, to: f64, length: Duration) -> Self {
        Self {
            from: self.now(),
            to,
            start: Instant::now(),
            length,
        }
    }

    pub fn now(&self) -> f64 {
        if self.length.is_zero() {
            return self.to;
        }
        let k = (self.start.elapsed().as_secs_f64() / self.length.as_secs_f64()).clamp(0.0, 1.0);
        self.from + (self.to - self.from) * k * k * (3.0 - 2.0 * k)
    }

    pub fn target(&self) -> f64 {
        self.to
    }

    pub fn moving(&self) -> bool {
        self.start.elapsed() < self.length && self.from != self.to
    }
}

/// The next song, cued paused and silent on a second deck.
pub(super) struct Cued {
    pub deck: Arc<Player>,
    pub next: Appended,
}

/// The song before, playing out under the new one.
pub(super) struct Tail {
    deck: Arc<Player>,
    /// Seconds the blend lasts, on the incoming song's clock.
    length: f64,
}

pub(super) struct Decks {
    pub mixes: Mixes,
    /// The queue is a radio or a mix (an `RD…` playlist, Start radio).
    pub radio: bool,
    /// Queue entries autoplay added: their changes blend too.
    pub autoplay: HashSet<u64>,
    pub cued: Option<Cued>,
    tail: Option<Tail>,
    /// A stopped deck kept for the next cue.
    spare: Option<Arc<Player>>,
    /// The main and tail decks' share under an audition.
    pub duck: Ramp,
    pub audition: super::audition::Audition,
    /// When the main deck last reported its position.
    position_at: Instant,
    /// The volume clock: running, and the stamp that stops a stale one.
    ticking: bool,
    clock: Arc<AtomicU64>,
}

impl Decks {
    pub fn new(mixes: Mixes) -> Self {
        Self {
            mixes,
            radio: false,
            autoplay: HashSet::new(),
            cued: None,
            tail: None,
            spare: None,
            duck: Ramp::steady(1.0),
            audition: Default::default(),
            position_at: Instant::now(),
            ticking: false,
            clock: Arc::default(),
        }
    }

    pub fn blending(&self) -> bool {
        self.tail.is_some()
    }
}

/// Whether a play request starts a radio or a mix.
pub(super) fn is_radio(target: &Target) -> bool {
    matches!(target, Target::Watch { playlist_id: Some(id), .. } if id.starts_with("RD"))
}

impl super::Worker {
    // ---- routing ----

    /// An event from one of the decks, by its serial.
    pub(super) async fn deck_event(&mut self, serial: u64, event: PlayerEvent) {
        let is = |player: Option<&Arc<Player>>| player.is_some_and(|m| m.serial() == serial);
        if is(self.main.as_ref()) {
            self.main_event(event).await;
        } else if is(self.decks.tail.as_ref().map(|t| &t.deck)) {
            // The old song played out: the blend is over.
            if let PlayerEvent::EndFile { .. } = event {
                self.finish_blend().await;
            }
        } else if is(self.decks.cued.as_ref().map(|c| &c.deck)) {
            if let PlayerEvent::EndFile {
                reason: EndReason::Error,
                ..
            } = event
            {
                log::warn!("the cued song failed; it will start the usual way");
                if let Some(cued) = self.decks.cued.take() {
                    self.decks.spare = Some(cued.deck);
                }
                self.emit(true);
            }
        } else if is(self.decks.audition.deck()) {
            self.audition_event(event).await;
        }
    }

    pub(super) async fn deck_message(&mut self, message: Message) {
        match message {
            Message::Tick { stamp } => self.tick(stamp).await,
            Message::AuditionReady { stamp, stream } => self.audition_ready(stamp, stream).await,
            Message::AuditionIdle { stamp } => self.audition_idle(stamp),
        }
    }

    /// Every running player: the equalizer goes to all of them.
    pub(super) fn decks(&self) -> Vec<Arc<Player>> {
        self.main
            .iter()
            .chain(self.decks.tail.as_ref().map(|t| &t.deck))
            .chain(self.decks.cued.as_ref().map(|c| &c.deck))
            .chain(self.decks.spare.as_ref())
            .chain(self.decks.audition.deck())
            .cloned()
            .collect()
    }

    // ---- volumes ----

    /// The blend's progress, 0 to 1, while one runs.
    fn blend_progress(&self) -> Option<f64> {
        let tail = self.decks.tail.as_ref()?;
        let mut position = self.state.position;
        if self.state.playing && !self.state.loading {
            position += self.decks.position_at.elapsed().as_secs_f64().min(0.5);
        }
        Some((position / tail.length).clamp(0.0, 1.0))
    }

    /// The volume the main deck plays at.
    pub(super) fn main_volume(&self) -> f64 {
        let blend = self.blend_progress().map_or(1.0, |t| (t * FRAC_PI_2).sin());
        self.state.volume * self.fade * (self.decks.duck.now() * blend).cbrt()
    }

    fn tail_volume(&self) -> f64 {
        let blend = self.blend_progress().map_or(0.0, |t| (t * FRAC_PI_2).cos());
        self.state.volume * self.fade * (self.decks.duck.now() * blend).cbrt()
    }

    /// Sets every playing deck's volume from the user's volume, the sleep
    /// fade, the duck, the blend and the audition's fades.
    pub(super) async fn apply_volumes(&mut self) {
        if let Some(player) = &self.main {
            player.set_volume(self.main_volume());
        }
        if let Some(tail) = &self.decks.tail {
            tail.deck.set_volume(self.tail_volume());
        }
        if let Some(deck) = self.decks.audition.deck() {
            let volume = self.audition_volume();
            deck.set_volume(volume);
        }
    }

    fn fading(&self) -> bool {
        self.decks.tail.is_some() || self.decks.duck.moving() || self.decks.audition.fading()
    }

    /// Runs the volume clock while anything fades.
    pub(super) fn keep_time(&mut self) {
        if self.decks.ticking {
            return;
        }
        self.decks.ticking = true;
        let stamp = self.decks.clock.fetch_add(1, Ordering::SeqCst) + 1;
        let clock = self.decks.clock.clone();
        let tx = self.internal_tx.clone();
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(TICK).await;
                if clock.load(Ordering::SeqCst) != stamp
                    || tx.send(Internal::Deck(Message::Tick { stamp })).is_err()
                {
                    return;
                }
            }
        });
    }

    async fn tick(&mut self, stamp: u64) {
        if stamp != self.decks.clock.load(Ordering::SeqCst) {
            return;
        }
        if self.blend_progress().is_some_and(|t| t >= 1.0) {
            self.finish_blend().await;
        }
        self.apply_volumes().await;
        self.audition_faded().await;
        if !self.fading() {
            self.decks.ticking = false;
            self.decks.clock.fetch_add(1, Ordering::SeqCst);
        }
    }

    // ---- smooth mixes ----

    pub(super) async fn set_mixes(&mut self, mixes: Mixes) {
        let mixes = Mixes {
            on: mixes.on,
            seconds: mixes.seconds.clamp(Mixes::SHORTEST, Mixes::LONGEST),
        };
        self.state.mixes = mixes;
        self.decks.mixes = mixes;
        self.update_settings(|s| s.mixes = mixes);
        self.emit(true);
        self.requeue_next().await;
        if !mixes.on {
            self.decks.spare = None;
        }
    }

    /// After a change to what blends (Smooth mixes, repeat): the song after
    /// the current one is queued again if it now changes the other way.
    pub(super) async fn requeue_next(&mut self) {
        let Some(next) = self.pos.and_then(|p| self.queue.id(p + 1)) else {
            return;
        };
        let queued = self.appended.is_some() || self.decks.cued.is_some();
        if queued && self.blends_into(next) != self.decks.cued.is_some() {
            self.drop_appended().await;
            if self.current_entry.is_some() {
                self.prefetch();
            }
        }
    }

    /// Whether the change to queue entry `id` (the song after the current
    /// one) blends rather than being gapless.
    pub(super) fn blends_into(&self, id: u64) -> bool {
        self.decks.mixes.on
            && self.state.repeat != Repeat::One
            && !self.sleeping_at_song_end()
            && (self.decks.radio || self.decks.autoplay.contains(&id))
    }

    /// Cues the next song paused and silent on a second deck.
    pub(super) async fn cue(&mut self, id: u64, video_id: &str, stream: &Stream) {
        let deck = match self.decks.spare.take() {
            Some(deck) => deck,
            None => match Player::spawn(0.0, self.player_tx.clone()).await {
                Ok(deck) => {
                    deck.set_equalizer(&self.deck_equalizer());
                    deck
                }
                Err(error) => {
                    log::warn!("couldn't start a second audio player: {error:#}");
                    return;
                }
            },
        };
        deck.set_pause(true);
        deck.set_volume(0.0);
        deck.set_loop(false);
        let (options, gain) = self.file_options(video_id, stream, None);
        match deck.load(&stream.url, LoadMode::Replace, &options).await {
            Ok(entry) => {
                self.decks.cued = Some(Cued {
                    deck,
                    next: Appended {
                        id,
                        itag: stream.itag,
                        entry,
                        gain,
                    },
                });
                self.emit(true);
            }
            Err(error) => {
                log::warn!("couldn't cue the next song: {error:#}");
                self.decks.spare = Some(deck);
            }
        }
    }

    /// Stops the cued song; its deck is kept as the spare.
    pub(super) async fn drop_cued(&mut self) {
        if let Some(cued) = self.decks.cued.take() {
            cued.deck.stop();
            self.decks.spare = Some(cued.deck);
        }
    }

    /// The main deck reported its position: start the blend when the
    /// current song is close enough to its end. True when it started.
    pub(super) async fn deck_position(&mut self) -> bool {
        self.decks.position_at = Instant::now();
        let Some(id) = self.decks.cued.as_ref().map(|c| c.next.id) else {
            return false;
        };
        if self.decks.tail.is_some()
            || self.paused
            || self.state.duration <= 0.0
            || !self.blends_into(id)
        {
            return false;
        }
        let length = f64::from(self.decks.mixes.seconds).min(self.state.duration / 3.0);
        let left = self.state.duration - self.state.position;
        // Closer to the end than that, the end of the file changes songs at once.
        if left > length || left <= 0.25 {
            return false;
        }
        self.swap(left).await;
        true
    }

    /// The cued song becomes the current one on its own deck, now. Over
    /// `length` seconds the old one fades out under it; 0 cuts it.
    pub(super) async fn swap(&mut self, length: f64) {
        self.finish_blend().await;
        let Some(cued) = self.decks.cued.take() else {
            return;
        };
        let outgoing = self.main.replace(cued.deck.clone());
        self.current_entry = Some(cued.next.entry);
        self.idle = false;
        self.paused = false;
        self.state.position = 0.0;
        self.decks.position_at = Instant::now();
        if let Some(old) = outgoing {
            if length > 0.0 {
                log::info!("blending into the next song over {length:.1}s");
                self.decks.tail = Some(Tail { deck: old, length });
                self.keep_time();
            } else {
                old.stop();
                if self.decks.mixes.on {
                    self.decks.spare = Some(old);
                }
            }
        }
        self.apply_loop().await;
        self.apply_volumes().await;
        cued.deck.set_pause(false);
        let entry = cued.next.entry;
        self.advanced(cued.next).await;
        // The cued file's length arrived while it waited; ask for it again.
        if self.current_entry == Some(entry)
            && let Some(duration) = cued.deck.duration().await
        {
            self.state.duration = duration;
            self.emit(true);
        }
    }

    /// Ends a blend at once: the old song stops, the new one plays at full
    /// volume, and the song after it is queued.
    pub(super) async fn finish_blend(&mut self) {
        if self.stop_tail().await {
            self.blend_over().await;
        }
    }

    /// Ends a blend because the current song is changing or failing: the
    /// old song stops and the current deck is at full volume at once, so the
    /// blend's progress (the current song's position) can't bring the old
    /// one back. Whatever changes the song queues the one after it.
    pub(super) async fn stop_tail(&mut self) -> bool {
        let Some(tail) = self.decks.tail.take() else {
            return false;
        };
        tail.deck.stop();
        if self.decks.mixes.on {
            self.decks.spare = Some(tail.deck);
        }
        self.apply_volumes().await;
        true
    }

    /// After a blend: the new song at full volume, and the song after it
    /// queued (its resolve was set aside while both decks were busy).
    async fn blend_over(&mut self) {
        self.apply_volumes().await;
        if self.current_entry.is_some() {
            self.prefetch();
        }
    }
}
