//! Response cookie renewal, serialized with account changes and private file writes.

use std::sync::{Arc, Mutex};

use crate::auth::Session;
use crate::sync::Recover;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) struct SessionToken(u64);

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) struct ConnectionToken(u64);

#[derive(Default)]
struct State {
    generation: u64,
    connection: u64,
    current: Option<Session>,
    dirty: bool,
}

#[derive(Clone, Default)]
pub(super) struct Sessions(Arc<Mutex<State>>);

impl Sessions {
    pub fn set(&self, session: Option<Session>) {
        let mut state = self.0.lock().recover();
        state.connection = state.connection.wrapping_add(1);
        state.generation = state.generation.wrapping_add(1);
        state.current = session;
        state.dirty = false;
    }

    pub fn begin_connection(&self) -> ConnectionToken {
        let mut state = self.0.lock().recover();
        state.connection = state.connection.wrapping_add(1);
        ConnectionToken(state.connection)
    }

    pub fn connection_is_current(&self, token: ConnectionToken) -> bool {
        self.0.lock().recover().connection == token.0
    }

    pub fn set_for_connection(&self, token: ConnectionToken, session: Option<Session>) -> bool {
        let mut state = self.0.lock().recover();
        if state.connection != token.0 {
            return false;
        }
        state.generation = state.generation.wrapping_add(1);
        state.current = session;
        state.dirty = false;
        true
    }

    pub fn for_connection(&self, token: ConnectionToken, change: impl FnOnce()) -> bool {
        let state = self.0.lock().recover();
        if state.connection != token.0 {
            return false;
        }
        change();
        true
    }

    /// Import and renewal share the same lock and file slots. Invalidate the
    /// old login only after a successful import, before its new file is read.
    pub fn replace<T, E>(
        &self,
        save: impl FnOnce() -> std::result::Result<T, E>,
    ) -> std::result::Result<T, E> {
        let mut state = self.0.lock().recover();
        let saved = save()?;
        state.connection = state.connection.wrapping_add(1);
        state.generation = state.generation.wrapping_add(1);
        state.current = None;
        state.dirty = false;
        Ok(saved)
    }

    /// The credentials and token are read under the same lock. Responses
    /// from an earlier login cannot update a newly selected account.
    pub fn with<T>(&self, read: impl FnOnce(Option<&Session>, Option<SessionToken>) -> T) -> T {
        let state = self.0.lock().recover();
        let token = state
            .current
            .as_ref()
            .map(|_| SessionToken(state.generation));
        read(state.current.as_ref(), token)
    }

    pub async fn remember(&self, token: Option<SessionToken>, response: &reqwest::Response) {
        let Some(token) = token else {
            return;
        };
        let headers = response
            .headers()
            .get_all(reqwest::header::SET_COOKIE)
            .iter()
            .filter_map(|header| header.to_str().ok().map(str::to_owned))
            .collect();
        self.remember_headers(token, response.url().clone(), headers)
            .await;
    }

    async fn remember_headers(&self, token: SessionToken, url: reqwest::Url, headers: Vec<String>) {
        let store = self.clone();
        // Hold the session lock through the atomic write on the blocking pool:
        // a slower response cannot save an older snapshot over a newer one.
        if tokio::task::spawn_blocking(move || {
            let mut state = store.0.lock().recover();
            if state.generation != token.0 || state.current.is_none() {
                return;
            }
            let changed = state
                .current
                .as_mut()
                .is_some_and(|s| s.renew(&url, &headers));
            state.dirty |= changed;
            if state.dirty {
                match state.current.as_ref().map(Session::persist) {
                    Some(Ok(())) => state.dirty = false,
                    Some(Err(_)) => {
                        // No headers, cookie values or raw web-engine errors.
                        log::warn!("couldn't save renewed YouTube session cookies; will retry");
                    }
                    None => {}
                }
            }
        })
        .await
        .is_err()
        {
            log::warn!("YouTube session cookie worker stopped");
        }
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    reason = "tests use only synthetic session cookies"
)]
mod tests {
    use super::*;
    use crate::auth::Cookie;
    use crate::innertube::Client;
    use reqwest::ResponseBuilderExt;

