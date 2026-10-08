//! The JSON payloads a sender sends and the statuses it reads, on the four
//! namespaces a media sender needs.

use serde_json::{Value, json};

pub const NS_CONNECTION: &str = "urn:x-cast:com.google.cast.tp.connection";
pub const NS_HEARTBEAT: &str = "urn:x-cast:com.google.cast.tp.heartbeat";
pub const NS_RECEIVER: &str = "urn:x-cast:com.google.cast.receiver";
pub const NS_MEDIA: &str = "urn:x-cast:com.google.cast.media";

/// Google's Default Media Receiver: plays a URL it is given, no
/// registration needed.
pub const DEFAULT_MEDIA_RECEIVER: &str = "CC1AD845";
/// The device's platform endpoint.
pub const PLATFORM: &str = "receiver-0";

/// What to play: the relay URL and what the app knows about the song.
#[derive(Clone, Debug, Default)]
pub struct Media {
    pub url: String,
    pub content_type: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub image: Option<String>,
    pub duration: Option<f64>,
}

pub fn connect() -> Value {
    json!({ "type": "CONNECT", "userAgent": "encore-yt" })
}

pub fn ping() -> Value {
    json!({ "type": "PING" })
}

pub fn pong() -> Value {
    json!({ "type": "PONG" })
}

pub fn get_status(request_id: u64) -> Value {
    json!({ "type": "GET_STATUS", "requestId": request_id })
}

pub fn launch(request_id: u64, app_id: &str) -> Value {
    json!({ "type": "LAUNCH", "requestId": request_id, "appId": app_id })
}

pub fn stop(request_id: u64, session_id: &str) -> Value {
    json!({ "type": "STOP", "requestId": request_id, "sessionId": session_id })
}

/// LOAD for the Default Media Receiver: a buffered (seekable) stream with
/// music metadata (`metadataType` 3 is MusicTrackMediaMetadata).
pub fn load(request_id: u64, session_id: &str, media: &Media) -> Value {
    load_at(request_id, session_id, media, 0.0, true)
}

pub fn load_at(request_id: u64, session_id: &str, media: &Media, at: f64, playing: bool) -> Value {
    let mut metadata = json!({
        "metadataType": 3,
        "title": media.title,
        "artist": media.artist,
        "albumName": media.album,
    });
    if let Some(image) = &media.image {
        metadata["images"] = json!([{ "url": image }]);
    }
    let mut info = json!({
        "contentId": media.url,
        "contentUrl": media.url,
        "contentType": media.content_type,
        "streamType": "BUFFERED",
        "metadata": metadata,
    });
    if let Some(duration) = media.duration {
        info["duration"] = json!(duration);
    }
    json!({
        "type": "LOAD",
        "requestId": request_id,
        "sessionId": session_id,
        "media": info,
        "autoplay": playing,
        "currentTime": at,
    })
}

/// PLAY, PAUSE, STOP or GET_STATUS for one media session.
pub fn media_command(request_id: u64, kind: &str, media_session_id: i64) -> Value {
    json!({ "type": kind, "requestId": request_id, "mediaSessionId": media_session_id })
}

pub fn seek(request_id: u64, media_session_id: i64, seconds: f64) -> Value {
    json!({
        "type": "SEEK",
        "requestId": request_id,
        "mediaSessionId": media_session_id,
        "currentTime": seconds,
    })
}

/// The stream's own volume (0–1), not the device's.
pub fn media_volume(request_id: u64, media_session_id: i64, level: f64) -> Value {
    json!({
        "type": "SET_VOLUME",
        "requestId": request_id,
        "mediaSessionId": media_session_id,
        "volume": { "level": level },
    })
}

