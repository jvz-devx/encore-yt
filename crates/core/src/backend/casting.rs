//! Queue policy and local/remote handoff. Remote owns all network I/O on
//! its own task, so a slow receiver cannot block backend commands.

mod remote;
pub(super) use remote::{Message, Remote, Request};

use std::time::Duration;

use encore_cast::discovery::Policy;
use encore_cast::relay::Source;
use encore_cast::{Device, Kind};

use super::{Event, Internal, Worker};
use crate::casting::{Session, State};
use crate::innertube::Stream;

pub(super) struct Cast {
    pub state: State,
    devices: Vec<Device>,
    pub remote: Option<Remote>,
    pub scan_open: bool,
    stamp: u64,
    policy: Policy,
    pub play_on_load: bool,
    pub resume_paused: Option<bool>,
    pub aac: bool,
}

impl Cast {
    pub fn new() -> Self {
        Self {
            state: State::default(),
            devices: Vec::new(),
            remote: None,
            scan_open: false,
            stamp: 0,
            policy: Policy::from_env(),
            play_on_load: true,
            resume_paused: None,
            aac: false,
        }
    }
}

impl Worker {
    pub(super) fn casting(&self) -> bool {
        self.cast.remote.is_some()
    }

    fn emit_cast(&self) {
        self.sink.send(Event::Cast(self.cast.state.clone()));
    }

    pub(super) fn scan_cast(&mut self) {
        if self.cast.state.scanning {
            return;
        }
        self.cast.state.scanning = true;
        self.cast.state.error = None;
        self.emit_cast();
        let tx = self.internal_tx.clone();
        let policy = self.cast.policy;
        tokio::spawn(async move {
            let result = encore_cast::discovery::scan(policy, Duration::from_secs(3)).await;
            let _ = tx.send(Internal::CastDevices { result });
        });
    }

    pub(super) fn cast_devices(&mut self, result: anyhow::Result<Vec<Device>>) {
        self.cast.state.scanning = false;
        match result {
            Ok(devices) => {
                self.cast.state.devices =
                    devices.iter().map(crate::casting::Device::from).collect();
                self.cast.devices = devices;
            }
            Err(error) => {
                log::warn!("cast discovery: {error:#}");
                self.cast.state.error = Some(
                    "Couldn't find devices. Check your network and firewall, then try again."
                        .into(),
                );
            }
        }
        self.emit_cast();
    }

    pub(super) async fn connect_cast(&mut self, id: &str, kind: Kind, takeover: bool) {
        log::info!("cast: connection request for {kind:?} device {id}");
        if self.cast.remote.is_some() {
            self.emit_cast();
            return;
        }
        if self.current().is_none() {
            self.cast.state.error = Some("Choose a song to start casting".into());
            self.emit_cast();
            return;
        }
        let Some(device) = self
            .cast
            .devices
            .iter()
            .find(|d| d.id() == id && d.kind() == kind)
            .cloned()
        else {
            self.cast.state.session = None;
            self.cast.state.error =
                Some("The device is no longer available. Find devices again.".into());
            self.emit_cast();
            return;
        };
        if let Err(error) = self.cast.policy.check(&device) {
            log::warn!("cast device refused: {error:#}");
            self.cast.state.session = None;
            self.cast.state.error = Some("This device isn't allowed in this check.".into());
            self.emit_cast();
            return;
        }
        // Only a response to the actual confirmation can take over a busy app.
        if takeover
            && !matches!(&self.cast.state.session, Some(Session::Confirm { device, .. }) if device.id == id && device.kind == kind)
        {
            self.cast.state.session = None;
            self.cast.state.error = Some("The device changed. Choose it again.".into());
            self.emit_cast();
            return;
        }
        self.finish_blend().await;
        self.drop_appended().await;
        self.end_audition().await;
        self.cast.stamp += 1;
        self.cast.play_on_load = self.state.playing;
        self.cast.state.error = None;
        self.cast.state.session = Some(Session::Connecting((&device).into()));
        self.cast.remote = Some(Remote::start(
            device,
            self.cast.policy,
            takeover,
            self.cast.stamp,
            self.internal_tx.clone(),
        ));
        self.emit_cast();
    }

    pub(super) fn disconnect_cast(&mut self) {
        if let Some(remote) = &self.cast.remote {
            if let Some(session) = &self.cast.state.session {
                self.cast.state.session = Some(Session::Disconnecting(session.device().clone()));
            }
            remote.send(Request::Stop);
        } else {
            self.cast.state.session = None;
        }
        self.emit_cast();
    }

    pub(super) fn cast_control(&mut self, request: Request) {
        if let Some(remote) = &self.cast.remote {
            remote.send(request);
        }
    }

