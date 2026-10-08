use super::*;

/// Songs failing one after the other before playback stops.
pub(super) const FAILURES_TO_STOP: u32 = 3;

impl super::Worker {
    /// A play request replaces the queue: results of earlier ones no longer apply.
    pub(super) fn new_epoch(&mut self) -> u64 {
        self.epoch += 1;
        self.extending = false;
        self.advance_pending = false;
        self.waiting_for_network = false;
        self.decks.radio = false;
        self.decks.autoplay.clear();
        self.epoch
    }

    /// A new list as the queue, `start` (a list index) current; shuffled,
    /// it plays first and the rest at random.
    pub(super) fn set_queue(&mut self, tracks: Vec<Track>, start: usize) {
        self.queue.replace(tracks);
        self.pos = None;
        if !self.queue.is_empty() {
            let start = start.min(self.queue.len() - 1);
            self.pos = Some(if self.state.shuffle {
                self.queue.shuffle(start)
            } else {
                start
            });
        }
        self.send_queue();
    }

    pub(super) fn send_queue(&mut self) {
        self.sink.send(Event::Queue(self.queue.tracks()));
        self.save_session(true);
    }

    pub(super) fn track_at(&self, pos: usize) -> Option<&Track> {
        self.queue.track(pos)
    }

    /// The song playing, or ready to play.
    pub(super) fn current(&self) -> Option<&Track> {
        self.pos.and_then(|p| self.queue.track(p))
    }

    /// Applies a queue edit. The current song stays current wherever it
    /// moves; if the song after it changed, the one queued in the player behind
    /// it is dropped and the new next one is prepared and queued instead.
    /// An edit never fetches autoplay's radio: songs removed or cleared
    /// stay gone, and autoplay continues when the last song ends
    /// ([`Self::next`]), as in YouTube Music.
    pub(super) async fn edit_queue(
        &mut self,
        edit: impl FnOnce(&mut queue::Queue, Option<usize>, bool),
    ) {
        let current = self.pos.and_then(|p| self.queue.id(p));
        let next = self.pos.and_then(|p| self.queue.id(p + 1));
        edit(&mut self.queue, self.pos, self.state.shuffle);
        self.pos = current.and_then(|id| self.queue.position(id));
        self.state.index = self.pos;
        let new_next = self.pos.and_then(|p| self.queue.id(p + 1));
        if new_next != next {
            self.drop_appended().await;
            self.prefetch();
        }
        self.send_queue();
        self.emit(true);
    }

    /// Play next (`next`) or Add to queue. With nothing to play yet, the
    /// songs become the queue and start.
    pub(super) async fn add(&mut self, tracks: Vec<Track>, next: bool) {
        if tracks.is_empty() {
            return;
        }
        if self.pos.is_none() {
            self.new_epoch();
            self.set_queue(tracks, 0);
            if let Some(pos) = self.pos {
                self.start(pos).await;
            }
            return;
        }
        self.edit_queue(|queue, pos, _| {
            if next {
                queue.play_next(pos, tracks);
            } else {
                queue.add_to_queue(pos, tracks);
            }
        })
        .await;
    }

    /// Starts the track at play-order position `pos`.
    pub(super) async fn start(&mut self, pos: usize) {
        self.start_at(pos, None).await;
    }

    /// Starts the track at `pos`, from `at` seconds in.
    pub(super) async fn start_at(&mut self, pos: usize, at: Option<f64>) {
        let Some(track) = self.track_at(pos).cloned() else {
            return;
        };
        self.generation += 1;
        self.pos = Some(pos);
        self.queue.reached(pos);
        self.current_entry = None;
        self.appended = None;
        // A blend in progress ends, and a song cued on the second deck goes.
        self.finish_blend().await;
        self.drop_cued().await;
        self.retried = false;
        self.reported = false;
        self.waiting_for_network = false;
        self.resume_at = at;
        self.asked = Instant::now();
        self.state.index = Some(pos);
        self.state.loading = true;
        self.state.position = at.unwrap_or(0.0);
        self.state.duration = track.duration.map(f64::from).unwrap_or(0.0);
        self.state.format = None;
        self.state.gain = None;
        self.state.lyrics = None;
        self.state.related = None;
        self.emit(true);
        if let Some(player) = &self.main {
            // Stop the previous song at once; the new one follows when resolved.
            player.stop();
        }
        if self.sleeping_at_song_end() {
            // The timer now waits for this song's end.
            self.restore_fade().await;
        }
        self.resolve_current(&track.video_id);
        self.fetch_watch_info(&track.video_id);
        self.fetch_player(&track.video_id);
        self.maybe_extend();
        self.save_session(true);
    }

