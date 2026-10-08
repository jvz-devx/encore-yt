//! Song-change notifications (Settings, off by default) through
//! `org.freedesktop.Notifications`: the title, the artists and the cached
//! cover. Each replaces the previous one. Only real song changes count
//! (not pause, resume or queue edits), and none appear while a Encore window
//! has the focus: the song is on screen already.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use tokio::sync::watch;
use zbus::zvariant::Value;

use crate::desktop::{Flags, Now};
use crate::paths::Paths;

pub(crate) async fn run(
    connection: zbus::Connection,
    mut now: watch::Receiver<Now>,
    flags: Arc<Flags>,
    paths: Paths,
    http: reqwest::Client,
) {
    // The song last seen playing, so the same one is never announced twice.
    let mut last: Option<String> = None;
    let mut replaces: u32 = 0;
    while now.changed().await.is_ok() {
        let song = {
            let now = now.borrow_and_update();
            match now.track() {
                Some(track) if now.playback.playing => (
                    track.video_id.clone(),
                    track.title.clone(),
                    track.artist_line(),
                    track.thumbnail.clone(),
                ),
                _ => continue,
            }
        };
        let (video_id, title, artists, cover) = song;
        if last.as_deref() == Some(video_id.as_str()) {
            continue;
        }
        last = Some(video_id);
        if !flags.notifications.load(Ordering::Relaxed) || flags.focused.load(Ordering::Relaxed) {
            continue;
        }
        let image = match cover {
            Some(url) => tokio::time::timeout(
                Duration::from_secs(5),
                crate::desktop::cached_cover(&http, &paths, &url),
            )
            .await
            .ok()
            .flatten(),
            None => None,
        };
        let mut hints: HashMap<&str, Value<'_>> = HashMap::new();
        hints.insert("desktop-entry", Value::from(crate::app_id()));
        hints.insert("transient", Value::from(true));
        if let Some(file) = &image {
            hints.insert(
                "image-path",
                Value::from(format!("file://{}", file.display())),
            );
        }
        let reply = connection
            .call_method(
                Some("org.freedesktop.Notifications"),
                "/org/freedesktop/Notifications",
                Some("org.freedesktop.Notifications"),
                "Notify",
                &(
                    crate::APP_NAME,
                    replaces,
                    "audio-x-generic",
                    title.as_str(),
                    artists.as_str(),
                    Vec::<&str>::new(),
                    hints,
                    -1i32,
                ),
            )
            .await;
        match reply.and_then(|message| message.body().deserialize::<u32>()) {
            Ok(id) => {
                replaces = id;
                log::info!("notified song change (notification {id})");
            }
            Err(error) => log::warn!("couldn't show a song notification: {error}"),
        }
    }
}