    pub(super) fn load_cast(&mut self, stream: Stream) {
        let Some(track) = self.current() else {
            return;
        };
        let mime = stream_mime(&stream);
        let source = if stream.url.starts_with("https://") || stream.url.starts_with("http://") {
            Source::Remote(stream.url.clone())
        } else {
            Source::File(stream.url.clone().into())
        };
        let metadata = encore_cast::castv2::Media {
            content_type: mime.into(),
            title: track.title.clone(),
            artist: track
                .artists
                .iter()
                .map(|a| a.text.as_str())
                .collect::<Vec<_>>()
                .join(", "),
            album: track
                .album
                .as_ref()
                .map(|a| a.text.clone())
                .unwrap_or_default(),
            image: track.thumbnail.clone(),
            duration: (self.state.duration > 0.0).then_some(self.state.duration),
            ..Default::default()
        };
        self.state.format = Some(crate::resolver::describe(stream.itag));
        self.cast_control(Request::Load {
            generation: self.generation,
            source,
            metadata: Box::new(metadata),
            position: self.resume_at.unwrap_or(self.state.position),
            playing: self.cast.play_on_load,
            volume: self.state.volume,
        });
        self.resume_at = None;
        self.emit(true);
    }

    pub(super) async fn cast_message(&mut self, stamp: u64, message: Message) {
        if stamp != self.cast.stamp || self.cast.remote.is_none() {
            return;
        }
        match message {
            Message::Ready { aac } => {
                self.cast.aac = aac;
                // Only now does the local deck stop. Its late events cannot
                // mutate playback while the remote owns the deck.
                if let Some(player) = &self.main
                    && let Some(position) =
                        player.property("time-pos").await.and_then(|v| v.as_f64())
                {
                    self.state.position = position;
                }
                self.current_entry = None;
                if let Some(player) = &self.main {
                    player.stop();
                }
                self.resume_at = Some(self.state.position);
                self.state.loading = true;
                self.emit(true);
                if let Some(track) = self.current() {
                    self.resolve_current(&track.video_id.clone());
                }
            }
            Message::Busy(app) => {
                self.cast.remote = None;
                if let Some(session) = self.cast.state.session.take() {
                    self.cast.state.session = Some(Session::Confirm {
                        device: session.device().clone(),
                        app,
                    });
                }
                self.emit_cast();
            }
            Message::Status { generation, status } => {
                if generation != self.generation {
                    return;
                }
                self.state.position = status.position;
                self.state.playing = status.playing;
                self.state.loading = status.loading;
                if let Some(duration) = status.duration {
                    self.state.duration = duration;
                }
                if !status.loading
                    && let Some(Session::Connecting(device)) = &self.cast.state.session
                {
                    self.cast.state.session = Some(Session::Active(device.clone()));
                    self.emit_cast();
                }
                self.emit(true);
                self.save_session(false);
                if status.playing && status.position >= 10.0 && !self.reported {
                    self.reported = true;
                    self.report_play();
                }
                if status.finished {
                    if self.state.repeat == crate::model::Repeat::One
                        && !self.sleeping_at_song_end()
                    {
                        if let Some(pos) = self.pos {
                            self.start(pos).await;
                        }
                    } else {
                        self.next(true).await;
                    }
                }
            }
            Message::Ended {
                generation,
                status,
                error,
                loaded,
            } => {
                self.cast.remote = None;
                self.cast.state.session = None;
                self.cast.state.error = error;
                self.emit_cast();
                if loaded {
                    let current = generation == self.generation;
                    let position = if current {
                        status.position
                    } else {
                        self.state.position
                    };
                    let paused = if current {
                        !status.playing && !status.loading
                    } else {
                        !self.cast.play_on_load
                    };
                    self.cast.resume_paused = Some(paused);
                    if let Some(pos) = self.pos {
                        self.start_at(pos, Some(position)).await;
                    }
                } else {
                    // Connection/LOAD failed after the local deck stopped.
                    if self.current_entry.is_none()
                        && let Some(pos) = self.pos
                    {
                        self.cast.resume_paused = Some(!self.cast.play_on_load);
                        self.start_at(pos, Some(self.state.position)).await;
                    }
                }
            }
        }
    }
}

fn stream_mime(stream: &Stream) -> &'static str {
    // Fake streams may be any local fixture, independently of their itag.
    if !stream.url.starts_with("http") {
        return match std::path::Path::new(&stream.url)
            .extension()
            .and_then(|e| e.to_str())
        {
            Some("mp3") => "audio/mpeg",
            Some("wav") => "audio/wav",
            Some("ogg" | "opus") => "audio/ogg",
            Some("m4a" | "mp4") => "audio/mp4",
            Some("flac") => "audio/flac",
            _ => "audio/webm",
        };
    }
    match stream.itag {
        140 | 141 => "audio/mp4",
        _ => "audio/webm",
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, reason = "test fixtures")]
mod tests {
    use super::*;
    use crate::backend::{Sink, queue};
    use crate::model::{Playback, Repeat, Track};
    use crate::resolver::Resolver;
    use encore_cast::session::Status;
    use std::sync::Arc;