    /// Resolves the current song for playback. Its share of the run is
    /// taken before the previous song's resolve and the old prefetch are
    /// stopped, so a song that was next keeps resolving as it becomes current.
    pub(super) fn resolve_current(&mut self, video_id: &str) {
        let generation = self.generation;
        let request = self.resolver.request(video_id);
        let tx = self.internal_tx.clone();
        let task = tokio::spawn(async move {
            let stream = request.wait().await;
            let _ = tx.send(Internal::Started { generation, stream });
        });
        if let Some(old) = self.resolving.replace(task.abort_handle()) {
            old.abort();
        }
        if let Some(old) = self.prefetching.take() {
            old.abort();
        }
    }

    pub(super) fn fetch_watch_info(&self, video_id: &str) {
        let generation = self.generation;
        let client = self.client.clone();
        let tx = self.internal_tx.clone();
        let target = Target::Watch {
            video_id: Some(video_id.to_owned()),
            playlist_id: None,
            params: None,
        };
        tokio::spawn(async move {
            if let Ok(value) = client.next(&target).await {
                let _ = tx.send(Internal::Watch {
                    generation,
                    info: parse::watch_next(&value),
                });
            }
        });
    }

    /// Keeps the queue ahead: the next song resolves for playback (with its
    /// player response, for its loudness) and is appended to the player's playlist
    /// so the change is gapless; the one after it is prepared as a guess.
    pub(super) fn prefetch(&mut self) {
        let Some(pos) = self.pos else { return };
        if let Some(after) = self.track_at(pos + 2) {
            let id = after.video_id.clone();
            self.resolver.prepare(&id);
        }
        // With the sleep timer at the song's end, nothing follows in the player.
        if self.sleeping_at_song_end() {
            return;
        }
        let Some(entry) = self.queue.get(pos + 1) else {
            return;
        };
        if self.appended.as_ref().is_some_and(|a| a.id == entry.id)
            || self
                .decks
                .cued
                .as_ref()
                .is_some_and(|c| c.next.id == entry.id)
        {
            return;
        }
        let (id, video_id) = (entry.id, entry.track.video_id.clone());
        let generation = self.generation;
        let request = self.resolver.request(&video_id);
        let client = (!self.players.contains_key(&video_id)).then(|| self.client.clone());
        let tx = self.internal_tx.clone();
        let task = tokio::spawn(async move {
            let player = async {
                match client {
                    Some(client) => client
                        .player(&video_id)
                        .await
                        .ok()
                        .map(|v| sound::player_info(&v)),
                    None => None,
                }
            };
            let (stream, player) = tokio::join!(request.wait(), player);
            match stream {
                Ok(stream) => {
                    let _ = tx.send(Internal::NextReady {
                        generation,
                        id,
                        video_id,
                        stream,
                        player,
                    });
                }
                Err(error) => log::warn!("resolving the next track failed: {error:#}"),
            }
        });
        if let Some(old) = self.prefetching.replace(task.abort_handle()) {
            old.abort();
        }
    }

    /// The main deck, started on first use.
    async fn ensure_main(&mut self) -> Option<Arc<Player>> {
        if self.main.is_none() {
            match Player::spawn(self.main_volume(), self.player_tx.clone()).await {
                Ok(player) => {
                    self.main = Some(player.clone());
                    self.apply_loop().await;
                    self.apply_equalizer(&player).await;
                }
                Err(error) => {
                    self.sink.send(Event::Error(format!(
                        "Couldn't start the audio player: {error:#}"
                    )));
                    self.state.loading = false;
                    self.emit(true);
                    return None;
                }
            }
        }
        self.main.clone()
    }