    fn session(value: &str, persist: Option<std::path::PathBuf>) -> Session {
        Session {
            source: "Synthetic import".into(),
            profile: "synthetic".into(),
            cookies: vec![Cookie {
                host: ".youtube.com".into(),
                name: "SAPISID".into(),
                value: value.into(),
                path: "/".into(),
                secure: true,
                expires: 0,
            }],
            persist,
        }
    }

    #[tokio::test]
    async fn renewal_survives_restarting_the_client() {
        let path = std::env::temp_dir().join(format!(
            "encore-cookie-restart-{}-{}.txt",
            std::process::id(),
            fastrand::u64(..)
        ));
        let first = session("before", Some(path.clone()));
        first.persist().unwrap();
        let client = Client::new().unwrap();
        client.set_session(Some(first));
        let token = client.with_session(|_, token| token);
        let music: reqwest::Response = http::Response::builder()
            .url(
                "https://music.youtube.com/youtubei/v1/browse"
                    .parse()
                    .unwrap(),
            )
            .header(
                "Set-Cookie",
                "SAPISID=after; Domain=.youtube.com; Path=/; Secure; Max-Age=3600",
            )
            .header(
                "Set-Cookie",
                "SIDCC=music; Domain=.youtube.com; Path=/; Secure",
            )
            .body("")
            .unwrap()
            .into();
        client.remember_cookies(token, &music).await;
        let playback: reqwest::Response = http::Response::builder()
            .url(
                "https://www.youtube.com/youtubei/v1/player"
                    .parse()
                    .unwrap(),
            )
            .header(
                "Set-Cookie",
                "SIDCC=playback; Domain=.youtube.com; Path=/; Secure",
            )
            .body("")
            .unwrap()
            .into();
        client.remember_cookies(token, &playback).await;
        assert_eq!(client.cookie("SAPISID"), Some("after".into()));
        drop(client);

        // Read the on-disk Netscape snapshot into a fresh client, just as a
        // restart does. No credentials from the previous client are reused.
        let saved = std::fs::read_to_string(&path).unwrap();
        let cookies = crate::auth::test_parse_netscape(&saved);
        let restarted = Client::new().unwrap();
        let mut next = session("unused", Some(path.clone()));
        next.cookies = cookies;
        restarted.set_session(Some(next));
        assert_eq!(
            restarted.cookie_header().as_deref(),
            Some("SAPISID=after; SIDCC=playback")
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn signed_in_requests_send_cookies_for_the_actual_request_path() {
        let client = Client::new().unwrap();
        let mut saved = session("root", None);
        saved.cookies.push(Cookie {
            host: ".youtube.com".into(),
            name: "SIDCC".into(),
            value: "api".into(),
            path: "/youtubei/v1".into(),
            secure: true,
            expires: 0,
        });
        client.set_session(Some(saved));
        let url = "https://music.youtube.com/youtubei/v1/browse";
        let (request, token) = client.auth_headers(client.http().post(url), url);
        let request = request.build().unwrap();
        assert!(token.is_some());
        assert_eq!(request.headers()["Cookie"], "SAPISID=root; SIDCC=api");
        assert!(request.headers().contains_key("Authorization"));
        let root = "https://music.youtube.com/getAccountSwitcherEndpoint";
        let (request, _) = client.auth_headers(client.http().get(root), root);
        assert_eq!(request.build().unwrap().headers()["Cookie"], "SAPISID=root");
    }

    #[tokio::test]
    async fn responses_from_an_old_login_cannot_overwrite_the_new_login() {
        let store = Sessions::default();
        store.set(Some(session("old", None)));
        let token = store.with(|_, token| token.unwrap());
        // Even selecting the same profile again starts a new generation.
        store.set(Some(session("new", None)));
        store
            .remember_headers(
                token,
                "https://music.youtube.com/".parse().unwrap(),
                vec!["SAPISID=late; Domain=.youtube.com; Path=/; Secure".into()],
            )
            .await;
        assert_eq!(store.with(|s, _| s.unwrap().header()), "SAPISID=new");
        store.set(None);
        store
            .remember_headers(
                token,
                "https://music.youtube.com/".parse().unwrap(),
                vec!["SAPISID=late; Domain=.youtube.com; Path=/; Secure".into()],
            )
            .await;
        assert!(store.with(|s, _| s.is_none()));
    }

    #[tokio::test]
    async fn importing_cookies_invalidates_renewal_before_reconnect_loads_the_file() {
        let store = Sessions::default();
        store.set(Some(session("old", None)));
        let token = store.with(|_, token| token.unwrap());
        let failed: std::result::Result<(), ()> = store.replace(|| Err(()));
        assert!(failed.is_err());
        assert_eq!(store.with(|s, _| s.unwrap().header()), "SAPISID=old");
        let saved: std::result::Result<(), ()> = store.replace(|| Ok(()));
        assert!(saved.is_ok());
        store
            .remember_headers(
                token,
                "https://music.youtube.com/".parse().unwrap(),
                vec!["SAPISID=late; Domain=.youtube.com; Path=/; Secure".into()],
            )
            .await;
        assert!(store.with(|s, _| s.is_none()));
    }

    #[test]
    fn an_old_connection_cannot_sign_out_or_restore_over_a_new_login() {
        let store = Sessions::default();
        let old = store.begin_connection();
        assert!(store.set_for_connection(old, Some(session("old", None))));
        let new = store.begin_connection();
        assert!(store.set_for_connection(new, Some(session("new", None))));
        assert!(!store.set_for_connection(old, None));
        assert!(!store.set_for_connection(old, Some(session("late-load", None))));
        assert!(!store.for_connection(old, || panic!("old channel must not be selected")));
        assert!(!store.connection_is_current(old));
        assert!(store.connection_is_current(new));
        assert_eq!(store.with(|s, _| s.unwrap().header()), "SAPISID=new");
        let saved: std::result::Result<(), ()> = store.replace(|| Ok(()));
        assert!(saved.is_ok());
        assert!(!store.set_for_connection(new, Some(session("late-load", None))));
        assert!(store.with(|s, _| s.is_none()));
    }

    #[tokio::test]
    async fn a_failed_cookie_write_is_retried_on_the_next_response() {
        let dir = std::env::temp_dir().join(format!(
            "encore-cookie-retry-{}-{}",
            std::process::id(),
            fastrand::u64(..)
        ));
        let path = dir.join("cookies.txt");
        let store = Sessions::default();
        store.set(Some(session("before", Some(path.clone()))));
        let token = store.with(|_, token| token.unwrap());
        // The missing directory makes the first private atomic write fail.
        store
            .remember_headers(
                token,
                "https://music.youtube.com/".parse().unwrap(),
                vec!["SAPISID=after; Domain=.youtube.com; Path=/; Secure".into()],
            )
            .await;
        assert!(!path.exists());
        assert_eq!(store.with(|s, _| s.unwrap().header()), "SAPISID=after");
        std::fs::create_dir(&dir).unwrap();
        // No additional rotation is needed to trigger a retry.
        store
            .remember_headers(
                token,
                "https://music.youtube.com/".parse().unwrap(),
                Vec::new(),
            )
            .await;
        let saved = std::fs::read_to_string(&path).unwrap();
        assert!(
            crate::auth::test_parse_netscape(&saved)
                .iter()
                .any(|c| c.name == "SAPISID" && c.value == "after")
        );
        std::fs::remove_dir_all(dir).unwrap();
    }
}
