//! The sign-in window (M31 spike): opens YouTube Music in the operating
//! system's own web engine (WebView2, WKWebView or WebKitGTK), waits for the
//! session cookies that mean "signed in", and saves them as a Netscape
//! cookie file for the app to import like any other.
//!
//! Usage: `encore-yt-signin --out <cookies file> [--profile <directory>]`.
//! Windows retains an Encore-owned WebView2 profile between runs. Prints `signed in` or
//! `cancelled` on stdout and nothing else; never a cookie name or value.
//! Closing the window is cancelling. `ENCORE_SIGNIN_URL` replaces the start
//! page (for checks against a local page).

// No console window in a Windows release build.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tao::dpi::LogicalSize;
use tao::event::{Event, WindowEvent};
use tao::event_loop::{ControlFlow, EventLoopBuilder};
use tao::window::WindowBuilder;
use wry::{PageLoadEvent, WebContext, WebViewBuilder};

/// Google's own sign-in, handing over to YouTube Music, which sets the
/// youtube.com cookies the app signs in with.
const START: &str = "https://accounts.google.com/ServiceLogin?service=youtube&continue=https%3A%2F%2Fmusic.youtube.com%2F";
const POLL: Duration = Duration::from_secs(1);
const CHECK_LOGIN: &str =
    "Boolean(window.ytcfg && window.ytcfg.get && window.ytcfg.get('LOGGED_IN'))";

/// One cookie, as a line of a Netscape cookie file.
struct Row {
    host: String,
    path: String,
    secure: bool,
    expires: i64,
    name: String,
    value: String,
}

#[allow(
    clippy::print_stdout,
    reason = "signed in/cancelled is the helper protocol"
)]
fn main() {
    let Some((out, profile)) = options() else {
        eprintln!("usage: encore-yt-signin --out <cookies file> [--profile <directory>]");
        std::process::exit(2);
    };
    let start = std::env::var("ENCORE_SIGNIN_URL").unwrap_or_else(|_| START.into());
    // Persist only after the webview and event loop have closed, so filesystem
    // latency cannot stall an active sign-in window.
    let profile = match profile {
        Some(profile) => Ok(Some(profile)),
        None => default_profile(),
    };
    let result = profile
        .map_err(Into::into)
        .and_then(|profile| run(&start, profile.as_deref()))
        .and_then(|rows| match rows {
            Some(rows) => write(&out, &rows).map(|()| true).map_err(Into::into),
            None => Ok(false),
        });
    match result {
        Ok(true) => println!("signed in"),
        Ok(false) => println!("cancelled"),
        Err(error) => {
            eprintln!("sign-in window: {error}");
            std::process::exit(1);
        }
    }
}

fn options() -> Option<(PathBuf, Option<PathBuf>)> {
    let mut args = std::env::args_os().skip(1);
    let mut out = None;
    let mut profile = None;
    while let Some(arg) = args.next() {
        if arg == "--out" {
            out = Some(PathBuf::from(args.next()?));
        } else if arg == "--profile" {
            profile = Some(PathBuf::from(args.next()?));
        } else {
            return None;
        }
    }
    Some((out?, profile))
}

fn default_profile() -> Result<Option<PathBuf>, String> {
    #[cfg(windows)]
    {
        // Stable across app and PC restarts. Never share Edge's user profile:
        // this belongs to Encore and inherits the user's LOCALAPPDATA ACLs.
        let local =
            std::env::var_os("LOCALAPPDATA").ok_or("couldn't find the sign-in profile folder")?;
        Ok(Some(
            PathBuf::from(local)
                .join("encore-yt")
                .join("signin-profile"),
        ))
    }
    #[cfg(not(windows))]
    {
        Ok(None)
    }
}