/// An app running on the device, from RECEIVER_STATUS.
#[derive(Clone, Debug, PartialEq)]
pub struct App {
    pub app_id: String,
    pub display_name: String,
    pub session_id: String,
    pub transport_id: String,
    /// True for the idle screen (backdrop) apps a sender may replace.
    pub is_idle_screen: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ReceiverStatus {
    pub apps: Vec<App>,
    pub volume: Option<f64>,
    pub muted: bool,
}

impl ReceiverStatus {
    pub fn parse(payload: &Value) -> ReceiverStatus {
        let status = &payload["status"];
        let apps = status["applications"]
            .as_array()
            .map(|apps| {
                apps.iter()
                    .map(|a| App {
                        app_id: str_of(&a["appId"]),
                        display_name: str_of(&a["displayName"]),
                        session_id: str_of(&a["sessionId"]),
                        transport_id: str_of(&a["transportId"]),
                        is_idle_screen: a["isIdleScreen"].as_bool().unwrap_or(false),
                    })
                    .collect()
            })
            .unwrap_or_default();
        ReceiverStatus {
            apps,
            volume: status["volume"]["level"].as_f64(),
            muted: status["volume"]["muted"].as_bool().unwrap_or(false),
        }
    }

    /// The app showing something other than the idle screen, if any: what
    /// casting would interrupt.
    pub fn busy_app(&self) -> Option<&App> {
        self.apps.iter().find(|a| !a.is_idle_screen)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct MediaStatus {
    pub media_session_id: i64,
    /// IDLE, BUFFERING, PLAYING or PAUSED.
    pub player_state: String,
    /// FINISHED, CANCELLED, INTERRUPTED or ERROR when IDLE.
    pub idle_reason: Option<String>,
    pub current_time: f64,
    pub volume: Option<f64>,
}

impl MediaStatus {
    /// The first entry of a MEDIA_STATUS, or None when nothing is loaded.
    pub fn parse(payload: &Value) -> Option<MediaStatus> {
        let s = payload["status"].as_array()?.first()?;
        Some(MediaStatus {
            media_session_id: s["mediaSessionId"].as_i64()?,
            player_state: str_of(&s["playerState"]),
            idle_reason: s["idleReason"].as_str().map(str::to_owned),
            current_time: s["currentTime"].as_f64().unwrap_or(0.0),
            volume: s["volume"]["level"].as_f64(),
        })
    }
}

fn str_of(value: &Value) -> String {
    value.as_str().unwrap_or_default().to_owned()
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "test assertions report fixture failures"
)]
mod tests {
    use super::*;

    #[test]
    fn load_carries_the_url_type_and_music_metadata() {
        let media = Media {
            url: "http://192.168.1.2:4000/s/t.webm".into(),
            content_type: "audio/webm".into(),
            title: "Song".into(),
            artist: "Artist".into(),
            image: Some("https://i.ytimg.com/x.jpg".into()),
            duration: Some(180.0),
            ..Media::default()
        };
        let load = load(7, "sess", &media);
        assert_eq!(load["type"], "LOAD");
        assert_eq!(load["requestId"], 7);
        assert_eq!(load["sessionId"], "sess");
        assert_eq!(load["media"]["contentId"], media.url);
        assert_eq!(load["media"]["streamType"], "BUFFERED");
        assert_eq!(load["media"]["metadata"]["metadataType"], 3);
        assert_eq!(
            load["media"]["metadata"]["images"][0]["url"],
            "https://i.ytimg.com/x.jpg"
        );
        assert_eq!(load["media"]["duration"], 180.0);
    }

    #[test]
    fn statuses_parse() {
        let receiver = json!({"type":"RECEIVER_STATUS","requestId":1,"status":{
            "applications":[{"appId":"E8C28D3C","displayName":"Backdrop","isIdleScreen":true,
              "sessionId":"s1","transportId":"t1"}],
            "volume":{"level":0.25,"muted":false}}});
        let status = ReceiverStatus::parse(&receiver);
        assert_eq!(status.apps.len(), 1);
        assert_eq!(status.volume, Some(0.25));
        assert!(status.busy_app().is_none());

        let media = json!({"type":"MEDIA_STATUS","status":[{"mediaSessionId":3,
            "playerState":"IDLE","idleReason":"ERROR","currentTime":1.5}]});
        let m = MediaStatus::parse(&media).unwrap();
        assert_eq!((m.media_session_id, m.player_state.as_str()), (3, "IDLE"));
        assert_eq!(m.idle_reason.as_deref(), Some("ERROR"));
        assert!(MediaStatus::parse(&json!({"status":[]})).is_none());
    }
}
