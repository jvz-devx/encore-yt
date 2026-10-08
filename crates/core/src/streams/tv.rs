//! The TV client signed in, built as yt-dlp builds its `tv_downgraded`
//! request (2026.08.19; docs/gpui/RESOLVER.md, "M27"): the session's own
//! watch page (read every few hours) first gives the player version, the visitor id
//! and the account's session id, and the `player` request then carries
//! yt-dlp's context and headers, with all three SAPISIDHASH schemes.

use serde_json::{Value, json};
use sha1::Digest;

use crate::innertube::PlayerClient;

pub const WWW: &str = "https://www.youtube.com";

/// What yt-dlp reads from a page's `ytcfg` before it asks as another
/// client.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct WebConfig {
    /// The player version the session is served (`PLAYER_JS_URL`).
    pub player: Option<String>,
    /// Its signature timestamp (`STS`), which yt-dlp sends in preference
    /// to the one in the player script.
    pub sts: Option<u32>,
    pub visitor: Option<String>,
    /// The account's session id (`DATASYNC_ID`), hashed into the
    /// authorization.
    pub user_session: Option<String>,
    /// A brand channel's page id, when the page is one's.
    pub delegated_session: Option<String>,
    pub session_index: Option<u32>,
    /// The cookies the page's response set (the session's SIDCC ones
    /// among them), sent with the TV requests in place of the browser's,
    /// as yt-dlp's cookie jar does.
    pub cookies: SetCookies,
}

/// Cookies a response set: name and value, `None` for one it deleted.
/// The values are secrets, so `Debug` shows only the names.
#[derive(Clone, Default, PartialEq)]
pub struct SetCookies(pub Vec<(String, Option<String>)>);

impl std::fmt::Debug for SetCookies {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_list()
            .entries(self.0.iter().map(|(name, _)| name))
            .finish()
    }
}

impl SetCookies {
    /// Adds a response's `Set-Cookie` headers (`year`: this year, to tell
    /// a deletion, which expires in the past).
    pub fn add<'a>(&mut self, headers: impl IntoIterator<Item = &'a str>, year: u32) {
        for header in headers {
            let Some((name, value)) = parse_set_cookie(header, year) else {
                continue;
            };
            self.0.retain(|(n, _)| *n != name);
            self.0.push((name, value));
        }
    }

    /// The `Cookie` header with these cookies in place of the session's.
    pub fn apply(&self, header: &str) -> String {
        let mut pairs: Vec<(String, String)> = header
            .split("; ")
            .filter_map(|pair| pair.split_once('='))
            .map(|(n, v)| (n.to_owned(), v.to_owned()))
            .collect();
        for (name, value) in &self.0 {
            match (pairs.iter_mut().find(|(n, _)| n == name), value) {
                (Some(pair), Some(value)) => pair.1 = value.clone(),
                (None, Some(value)) => pairs.push((name.clone(), value.clone())),
                (_, None) => pairs.retain(|(n, _)| n != name),
            }
        }
        pairs
            .iter()
            .map(|(n, v)| format!("{n}={v}"))
            .collect::<Vec<_>>()
            .join("; ")
    }
}

/// One `Set-Cookie` header: its name, and its value unless it deletes the
/// cookie (`Max-Age=0`, or an `Expires` before this year).
fn parse_set_cookie(header: &str, year: u32) -> Option<(String, Option<String>)> {
    let mut parts = header.split(';').map(str::trim);
    let (name, value) = parts.next()?.split_once('=')?;
    if name.is_empty() {
        return None;
    }
    let deleted = parts.any(|attr| {
        let (key, val) = attr.split_once('=').unwrap_or((attr, ""));
        match key.to_ascii_lowercase().as_str() {
            "max-age" => val.trim().parse::<i64>().is_ok_and(|age| age <= 0),
            "expires" => val
                .split([' ', '-'])
                .find_map(|word| {
                    (word.len() == 4)
                        .then(|| word.parse::<u32>().ok())
                        .flatten()
                })
                .is_some_and(|y| y < year),
            _ => false,
        }
    });
    Some((name.to_owned(), (!deleted).then(|| value.to_owned())))
}