    pub(super) async fn next(&mut self, automatic: bool) {
        let Some(pos) = self.pos else { return };
        if self.decks.blending() {
            if !automatic {
                // A manual Next during a blend completes it at once.
                self.finish_blend().await;
                return;
            }
            // The song blended into ended or failed during the blend: the
            // old one stops before the queue moves on.
            self.stop_tail().await;
        }
        if automatic && self.sleeping_at_song_end() {
            self.sleep_after_song().await;
            return;
        }
        if pos + 1 < self.queue.len() {
            if let Some(cued) = &self.decks.cued
                && self.queue.position(cued.next.id) == Some(pos + 1)
            {
                self.swap(0.0).await;
                return;
            }
            if let (Some(appended), Some(player), false) = (&self.appended, &self.main, self.idle)
                && self.queue.position(appended.id) == Some(pos + 1)
            {
                player.skip();
                return;
            }
            self.start(pos + 1).await;
        } else if self.state.repeat == Repeat::All && !self.queue.is_empty() {
            self.start(0).await;
        } else if self.state.autoplay {
            // Continue with radio for the last track; play it as soon as it arrives.
            self.state.loading = true;
            self.emit(true);
            if self.extending {
                self.advance_pending = true;
            } else {
                self.extend(true);
            }
        } else if automatic {
            self.state.playing = false;
            self.state.loading = false;
            self.emit(true);
        }
    }

    pub(super) async fn seek(&mut self, seconds: f64) {
        let seconds = seconds.max(0.0);
        self.finish_blend().await;
        if let (Some(player), Some(_)) = (&self.main, self.current_entry) {
            player.seek(seconds);
            self.state.position = seconds;
            self.emit(true);
            self.save_session(true);
        } else if self.pos.is_some() {
            // Nothing loaded yet (a restored session): Play starts here.
            self.resume_at = Some(seconds);
            self.state.position = seconds;
            self.emit(true);
            self.save_session(true);
        }
    }

    pub(super) async fn drop_appended(&mut self) {
        if self.appended.take().is_some()
            && let Some(player) = &self.main
        {
            player.remove(1);
        }
        self.drop_cued().await;
    }

    /// Autoplay: when the last track in the queue is playing, fetch a radio
    /// to follow it.
    pub(super) fn maybe_extend(&mut self) {
        let Some(pos) = self.pos else { return };
        if self.state.autoplay && !self.extending && pos + 1 >= self.queue.len() {
            self.extend(false);
        }
    }

    /// Fetches YouTube Music's radio for the last track of the queue.
    fn extend(&mut self, then_play: bool) {
        let Some(last) = self.queue.last() else {
            return;
        };
        self.extending = true;
        let target = Target::Watch {
            video_id: Some(last.video_id.clone()),
            playlist_id: Some(format!("RDAMVM{}", last.video_id)),
            params: Some("wAEB".into()),
        };
        let known = self.queue.video_ids();
        let client = self.client.clone();
        let tx = self.internal_tx.clone();
        let epoch = self.epoch;
        tokio::spawn(async move {
            let tracks = match client.next(&target).await {
                Ok(value) => parse::watch_next(&value)
                    .tracks
                    .into_iter()
                    .filter(|t| !known.contains(&t.video_id))
                    .collect(),
                Err(error) => {
                    log::warn!("autoplay radio failed: {error}");
                    Vec::new()
                }
            };
            let _ = tx.send(Internal::Extended {
                epoch,
                tracks,
                then_play,
                autoplay: true,
            });
        });
    }