/// Runs the window until session cookies appear or the user closes it.
fn run(
    start: &str,
    profile: Option<&Path>,
) -> Result<Option<Vec<Row>>, Box<dyn std::error::Error>> {
    let event_loop = EventLoopBuilder::new().build();
    let window = WindowBuilder::new()
        .with_title("Sign in to YouTube Music")
        .with_inner_size(LogicalSize::new(480.0, 720.0))
        .build(&event_loop)?;
    if let Some(profile) = profile {
        let mut dir = std::fs::DirBuilder::new();
        dir.recursive(true);
        #[cfg(unix)]
        std::os::unix::fs::DirBuilderExt::mode(&mut dir, 0o700);
        dir.create(profile)?;
    }
    let mut context = WebContext::new(profile.map(Path::to_owned));
    let ready = Arc::new(Mutex::new(Readiness::default()));
    let loaded = ready.clone();
    let builder = WebViewBuilder::new_with_web_context(&mut context)
        .with_incognito(profile.is_none())
        .with_url(start)
        .with_on_page_load_handler(move |event, url| {
            loaded
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .load(event, &url);
        });
    #[cfg(target_os = "linux")]
    let webview = {
        use tao::platform::unix::WindowExtUnix;
        use wry::WebViewBuilderExtUnix;
        let vbox = window.default_vbox().ok_or("no GTK box in the window")?;
        builder.build_gtk(vbox)?
    };
    #[cfg(not(target_os = "linux"))]
    let webview = builder.build(&window)?;

    let mut outcome: Result<Option<Vec<Row>>, String> = Ok(None);
    let mut next = Instant::now() + POLL;
    let mut event_loop = event_loop;
    use tao::platform::run_return::EventLoopExtRunReturn;
    event_loop.run_return(|event, _, flow| {
        *flow = ControlFlow::WaitUntil(next);
        match event {
            Event::WindowEvent {
                event: WindowEvent::CloseRequested,
                ..
            } => *flow = ControlFlow::Exit,
            Event::NewEvents(tao::event::StartCause::ResumeTimeReached { .. }) => {
                next = Instant::now() + POLL;
                *flow = ControlFlow::WaitUntil(next);
                let state = ready.lock().unwrap_or_else(|e| e.into_inner());
                let generation = state.generation;
                let finished = state.finished;
                let signed_in = state.signed_in;
                drop(state);
                if !finished {
                    return;
                }
                if !signed_in {
                    // A retained profile can already contain SAPISID while its
                    // Google session needs another sign-in. Wait until Music
                    // itself reports logged in; cookie presence alone isn't enough.
                    let checked = ready.clone();
                    if webview
                        .evaluate_script_with_callback(CHECK_LOGIN, move |value| {
                            checked
                                .lock()
                                .unwrap_or_else(|e| e.into_inner())
                                .confirm(generation, value.trim() == "true");
                        })
                        .is_err()
                    {
                        outcome = Err("couldn't check the sign-in page".into());
                        *flow = ControlFlow::Exit;
                    }
                    return;
                }
                let Ok(cookies) = webview.cookies() else {
                    // Web-engine errors may contain session details. Only report
                    // the failed operation, never cookies or the raw error.
                    outcome = Err("couldn't read cookies from the sign-in window".into());
                    *flow = ControlFlow::Exit;
                    return;
                };
                let rows: Vec<Row> = cookies.iter().filter_map(row).collect();
                if rows.iter().any(signs_in)
                    && ready
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .can_export(generation)
                {
                    outcome = Ok(Some(rows));
                    *flow = ControlFlow::Exit;
                }
            }
            _ => {}
        }
    });
    drop(webview);
    drop(window);
    drop(context);
    outcome.map_err(Into::into)
}

#[derive(Default)]
struct Readiness {
    generation: u64,
    finished: bool,
    signed_in: bool,
}

impl Readiness {
    fn load(&mut self, event: PageLoadEvent, url: &str) {
        match event {
            PageLoadEvent::Started => {
                self.generation = self.generation.wrapping_add(1);
                self.finished = false;
                self.signed_in = false;
            }
            PageLoadEvent::Finished => self.finished = music_page(url),
        }
    }

    fn confirm(&mut self, generation: u64, signed_in: bool) {
        if self.generation == generation && self.finished {
            self.signed_in = signed_in;
        }
    }

    fn can_export(&self, generation: u64) -> bool {
        self.generation == generation && self.finished && self.signed_in
    }
}

fn music_page(url: &str) -> bool {
    let Ok(url) = url.parse::<wry::http::Uri>() else {
        return false;
    };
    (url.scheme_str() == Some("https") && url.host() == Some("music.youtube.com"))
        || (cfg!(debug_assertions)
            && !test_host().is_empty()
            && url.host() == Some(test_host().as_str()))
}

/// A YouTube or Google cookie as a row; others are left behind.
fn row(cookie: &wry::cookie::Cookie<'static>) -> Option<Row> {
    let domain = test_alias(cookie.domain()?);
    let bare = domain.trim_start_matches('.');
    let ours = ["youtube.com", "google.com"]
        .iter()
        .any(|d| bare == *d || bare.ends_with(&format!(".{d}")));
    if !ours {
        return None;
    }
    // A host-only cookie has no leading dot in WebKit's list, a domain
    // cookie has one; keep that, as the cookie file's flag derives from it.
    Some(Row {
        host: domain,
        path: cookie.path().unwrap_or("/").to_string(),
        secure: cookie.secure().unwrap_or(false),
        expires: cookie
            .expires_datetime()
            .map(|t| t.unix_timestamp())
            .unwrap_or(0),
        name: cookie.name().to_string(),
        value: cookie.value().to_string(),
    })
}

/// Debug builds only: a host (`ENCORE_SIGNIN_TEST_HOST=localhost`) whose
/// cookies stand in for youtube.com's, to check the reading and the file
/// against a local page. Release builds have no such host.
fn test_host() -> String {
    if cfg!(debug_assertions) {
        std::env::var("ENCORE_SIGNIN_TEST_HOST").unwrap_or_default()
    } else {
        String::new()
    }
}

fn test_alias(domain: &str) -> String {
    let host = test_host();
    if !host.is_empty() && domain.trim_start_matches('.') == host {
        ".youtube.com".into()
    } else {
        domain.into()
    }
}

