//! M15: the system media controls outside Linux, where MPRIS does this job:
//! the Windows media overlay, lock screen and media keys (System Media
//! Transport Controls), and macOS's Now Playing in Control Center and the
//! media keys (MPNowPlayingInfoCenter, MPRemoteCommandCenter), both through
//! the `souvlaki` crate.
//!
//! They follow the backend's playing state ([`Now`]), like MPRIS, and drive
//! the app through the same [`Remote`]. Windows ties the controls to a
//! window (HWND), so they are made when the main window opens and dropped
//! when it closes (outside Linux that quits the app). Updates run on GPUI's
//! foreground, which is the main thread macOS wants; the button handlers
//! only hand commands to the backend, from whichever thread the system calls
//! them on.

use std::ffi::c_void;
use std::time::{Duration, Instant};

use gpui_kit::*;
use souvlaki::{
    MediaControlEvent, MediaControls, MediaMetadata, MediaPlayback, MediaPosition, PlatformConfig,
    SeekDirection,
};
use tokio::sync::watch;
use ytfast::backend::Command;
use ytfast::desktop::{Now, Remote, Request};
use ytfast::model::Track;

/// How far the overlay's fast-forward and rewind buttons jump, in seconds.
const SEEK_STEP: f64 = 10.0;
/// A position this far from where playback should have got to is a seek.
const SEEK_THRESHOLD: f64 = 1.5;
/// How often the position is republished while playing: Windows doesn't
/// advance the overlay's timeline by itself.
const POSITION_EVERY: Duration = Duration::from_secs(5);

/// The media controls while the main window is open; dropping it takes them
/// off the system.
pub struct Media {
    _follow: Task<()>,
}

/// Puts the app in the system's media controls, for `window`.
pub fn attach(
    window: &Window,
    remote: &Remote,
    now: watch::Receiver<Now>,
    cx: &mut App,
) -> Option<Media> {
    let hwnd = hwnd(window);
    if cfg!(target_os = "windows") && hwnd.is_none() {
        log::warn!("no window handle, so no system media controls");
        return None;
    }
    let config = PlatformConfig {
        display_name: "Music",
        dbus_name: "ytfast",
        hwnd,
    };
    let mut controls = MediaControls::new(config)
        .inspect_err(|e| log::warn!("no system media controls: {e:?}"))
        .ok()?;
    let events = remote.clone();
    controls
        .attach(move |event| handle(&events, event))
        .inspect_err(|e| log::warn!("no system media controls: {e:?}"))
        .ok()?;
    log::info!("system media controls attached");
    let follow = cx.spawn(async move |_| follow(controls, now).await);
    Some(Media { _follow: follow })
}

#[cfg(target_os = "windows")]
fn hwnd(window: &Window) -> Option<*mut c_void> {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    // GPUI's own `Window::window_handle` is a different one.
    match HasWindowHandle::window_handle(window).ok()?.as_raw() {
        RawWindowHandle::Win32(handle) => Some(handle.hwnd.get() as *mut c_void),
        _ => None,
    }
}

/// macOS's controls belong to the process, not a window.
#[cfg(not(target_os = "windows"))]
fn hwnd(_window: &Window) -> Option<*mut c_void> {
    None
}

/// Carries out a button from the overlay, Control Center or a media key.
fn handle(remote: &Remote, event: MediaControlEvent) {
    match event {
        MediaControlEvent::Play => remote.play(),
        MediaControlEvent::Pause | MediaControlEvent::Stop => remote.pause(),
        MediaControlEvent::Toggle => remote.toggle(),
        MediaControlEvent::Next => remote.command(Command::Next),
        MediaControlEvent::Previous => remote.command(Command::Previous),
        MediaControlEvent::Seek(direction) => seek_by(remote, signed(direction, SEEK_STEP)),
        MediaControlEvent::SeekBy(direction, by) => {
            seek_by(remote, signed(direction, by.as_secs_f64()))
        }
        MediaControlEvent::SetPosition(MediaPosition(at)) => seek_to(remote, at.as_secs_f64()),
        MediaControlEvent::SetVolume(volume) if volume.is_finite() => {
            remote.command(Command::Volume(volume.clamp(0.0, 1.0) * 100.0))
        }
        MediaControlEvent::SetVolume(_) => {}
        MediaControlEvent::OpenUri(link) => remote.request(Request::Open(link)),
        MediaControlEvent::Raise => remote.request(Request::Show),
        MediaControlEvent::Quit => remote.request(Request::Quit),
    }
}