impl WebConfig {
    /// What was found, for the log: the player and its timestamp, and
    /// which of the session's ids (not their values).
    pub fn summary(&self) -> String {
        let has = |v: bool| if v { "yes" } else { "no" };
        format!(
            "player {}, STS {}, visitor id {}, user session {}, session index {}",
            self.player.as_deref().unwrap_or("none"),
            self.sts.map_or("none".to_owned(), |s| s.to_string()),
            has(self.visitor.is_some()),
            has(self.user_session.is_some()),
            self.session_index
                .map_or("none".to_owned(), |i| i.to_string()),
        )
    }
}

/// The fields of a page's `ytcfg.set({…})` that the TV request needs.
pub fn parse_ytcfg(page: &str) -> WebConfig {
    let text = |key: &str| {
        ytcfg_value(page, key)
            .and_then(|v| v.as_str().map(str::to_owned))
            .filter(|v| !v.is_empty())
    };
    let (delegated, user) = text("DATASYNC_ID")
        .map(|id| split_data_sync_id(&id))
        .unwrap_or_default();
    WebConfig {
        player: text("PLAYER_JS_URL").and_then(|url| crate::streams::player_id(&url)),
        sts: ytcfg_value(page, "STS")
            .and_then(|v| v.as_u64())
            .and_then(|v| u32::try_from(v).ok()),
        visitor: text("VISITOR_DATA"),
        user_session: text("USER_SESSION_ID").or(user),
        delegated_session: text("DELEGATED_SESSION_ID").or(delegated),
        session_index: ytcfg_value(page, "SESSION_INDEX").and_then(|v| match v {
            Value::Number(n) => n.as_u64().and_then(|n| u32::try_from(n).ok()),
            Value::String(s) => s.parse().ok(),
            _ => None,
        }),
        cookies: SetCookies::default(),
    }
}

/// The first `"KEY":` value in the page.
fn ytcfg_value(page: &str, key: &str) -> Option<Value> {
    let needle = format!("\"{key}\":");
    let start = page.find(&needle)? + needle.len();
    serde_json::Deserializer::from_str(&page[start..])
        .into_iter::<Value>()
        .next()?
        .ok()
}

/// `DELEGATED||USER` for a brand channel, `USER||` for the account's own
/// (yt-dlp's `_parse_data_sync_id`).
fn split_data_sync_id(id: &str) -> (Option<String>, Option<String>) {
    let (first, second) = id.split_once("||").unwrap_or((id, ""));
    let some = |s: &str| (!s.is_empty()).then(|| s.to_owned());
    if second.is_empty() {
        (None, some(first))
    } else {
        (some(first), some(second))
    }
}

/// The session's cookies the authorization is computed from.
pub struct Sids {
    pub sapisid: Option<String>,
    pub one_p: Option<String>,
    pub three_p: Option<String>,
}

/// `SAPISIDHASH …[ SAPISID1PHASH …][ SAPISID3PHASH …]`, each
/// `<time>_<sha1>[_u]`, where the hash covers `[user session ]time sid
/// origin` (yt-dlp's `_get_sid_authorization_header`).
pub fn sid_authorization(
    sids: &Sids,
    origin: &str,
    user_session: Option<&str>,
    now: u64,
) -> Option<String> {
    let schemes = [
        ("SAPISIDHASH", &sids.sapisid),
        ("SAPISID1PHASH", &sids.one_p),
        ("SAPISID3PHASH", &sids.three_p),
    ];
    let parts: Vec<String> = schemes
        .iter()
        .filter_map(|(scheme, sid)| {
            let sid = sid.as_deref()?;
            let mut hashed = Vec::new();
            hashed.extend(user_session);
            let now = now.to_string();
            hashed.extend([now.as_str(), sid, origin]);
            let hash = sha1::Sha1::digest(hashed.join(" ").as_bytes());
            let hex: String = hash.iter().map(|b| format!("{b:02x}")).collect();
            let suffix = if user_session.is_some() { "_u" } else { "" };
            Some(format!("{scheme} {now}_{hex}{suffix}"))
        })
        .collect();
    (!parts.is_empty()).then(|| parts.join(" "))
}