/// The cookies the app signs in with: the same two names, on the same hosts,
/// as encore-core's `auth::signs_in`.
fn signs_in(row: &Row) -> bool {
    let music = matches!(
        row.host.as_str(),
        "music.youtube.com" | ".music.youtube.com" | ".youtube.com" | "youtube.com"
    );
    music && (row.name == "SAPISID" || row.name == "__Secure-3PAPISID")
}

/// A Netscape cookie file, the format encore-core's `Session::write_netscape`
/// writes and "Import a cookies file" reads.
fn netscape(rows: &[Row]) -> String {
    let mut text = String::from("# Netscape HTTP Cookie File\n");
    for r in rows {
        let flag = if r.host.starts_with('.') {
            "TRUE"
        } else {
            "FALSE"
        };
        let secure = if r.secure { "TRUE" } else { "FALSE" };
        text.push_str(&format!(
            "{}\t{flag}\t{}\t{secure}\t{}\t{}\t{}\n",
            r.host, r.path, r.expires, r.name, r.value
        ));
    }
    text
}

/// Writes the file private to this user (0600 on Unix; `%TEMP%` and
/// `%APPDATA%` are the user's own on Windows).
fn write(out: &Path, rows: &[Row]) -> Result<(), String> {
    let mut options = std::fs::OpenOptions::new();
    // create_new refuses existing files and symlinks atomically. mode alone
    // would leave an existing file's permissive mode unchanged.
    options.write(true).create_new(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    let mut file = options
        .open(out)
        .map_err(|e| format!("couldn't write the cookie file ({})", e.kind()))?;
    file.write_all(netscape(rows).as_bytes())
        .map_err(|e| format!("couldn't write the cookie file ({})", e.kind()))
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
    fn saved_cookies_are_not_exported_before_music_confirms_the_login() {
        let mut state = Readiness::default();
        state.load(PageLoadEvent::Started, START);
        let generation = state.generation;
        state.confirm(generation, true);
        assert!(!state.can_export(generation));
        state.load(PageLoadEvent::Finished, START);
        state.confirm(generation, true);
        assert!(!state.can_export(generation));
        state.load(PageLoadEvent::Started, "https://music.youtube.com/");
        let music = state.generation;
        state.load(PageLoadEvent::Finished, "https://music.youtube.com/");
        state.confirm(music, false);
        assert!(!state.can_export(music));
        state.confirm(music, true);
        assert!(state.can_export(music));
        // An asynchronous check of an earlier page must not close a new
        // sign-in/challenge page using cookies already in the profile.
        state.load(PageLoadEvent::Started, START);
        state.confirm(music, true);
        assert!(!state.can_export(state.generation));
    }

    #[test]
    fn only_the_music_origin_can_finish_sign_in() {
        assert!(music_page("https://music.youtube.com/"));
        assert!(!music_page("https://music.youtube.com.example/"));
        assert!(!music_page("https://example.com/music.youtube.com"));
        assert!(!music_page("https://accounts.google.com/"));
        assert!(!music_page("http://music.youtube.com/"));
    }

    fn row(host: &str, name: &str) -> Row {
        Row {
            host: host.into(),
            path: "/".into(),
            secure: true,
            expires: 0,
            name: name.into(),
            value: "x".into(),
        }
    }

    #[test]
    fn only_youtube_session_cookies_sign_in() {
        assert!(signs_in(&row(".youtube.com", "SAPISID")));
        assert!(signs_in(&row("music.youtube.com", "__Secure-3PAPISID")));
        assert!(!signs_in(&row(".google.com", "SAPISID")));
        assert!(!signs_in(&row(".youtube.com", "VISITOR_INFO1_LIVE")));
    }

    #[test]
    fn writes_tab_separated_lines() {
        let text = netscape(&[row(".youtube.com", "SAPISID")]);
        assert_eq!(
            text,
            "# Netscape HTTP Cookie File\n.youtube.com\tTRUE\t/\tTRUE\t0\tSAPISID\tx\n"
        );
    }

    #[test]
    fn output_is_private_and_existing_files_are_not_overwritten() {
        let path = std::env::temp_dir().join(format!("encore-signin-write-{}", std::process::id()));
        let rows = [row(".youtube.com", "SAPISID")];
        write(&path, &rows).expect("create synthetic cookie output");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
        assert!(write(&path, &[]).is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), netscape(&rows));
        std::fs::remove_file(path).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn output_does_not_follow_symlinks() {
        let path = std::env::temp_dir().join(format!("encore-signin-link-{}", std::process::id()));
        let target = path.with_extension("target");
        std::fs::write(&target, "unchanged").unwrap();
        std::os::unix::fs::symlink(&target, &path).unwrap();
        assert!(write(&path, &[row(".youtube.com", "SAPISID")]).is_err());
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "unchanged");
        std::fs::remove_file(path).unwrap();
        std::fs::remove_file(target).unwrap();
    }
}
