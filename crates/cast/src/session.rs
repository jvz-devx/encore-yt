//! One remote deck. Owns its connection and relay, never the song queue.
//! The caller serializes controls and polls status, advancing its own queue
//! only after an explicit finished status.

use std::time::Duration;

use anyhow::{Context, Result, ensure};

use crate::castv2::{self, App, Client, Media, MediaStatus};
use crate::discovery::Policy;
use crate::relay::{Relay, Source};
use crate::{Device, dlna, local_ip_for};

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Status {
    pub playing: bool,
    pub loading: bool,
    pub position: f64,
    pub duration: Option<f64>,
    pub finished: bool,
}

enum Receiver {
    Cast {
        client: Client,
        app: App,
        media: Option<i64>,
    },
    Dlna(dlna::Renderer),
}

pub enum Connection {
    Ready(Box<Session>),
    /// No LAUNCH or control has been sent. The UI must ask before retrying.
    Busy(String),
}

pub struct Session {
    receiver: Receiver,
    relay: Relay,
    status: Status,
    // DLNA STOPPED before the first PLAY is not a song ending.
    played: bool,
    sink_formats: Vec<String>,
}

impl Session {
    pub async fn connect(device: &Device, policy: Policy, takeover: bool) -> Result<Connection> {
        policy.check(device)?;
        let relay = Relay::start(local_ip_for(device.ip())?).await?;
        let (receiver, sink_formats) = match device {
            Device::Cast(device) => {
                let client = Client::connect(device.addr).await?;
                let status = client.receiver_status().await?;
                if !takeover && let Some(app) = status.busy_app() {
                    return Ok(Connection::Busy(app.display_name.clone()));
                }
                let app = client.launch(castv2::DEFAULT_MEDIA_RECEIVER).await?;
                (
                    Receiver::Cast {
                        client,
                        app,
                        media: None,
                    },
                    Vec::new(),
                )
            }
            Device::Dlna(renderer) => {
                let formats = if renderer.connection_manager.is_some() {
                    renderer.sink_formats().await?
                } else {
                    Vec::new()
                };
                (Receiver::Dlna(renderer.clone()), formats)
            }
        };
        Ok(Connection::Ready(Box::new(Self {
            receiver,
            relay,
            status: Status::default(),
            played: false,
            sink_formats,
        })))
    }

    pub fn supports(&self, mime: &str) -> bool {
        self.sink_formats.is_empty()
            || self.sink_formats.iter().any(|entry| {
                let format = entry.split(':').nth(2).unwrap_or_default();
                format == "*" || format.eq_ignore_ascii_case(mime.split(';').next().unwrap_or(mime))
            })
    }

    /// Each new song replaces the published source. Signed stream URLs stay
    /// inside the relay, never in a device command or metadata.
    pub async fn load(
        &mut self,
        source: Source,
        metadata: &Media,
        at: f64,
        playing: bool,
        volume: f64,
    ) -> Result<Status> {
        ensure!(at.is_finite() && at >= 0.0, "invalid start position");
        ensure!(
            self.supports(&metadata.content_type),
            "the device doesn't support this audio format"
        );
        self.played = false;
        self.relay.unpublish_all();
        let mut media = metadata.clone();
        media.url = self.relay.publish(source, &metadata.content_type)?;
        self.status = Status {
            position: at,
            duration: metadata.duration,
            loading: true,
            ..Status::default()
        };
        match &mut self.receiver {
            Receiver::Cast {
                client,
                app,
                media: id,
            } => {
                let status = client.load_at(app, &media, at, playing).await?;
                *id = Some(status.media_session_id);
                client
                    .volume(app, status.media_session_id, volume / 100.0)
                    .await?;
                self.update_cast(status)?;
            }
            Receiver::Dlna(r) => {
                r.set_uri(&dlna::Track {
                    url: media.url,
                    mime: media.content_type,
                    title: media.title,
                    artist: media.artist,
                    album: media.album,
                    art: media.image,
                    duration: media.duration.map(Duration::from_secs_f64),
                })
                .await?;
                if r.rendering_control.is_some() {
                    r.set_volume(volume.clamp(0.0, 100.0).round() as u8).await?;
                }
                r.play().await?;
                if at > 0.0 {
                    r.seek(Duration::from_secs_f64(at)).await?;
                }
                if !playing {
                    r.pause().await?;
                }
                self.status.playing = playing;
                self.status.loading = false;
                self.played = true;
            }
        }
        Ok(self.status.clone())
    }