    pub(super) async fn internal(&mut self, message: Internal) {
        match message {
            Internal::Connected(account) => {
                self.sink.send(Event::Account(account));
                self.prepare_restored();
            }
            Internal::AuthFailed => {
                if self.client.signed_in()
                    && self
                        .last_connect
                        .is_none_or(|t| t.elapsed() > Duration::from_secs(60))
                {
                    self.connect();
                }
            }
            Internal::Started { generation, stream } => {
                if generation != self.generation {
                    return;
                }
                let Some(track) = self.current().cloned() else {
                    return;
                };
                match stream {
                    Ok(stream) => {
                        let Some(player) = self.ensure_main().await else {
                            return;
                        };
                        // `Replace` empties the playlist, including any track queued behind.
                        self.appended = None;
                        let (options, gain) =
                            self.file_options(&track.video_id, &stream, self.resume_at);
                        match player.load(&stream.url, LoadMode::Replace, &options).await {
                            Ok(entry) => {
                                log::info!(
                                    "starting {} {:.1}s after it was asked for",
                                    track.video_id,
                                    self.asked.elapsed().as_secs_f64()
                                );
                                self.resume_at = None;
                                self.current_entry = Some(entry);
                                player.set_pause(false);
                                self.state.format = Some(resolver::describe(stream.itag));
                                self.state.gain = gain;
                                self.emit(true);
                                self.prefetch();
                            }
                            Err(error) => self.fail(&track, &format!("{error:#}")).await,
                        }
                    }
                    Err(error) => self.fail(&track, &format!("{error:#}")).await,
                }
            }
            Internal::NextReady {
                generation,
                id,
                video_id,
                stream,
                player,
            } => {
                if let Some(info) = player {
                    self.remember_player(video_id.clone(), info);
                }
                // The queue may have changed (an edit, shuffle) while it resolved.
                let still_next = self.pos.and_then(|p| self.queue.id(p + 1)) == Some(id);
                if generation != self.generation
                    || !still_next
                    || self.appended.is_some()
                    || self.decks.cued.is_some()
                    || self.current_entry.is_none()
                    || self.sleeping_at_song_end()
                {
                    return;
                }
                if self.blends_into(id) {
                    // Smooth mixes: it waits on the second deck, which is
                    // busy until a blend in progress ends (that queues it again).
                    if !self.decks.blending() {
                        self.cue(id, &video_id, &stream).await;
                    }
                    return;
                }
                let (options, gain) = self.file_options(&video_id, &stream, None);
                if let Some(player) = &self.main
                    && let Ok(entry) = player.load(&stream.url, LoadMode::Append, &options).await
                {
                    self.appended = Some(Appended {
                        id,
                        itag: stream.itag,
                        entry,
                        gain,
                    });
                    self.emit(true);
                }
            }
            Internal::Watch { generation, info } => {
                if generation != self.generation {
                    return;
                }
                self.state.lyrics = info.lyrics;
                self.state.related = info.related;
                if let Some(like) = info.like {
                    self.sink.send(Event::Likes(vec![like]));
                }
                self.emit(true);
            }
            Internal::Queue { epoch, result } => {
                if epoch != self.epoch {
                    return;
                }
                match result {
                    Ok(info) => {
                        let start = info.current;
                        self.set_queue(info.tracks, start);
                        if let Some(pos) = self.pos {
                            self.start(pos).await;
                        }
                    }
                    Err(error) => {
                        self.state.loading = false;
                        self.emit(true);
                        self.sink
                            .send(Event::Error(format!("Couldn't start playback: {error}")));
                    }
                }
            }
            Internal::Extended {
                epoch,
                tracks,
                then_play,
                autoplay,
            } => {
                if epoch != self.epoch {
                    return;
                }
                if autoplay {
                    self.extending = false;
                }
                let play_now = then_play || (autoplay && std::mem::take(&mut self.advance_pending));
                let first_new = self.queue.len();
                let known = self.queue.video_ids();
                self.queue.extend(
                    tracks
                        .into_iter()
                        .filter(|t| !known.contains(&t.video_id))
                        .collect(),
                );
                if autoplay {
                    // Autoplay continues as a radio: its songs blend in.
                    let ids: Vec<u64> = (first_new..self.queue.len())
                        .filter_map(|p| self.queue.id(p))
                        .collect();
                    self.decks.autoplay.extend(ids);
                }
                self.send_queue();
                if play_now && first_new < self.queue.len() {
                    self.start(first_new).await;
                } else if play_now {
                    self.state.loading = false;
                    self.state.playing = false;
                    self.emit(true);
                } else if self.appended.is_none() && self.decks.cued.is_none() {
                    self.prefetch();
                }
            }
            Internal::Failed {
                generation,
                title,
                error,
                online,
            } => {
                if generation != self.generation {
                    return;
                }
                if online && self.failed_in_row + 1 >= FAILURES_TO_STOP {
                    // Nothing plays (YouTube changed something): stop
                    // rather than skip through the whole queue.
                    self.failed_in_row = 0;
                    self.state.loading = false;
                    self.state.playing = false;
                    self.emit(true);
                    self.sink.send(Event::Error(format!(
                        "Couldn't play “{title}” or the songs before it, so playback stopped. \
                         Try again in a few minutes: {error}"
                    )));
                } else if online {
                    self.failed_in_row += 1;
                    self.sink.send(Event::Error(format!(
                        "Couldn't play “{title}”, skipped it. {error}"
                    )));
                    self.next(true).await;
                } else {
                    // Offline: don't skip through the queue; play this song when the connection returns.
                    self.waiting_for_network = true;
                    self.state.loading = true;
                    self.emit(true);
                    self.sink.send(Event::Error(format!(
                        "No connection. “{title}” will play when it's back."
                    )));
                    let client = self.client.clone();
                    let tx = self.internal_tx.clone();
                    tokio::spawn(async move {
                        loop {
                            tokio::time::sleep(Duration::from_secs(5)).await;
                            if client.reachable().await {
                                let _ = tx.send(Internal::Online { generation });
                                return;
                            }
                            if tx.is_closed() {
                                return;
                            }
                        }
                    });
                }
            }
            Internal::Online { generation } => {
                if generation == self.generation
                    && self.waiting_for_network
                    && let Some(pos) = self.pos
                {
                    self.start(pos).await;
                }
            }
            Internal::Player { video_id, info } => self.player_arrived(video_id, info).await,
            Internal::SleepTick { stamp } => self.sleep_tick(stamp).await,
            Internal::EqualizerSettled { stamp } => self.equalizer_settled(stamp).await,
            Internal::Deck(message) => self.deck_message(message).await,
        }
    }