/// yt-dlp's `player` body for a client: its context with English and
/// UTC, the player's signature timestamp, and the content checks.
pub fn player_body(client: &PlayerClient, video_id: &str, sts: Option<u32>) -> Value {
    let mut context = json!({
        "clientName": client.name,
        "clientVersion": client.version,
    });
    for (key, value) in client.extra {
        context[*key] = json!(value);
    }
    if let Some(agent) = client.user_agent {
        context["userAgent"] = json!(agent);
    }
    context["hl"] = json!("en");
    context["timeZone"] = json!("UTC");
    context["utcOffsetMinutes"] = json!(0);
    let mut playback = json!({"html5Preference": "HTML5_PREF_WANTS"});
    if let Some(sts) = sts {
        playback["signatureTimestamp"] = json!(sts);
    }
    json!({
        "context": {"client": context},
        "videoId": video_id,
        "playbackContext": {"contentPlaybackContext": playback},
        "contentCheckOk": true,
        "racyCheckOk": true,
    })
}

/// yt-dlp's API headers for a signed-in `player` request
/// (`generate_api_headers`), without the cookies.
pub fn player_headers(
    client: &PlayerClient,
    web: &WebConfig,
    authorization: Option<String>,
) -> Vec<(&'static str, String)> {
    let mut headers = vec![
        ("Content-Type", "application/json".to_owned()),
        ("Accept-Language", "en-us,en;q=0.5".to_owned()),
        ("X-YouTube-Client-Name", client.number.to_string()),
        ("X-YouTube-Client-Version", client.version.to_owned()),
        ("Origin", WWW.to_owned()),
    ];
    if let Some(agent) = client.user_agent {
        headers.push(("User-Agent", agent.to_owned()));
    }
    if let Some(visitor) = &web.visitor {
        headers.push(("X-Goog-Visitor-Id", visitor.clone()));
    }
    if let Some(page) = &web.delegated_session {
        headers.push(("X-Goog-PageId", page.clone()));
    }
    if web.delegated_session.is_some() || web.session_index.is_some() {
        headers.push((
            "X-Goog-AuthUser",
            web.session_index.unwrap_or(0).to_string(),
        ));
    }
    if let Some(authorization) = authorization {
        headers.push(("Authorization", authorization));
        headers.push(("X-Origin", WWW.to_owned()));
    }
    headers
}