    fn update_cast(&mut self, status: MediaStatus) -> Result<()> {
        ensure!(
            status.current_time.is_finite() && status.current_time >= 0.0,
            "invalid device position"
        );
        ensure!(
            status.idle_reason.as_deref() != Some("ERROR"),
            "the device couldn't play the song"
        );
        ensure!(
            !matches!(
                status.idle_reason.as_deref(),
                Some("CANCELLED" | "INTERRUPTED")
            ),
            "playback was interrupted on the device"
        );
        self.status.playing = status.player_state == "PLAYING";
        self.status.loading = status.player_state == "BUFFERING"
            || (status.player_state == "IDLE" && status.idle_reason.is_none());
        self.status.position = status.current_time;
        self.status.finished =
            status.player_state == "IDLE" && status.idle_reason.as_deref() == Some("FINISHED");
        Ok(())
    }

    pub async fn poll(&mut self) -> Result<Status> {
        match &mut self.receiver {
            Receiver::Cast { client, app, media } => {
                // Drain old broadcasts before asking for the authoritative current
                // status, so a previous song's FINISHED cannot advance the new one.
                while client.events.try_recv().is_ok() {}
                let receiver = client.receiver_status().await?;
                ensure!(
                    receiver.apps.iter().any(|a| a.session_id == app.session_id),
                    "another app replaced the casting session"
                );
                let id = media.context("no song loaded")?;
                let status = client
                    .media(app, "GET_STATUS", id)
                    .await?
                    .context("the device ended the media session")?;
                ensure!(
                    status.media_session_id == id,
                    "another sender replaced the song"
                );
                self.update_cast(status)?;
            }
            Receiver::Dlna(r) => {
                let state = r.state().await?;
                let (position, duration) = r.position().await?;
                self.status.playing = state == "PLAYING";
                self.status.loading = state == "TRANSITIONING";
                self.played |= self.status.playing;
                // A stopped renderer with no loaded URI is a lost session, not EOF.
                ensure!(
                    state != "NO_MEDIA_PRESENT",
                    "the device ended the media session"
                );
                self.status.finished = self.played && state == "STOPPED";
                if let Some(position) = position {
                    self.status.position = position;
                }
                if let Some(duration) = duration.filter(|n| *n > 0.0) {
                    self.status.duration = Some(duration);
                }
            }
        }
        Ok(self.status.clone())
    }

    pub async fn pause(&mut self, paused: bool) -> Result<()> {
        match &self.receiver {
            Receiver::Cast { client, app, media } => {
                client
                    .media(
                        app,
                        if paused { "PAUSE" } else { "PLAY" },
                        media.context("no song loaded")?,
                    )
                    .await?;
            }
            Receiver::Dlna(r) => {
                if paused {
                    r.pause().await?
                } else {
                    r.play().await?
                }
            }
        }
        self.status.playing = !paused;
        Ok(())
    }

    pub async fn seek(&mut self, seconds: f64) -> Result<()> {
        ensure!(
            seconds.is_finite() && seconds >= 0.0,
            "invalid seek position"
        );
        match &self.receiver {
            Receiver::Cast { client, app, media } => {
                client
                    .seek(app, media.context("no song loaded")?, seconds)
                    .await?;
            }
            Receiver::Dlna(r) => r.seek(Duration::from_secs_f64(seconds)).await?,
        }
        self.status.position = seconds;
        Ok(())
    }

    pub async fn volume(&self, volume: f64) -> Result<()> {
        match &self.receiver {
            Receiver::Cast { client, app, media } => {
                client
                    .volume(app, media.context("no song loaded")?, volume / 100.0)
                    .await?
            }
            Receiver::Dlna(r) => r.set_volume(volume.clamp(0.0, 100.0).round() as u8).await?,
        }
        Ok(())
    }

    /// Called only for our own session. A lost/replaced session is dropped
    /// without STOP, so it never interrupts another sender's playback.
    pub async fn stop(&self) -> Result<()> {
        match &self.receiver {
            Receiver::Cast { client, app, .. } => {
                let receiver = client.receiver_status().await?;
                ensure!(
                    receiver.apps.iter().any(|a| a.session_id == app.session_id),
                    "another app replaced the casting session"
                );
                client.stop_app(app).await
            }
            Receiver::Dlna(r) => r.stop().await,
        }
    }
}