fn signed(direction: SeekDirection, seconds: f64) -> f64 {
    match direction {
        SeekDirection::Forward => seconds,
        SeekDirection::Backward => -seconds,
    }
}

/// Seeks by `offset` seconds; past the end is the next song, as in MPRIS.
fn seek_by(remote: &Remote, offset: f64) {
    let (position, duration) = {
        let now = remote.now();
        (now.playback.position, Shown::of(&now).duration)
    };
    if duration <= 0.0 {
        return;
    }
    let target = position + offset;
    if target >= duration {
        remote.command(Command::Next);
    } else {
        remote.command(Command::Seek(target.max(0.0)));
    }
}

fn seek_to(remote: &Remote, seconds: f64) {
    let duration = Shown::of(&remote.now()).duration;
    if duration > 0.0 && (0.0..=duration).contains(&seconds) {
        remote.command(Command::Seek(seconds));
    }
}

/// Mirrors the playing state into the controls until the backend ends or
/// the window closes.
async fn follow(mut controls: MediaControls, mut now: watch::Receiver<Now>) {
    let mut shown = Shown::default();
    let mut at = Instant::now();
    let mut published = Instant::now();
    loop {
        let next = Shown::of(&now.borrow_and_update());
        let expected = shown.position_after(at.elapsed());
        let new_song = next.video_id() != shown.video_id() || next.length() != shown.length();
        let seeked = (next.position - expected).abs() > SEEK_THRESHOLD;
        let stale = next.playing && published.elapsed() >= POSITION_EVERY;
        if new_song {
            set_metadata(&mut controls, &next);
        }
        if new_song || seeked || stale || next.playing != shown.playing {
            let _ = controls
                .set_playback(next.playback())
                .inspect_err(|e| log::debug!("media controls: playback: {e:?}"));
            published = Instant::now();
        }
        shown = next;
        at = Instant::now();
        if now.changed().await.is_err() {
            break;
        }
    }
}

fn set_metadata(controls: &mut MediaControls, shown: &Shown) {
    let Some(track) = &shown.track else {
        return;
    };
    let artist = track.artist_line();
    let metadata = MediaMetadata {
        title: Some(&track.title),
        artist: Some(&artist),
        album: Some(track.album.as_ref().map_or("", |album| album.text.as_str())),
        // YouTube's 544 px cover; the system fetches it.
        cover_url: track.thumbnail.as_deref(),
        duration: (shown.duration > 0.0).then(|| Duration::from_secs_f64(shown.duration)),
    };
    let _ = controls
        .set_metadata(metadata)
        .inspect_err(|e| log::debug!("media controls: metadata: {e:?}"));
}

/// What the controls show.
#[derive(Clone, Debug, Default)]
struct Shown {
    track: Option<Track>,
    playing: bool,
    /// Seconds; the decoder's figure once it knows, else YouTube's.
    duration: f64,
    /// Seconds, as last reported.
    position: f64,
}

impl Shown {
    fn of(now: &Now) -> Self {
        let track = now.track().cloned();
        let playback = &now.playback;
        let duration = if playback.duration > 0.0 {
            playback.duration
        } else {
            track
                .as_ref()
                .and_then(|t| t.duration)
                .map_or(0.0, f64::from)
        };
        Self {
            track,
            playing: playback.playing,
            duration,
            position: playback.position,
        }
    }

    fn video_id(&self) -> Option<&str> {
        self.track.as_ref().map(|t| t.video_id.as_str())
    }

    /// Whole seconds, so the decoder refining the duration doesn't republish.
    fn length(&self) -> i64 {
        self.duration.round() as i64
    }

    /// Where playback should be `elapsed` after the last report.
    fn position_after(&self, elapsed: Duration) -> f64 {
        if !self.playing {
            return self.position;
        }
        let position = self.position + elapsed.as_secs_f64();
        if self.duration > 0.0 {
            position.min(self.duration)
        } else {
            position
        }
    }

    fn playback(&self) -> MediaPlayback {
        let progress = Some(MediaPosition(Duration::from_secs_f64(
            self.position.max(0.0),
        )));
        match (&self.track, self.playing) {
            (None, _) => MediaPlayback::Stopped,
            (Some(_), true) => MediaPlayback::Playing { progress },
            (Some(_), false) => MediaPlayback::Paused { progress },
        }
    }
}