/// The page the session's `ytcfg` is read from: a song's watch page, as
/// yt-dlp reads it.
pub fn page_url(video_id: &str) -> String {
    format!("{WWW}/watch?v={video_id}&bpctr=9999999999&has_verified=1")
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
    fn an_oversized_session_index_is_not_wrapped() {
        assert_eq!(
            parse_ytcfg(r#"ytcfg.set({"SESSION_INDEX":4294967296});"#).session_index,
            None
        );
        assert_eq!(
            parse_ytcfg(r#"ytcfg.set({"SESSION_INDEX":4294967295});"#).session_index,
            Some(u32::MAX)
        );
    }
    use crate::streams::TV_DOWNGRADED;

    const ORIGIN: &str = "https://www.youtube.com";

    /// The expected values were computed with yt-dlp 2026.08.19's
    /// `_make_sid_authorization` on the same made-up cookies.
    #[test]
    fn authorization_matches_yt_dlp() {
        let sids = Sids {
            sapisid: Some("sapisid-x".into()),
            one_p: Some("p1-x".into()),
            three_p: Some("p3-x".into()),
        };
        assert_eq!(
            sid_authorization(&sids, ORIGIN, Some("12345678"), 1_700_000_000).as_deref(),
            Some(
                "SAPISIDHASH 1700000000_d13b198d5203fa836fedb3fc51b35bbbe151e58a_u \
                 SAPISID1PHASH 1700000000_7694e93c5724a2c10b9ab04fa89c08d2d2ebf294_u \
                 SAPISID3PHASH 1700000000_acfa2d82632e699d8207fed2c569d5a9b5b080bd_u"
            )
        );
        let only = Sids {
            sapisid: Some("sapisid-x".into()),
            one_p: None,
            three_p: None,
        };
        assert_eq!(
            sid_authorization(&only, ORIGIN, None, 1_700_000_000).as_deref(),
            Some("SAPISIDHASH 1700000000_51c629ba7ecca6188cb5c894cfb4ced202941e0f")
        );
        let none = Sids {
            sapisid: None,
            one_p: None,
            three_p: None,
        };
        assert_eq!(sid_authorization(&none, ORIGIN, None, 1), None);
    }

    #[test]
    fn reads_the_ytcfg_fields() {
        let page = r#"<script>ytcfg.set({"CLIENT_CANARY_STATE":"none"});</script>
            <script>ytcfg.set({"DATASYNC_ID":"user-1||","LOGGED_IN":true,
            "PLAYER_JS_URL":"/s/player/0a1b2c3d/player_es6.vflset/en_US/base.js",
            "SESSION_INDEX":"0","STS":20728,"VISITOR_DATA":"visitor-1"});</script>"#;
        assert_eq!(
            parse_ytcfg(page),
            WebConfig {
                player: Some("0a1b2c3d".into()),
                sts: Some(20728),
                visitor: Some("visitor-1".into()),
                user_session: Some("user-1".into()),
                delegated_session: None,
                session_index: Some(0),
                cookies: SetCookies::default(),
            }
        );
        let brand = parse_ytcfg(r#"{"DATASYNC_ID":"page-2||user-1","SESSION_INDEX":1}"#);
        assert_eq!(brand.delegated_session.as_deref(), Some("page-2"));
        assert_eq!(brand.user_session.as_deref(), Some("user-1"));
        assert_eq!(brand.session_index, Some(1));
        assert_eq!(parse_ytcfg("no config here"), WebConfig::default());
    }

    /// The body yt-dlp sends as `tv_downgraded` (its `--print-traffic`
    /// output on 2026-10-07, video id aside).
    #[test]
    fn tv_body_is_yt_dlps() {
        let body = player_body(&TV_DOWNGRADED, "aaaaaaaaaaa", Some(20728));
        let expected: Value = serde_json::from_str(
            r#"{"context": {"client": {"clientName": "TVHTML5", "clientVersion": "5.20260707",
            "userAgent": "Mozilla/5.0 (ChromiumStylePlatform) Cobalt/Version", "hl": "en",
            "timeZone": "UTC", "utcOffsetMinutes": 0}}, "videoId": "aaaaaaaaaaa",
            "playbackContext": {"contentPlaybackContext": {"html5Preference": "HTML5_PREF_WANTS",
            "signatureTimestamp": 20728}}, "contentCheckOk": true, "racyCheckOk": true}"#,
        )
        .expect("json");
        assert_eq!(body, expected);
    }

    /// The page's cookies replace the browser's (yt-dlp's jar), and a
    /// deleted one is dropped.
    #[test]
    fn page_cookies_replace_the_sessions() {
        let mut set = SetCookies::default();
        set.add(
            [
                "SIDCC=new-1; expires=Thu, 07-Oct-2027 14:18:25 GMT; path=/; domain=.youtube.com",
                "__Secure-YEC=gone; Domain=.youtube.com; Expires=Thu, 11-Jan-2024 14:18:25 GMT; Path=/",
                "NEWONE=new-2; Path=/",
                "OLD=x; Max-Age=0",
            ],
            2026,
        );
        assert_eq!(
            set.apply("SID=a; SIDCC=old; __Secure-YEC=b; OLD=c"),
            "SID=a; SIDCC=new-1; NEWONE=new-2"
        );
        assert_eq!(
            format!("{set:?}"),
            r#"["SIDCC", "__Secure-YEC", "NEWONE", "OLD"]"#
        );
    }

    #[test]
    fn headers_carry_the_session() {
        let web = WebConfig {
            visitor: Some("visitor-1".into()),
            session_index: Some(0),
            ..WebConfig::default()
        };
        let headers = player_headers(&TV_DOWNGRADED, &web, Some("SAPISIDHASH x".into()));
        let get = |name: &str| {
            headers
                .iter()
                .find(|(k, _)| *k == name)
                .map(|(_, v)| v.as_str())
        };
        assert_eq!(get("X-YouTube-Client-Name"), Some("7"));
        assert_eq!(get("X-YouTube-Client-Version"), Some("5.20260707"));
        assert_eq!(
            get("User-Agent"),
            Some("Mozilla/5.0 (ChromiumStylePlatform) Cobalt/Version")
        );
        assert_eq!(get("X-Goog-Visitor-Id"), Some("visitor-1"));
        assert_eq!(get("X-Goog-AuthUser"), Some("0"));
        assert_eq!(get("X-Goog-PageId"), None);
        assert_eq!(get("Authorization"), Some("SAPISIDHASH x"));
        assert_eq!(get("X-Origin"), Some(ORIGIN));
        let signed_out = player_headers(&TV_DOWNGRADED, &WebConfig::default(), None);
        assert!(signed_out.iter().all(|(k, _)| *k != "Authorization"));
    }
}