    fn worker() -> (Worker, tokio::sync::mpsc::UnboundedReceiver<Request>) {
        static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "core-cast-{}-{}",
            std::process::id(),
            N.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        let paths = crate::paths::Paths {
            config: root.join("config"),
            cache: root.join("cache"),
            runtime: root.join("runtime"),
        };
        let (tx, _) = std::sync::mpsc::channel();
        let (now, _) = tokio::sync::watch::channel(crate::desktop::Now::default());
        let sink = Sink {
            tx,
            now: Arc::new(now),
            wake: Arc::new(|| {}),
        };
        let mut worker = Worker::new(
            Arc::new(crate::innertube::Client::new().unwrap()),
            Arc::new(Resolver::new(paths.runtime.clone())),
            paths,
            sink,
        );
        let tracks = (0..3)
            .map(|n| Track {
                video_id: format!("test-{n}"),
                title: format!("Song {n}"),
                duration: Some(180),
                artists: Vec::new(),
                album: None,
                thumbnail: None,
                like: None,
                set_video_id: None,
            })
            .collect();
        worker.queue = queue::Queue::default();
        worker.queue.replace(tracks);
        worker.pos = Some(0);
        worker.state = Playback {
            index: Some(0),
            playing: true,
            duration: 180.0,
            autoplay: false,
            ..Default::default()
        };
        let (remote, rx) = Remote::test_link();
        worker.cast.remote = Some(remote);
        worker.cast.stamp = 4;
        worker.cast.state.session = Some(Session::Active(crate::casting::Device {
            id: "local".into(),
            kind: Kind::Cast,
            name: "Local receiver".into(),
            model: "Test".into(),
            group: false,
        }));
        (worker, rx)
    }

    #[tokio::test]
    async fn a_receiver_lost_between_discovery_and_click_reports_an_error() {
        let (mut w, _) = worker();
        w.cast.remote = None;
        w.cast.state.session = None;
        w.connect_cast("gone-receiver", Kind::Cast, false).await;
        assert!(
            w.cast
                .state
                .error
                .as_deref()
                .is_some_and(|s| s.contains("no longer available"))
        );
        assert!(!w.casting());
    }

    #[tokio::test]
    async fn remote_controls_do_not_start_or_modify_a_local_deck() {
        let (mut w, mut rx) = worker();
        w.command(super::super::Command::TogglePause).await;
        w.command(super::super::Command::Seek(35.0)).await;
        w.command(super::super::Command::Volume(28.0)).await;
        assert!(matches!(rx.try_recv().unwrap(), Request::Pause(true)));
        assert!(matches!(rx.try_recv().unwrap(), Request::Seek(35.0)));
        assert!(matches!(rx.try_recv().unwrap(), Request::Volume(28.0)));
        assert!(w.main.is_none());
        w.main_event(crate::player::PlayerEvent::Idle(true)).await;
        assert!(w.state.playing, "late local events cannot pause the remote");
    }

    #[tokio::test]
    async fn remote_status_drives_position_and_old_sessions_are_inert() {
        let (mut w, _) = worker();
        w.cast_message(
            3,
            Message::Status {
                generation: 0,
                status: Status {
                    position: 120.0,
                    ..Default::default()
                },
            },
        )
        .await;
        assert_eq!(w.state.position, 0.0);
        w.cast_message(
            4,
            Message::Status {
                generation: 99,
                status: Status {
                    position: 120.0,
                    ..Default::default()
                },
            },
        )
        .await;
        assert_eq!(w.state.position, 0.0);
        w.cast_message(
            4,
            Message::Status {
                generation: 0,
                status: Status {
                    position: 37.0,
                    playing: true,
                    duration: Some(190.0),
                    ..Default::default()
                },
            },
        )
        .await;
        assert_eq!(w.state.position, 37.0);
        assert_eq!(w.state.duration, 190.0);
        assert!(matches!(w.cast.state.session, Some(Session::Active(_))));
    }

    #[tokio::test]
    async fn finished_song_advances_the_remote_queue_once_and_repeat_one_reloads() {
        let (mut w, mut commands) = worker();
        let end = || Message::Status {
            generation: 0,
            status: Status {
                position: 180.0,
                finished: true,
                ..Default::default()
            },
        };
        w.cast_message(4, end()).await;
        assert_eq!(w.pos, Some(1));
        assert_eq!(w.generation, 1);
        assert!(
            commands.try_recv().is_err(),
            "EOF must not Pause an already stopped renderer"
        );
        w.cast_message(4, end()).await;
        assert_eq!(w.pos, Some(1), "an old EOF cannot skip another song");
        let (mut w, _) = worker();
        w.state.repeat = Repeat::One;
        w.cast_message(4, end()).await;
        assert_eq!(w.pos, Some(0));
        assert_eq!(w.generation, 1);
        assert!(w.casting());
    }

    #[tokio::test]
    async fn disconnect_and_loss_restore_the_same_song_position_and_pause_state() {
        for playing in [true, false] {
            let (mut w, _) = worker();
            w.cast_message(
                4,
                Message::Ended {
                    generation: 0,
                    status: Status {
                        position: 47.0,
                        playing,
                        ..Default::default()
                    },
                    loaded: true,
                    error: (!playing).then(|| "Lost the device".into()),
                },
            )
            .await;
            assert!(!w.casting());
            assert_eq!(w.pos, Some(0));
            assert_eq!(w.resume_at, Some(47.0));
            assert_eq!(w.state.position, 47.0);
            assert_eq!(w.cast.resume_paused, Some(!playing));
        }
    }
}