    /// A track failed: retry once with a freshly resolved stream; then skip,
    /// unless YouTube is unreachable, in which case wait for the connection.
    async fn fail(&mut self, track: &Track, error: &str) {
        log::warn!("playback failed: {error}");
        // A song that fails while it blends in takes the old one with it.
        self.stop_tail().await;
        if !self.retried {
            self.retried = true;
            self.resolver.forget(&track.video_id).await;
            self.resolve_current(&track.video_id);
            return;
        }
        let generation = self.generation;
        let client = self.client.clone();
        let tx = self.internal_tx.clone();
        let (title, error) = (track.title.clone(), error.to_owned());
        tokio::spawn(async move {
            let online = client.reachable().await;
            let _ = tx.send(Internal::Failed {
                generation,
                title,
                error,
                online,
            });
        });
    }

    /// Adds the current song to the account's history, with the tracking
    /// URL of the player response fetched when it started if there is one.
    fn report_play(&self) {
        if crate::resolver::fake_stream().is_some() {
            return;
        }
        let Some(track) = self.current() else { return };
        let client = self.client.clone();
        let id = track.video_id.clone();
        let tracking = self.players.get(&id).and_then(|p| p.tracking.clone());
        tokio::spawn(async move {
            let result = match tracking {
                Some(url) => client.ping_playback(&url).await,
                None => client.report_play(&id).await,
            };
            if let Err(error) = result {
                log::warn!("reporting a play failed: {error}");
            }
        });
    }

    /// The song after the current one (`next`, queued gapless in the player or
    /// cued on the second deck) started and is now the current one.
    pub(super) async fn advanced(&mut self, next: Appended) {
        let Some(pos) = self.queue.position(next.id) else {
            // Removed from the queue as it started: move on (boxed: Next can
            // start a song cued on the second deck, which comes back here).
            Box::pin(self.next(true)).await;
            return;
        };
        self.generation += 1;
        self.pos = Some(pos);
        self.queue.reached(pos);
        self.retried = false;
        self.reported = false;
        let track = self.track_at(pos).cloned();
        self.state.index = Some(pos);
        self.state.position = 0.0;
        self.state.duration = track
            .as_ref()
            .and_then(|t| t.duration)
            .map(f64::from)
            .unwrap_or(0.0);
        // When the prefetched file opens at once, its `duration` can arrive
        // before the change of file, where it was taken as the old song's:
        // ask for it again.
        if let Some(player) = &self.main
            && let Some(duration) = player.duration().await
        {
            self.state.duration = duration;
        }
        self.state.format = Some(resolver::describe(next.itag));
        self.state.gain = next.gain;
        self.state.lyrics = None;
        self.state.related = None;
        self.emit(true);
        if let Some(track) = track {
            self.fetch_watch_info(&track.video_id);
            self.fetch_player(&track.video_id);
        }
        self.prefetch();
        self.maybe_extend();
        self.save_session(true);
    }

    pub(super) async fn main_event(&mut self, event: PlayerEvent) {
        match event {
            PlayerEvent::Position(position) => {
                // Before the current file loads, positions belong to the previous one.
                let (Some(position), Some(_)) = (position, self.current_entry) else {
                    return;
                };
                self.state.position = position;
                if self.deck_position().await {
                    // The next song took over on the second deck.
                    return;
                }
                if position >= 1.0 {
                    self.failed_in_row = 0;
                }
                if !self.reported && position >= 10.0 {
                    self.reported = true;
                    self.report_play();
                }
                self.song_end_fade().await;
                self.emit(false);
                self.save_session(false);
            }
            PlayerEvent::Duration(duration) => {
                if let (Some(duration), Some(_)) = (duration, self.current_entry) {
                    self.state.duration = duration;
                    self.emit(true);
                }
            }
            PlayerEvent::Pause(paused) => {
                self.paused = paused;
                self.state.playing = !self.paused && !self.idle && self.pos.is_some();
                self.emit(true);
                if self.paused {
                    self.save_session(true);
                }
            }
            PlayerEvent::Buffering(buffering) => {
                if !self.waiting_for_network {
                    self.state.loading = buffering;
                    self.emit(true);
                }
            }
            PlayerEvent::Idle(idle) => {
                self.idle = idle;
                if !self.idle {
                    self.state.loading = false;
                }
                self.state.playing = !self.paused && !self.idle && self.pos.is_some();
                self.emit(true);
                // Safety net: the player ran out of tracks although one was thought
                // to be queued behind the current one. Move on ourselves.
                if self.idle && !self.state.loading && self.appended.is_some() && self.pos.is_some()
                {
                    log::warn!("the player went idle with a track thought queued; advancing");
                    self.appended = None;
                    self.next(true).await;
                }
            }
            PlayerEvent::PlaylistPos(pos) => {
                // The player moved on to the track appended behind the current one.
                if pos == Some(1)
                    && let Some(appended) = self.appended.take()
                {
                    self.current_entry = Some(appended.entry);
                    if let Some(player) = &self.main {
                        player.remove(0);
                    }
                    // A short song that ends while it blends in: the old
                    // one stops before the position starts again at 0.
                    self.stop_tail().await;
                    self.advanced(appended).await;
                }
            }
            PlayerEvent::EndFile {
                reason,
                error,
                entry,
            } => {
                log::debug!(
                    "end-file {entry} {reason:?} {error:?}; current {:?}, queued {:?}",
                    self.current_entry,
                    self.appended.as_ref().map(|a| a.entry)
                );
                // Events of replaced or queued entries are not about the current track.
                if Some(entry) != self.current_entry {
                    return;
                }
                match reason {
                    EndReason::Eof
                        if self.appended.is_none()
                            && (self.state.repeat != Repeat::One
                                || self.sleeping_at_song_end()) =>
                    {
                        self.next(true).await
                    }
                    EndReason::Error => {
                        // Keep the player from moving on to the queued track: this one is
                        // retried or skipped first.
                        self.drop_appended().await;
                        self.current_entry = None;
                        if let Some(track) = self.current().cloned() {
                            let error = error.unwrap_or_else(|| "the stream failed".into());
                            self.fail(&track, &error).await;
                        }
                    }
                    _ => {}
                }
            }
            PlayerEvent::StartFile { entry } => log::debug!("start-file {entry}"),
        }
    }
}
