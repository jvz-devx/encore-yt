//! The sign-in window (M31 spike): opens YouTube Music in the operating
//! system's own web engine (WebView2, WKWebView or WebKitGTK), waits for the
//! session cookies that mean "signed in", and saves them as a Netscape
//! cookie file for the app to import like any other.
//!
//! Usage: `encore-yt-signin --out <cookies file>`. Prints `signed in` or
//! `cancelled` on stdout and nothing else; never a cookie name or value.
//! Closing the window is cancelling. `ENCORE_SIGNIN_URL` replaces the start
//! page (for checks against a local page).

// No console window in a Windows release build.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use tao::dpi::LogicalSize;
use tao::event::{Event, WindowEvent};
use tao::event_loop::{ControlFlow, EventLoopBuilder};
use tao::window::WindowBuilder;
use wry::WebViewBuilder;

/// Google's own sign-in, handing over to YouTube Music, which sets the
/// youtube.com cookies the app signs in with.
const START: &str = "https://accounts.google.com/ServiceLogin?service=youtube&continue=https%3A%2F%2Fmusic.youtube.com%2F";
const POLL: Duration = Duration::from_secs(1);

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
    let Some(out) = out_path() else {
        eprintln!("usage: encore-yt-signin --out <cookies file>");
        std::process::exit(2);
    };
    let start = std::env::var("ENCORE_SIGNIN_URL").unwrap_or_else(|_| START.into());
    // Persist only after the webview and event loop have closed, so filesystem
    // latency cannot stall an active sign-in window.
    let result = run(&start).and_then(|rows| match rows {
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

fn out_path() -> Option<PathBuf> {
    let mut args = std::env::args_os().skip(1);
    while let Some(arg) = args.next() {
        if arg == "--out" {
            return args.next().map(PathBuf::from);
        }
    }
    None
}

/// Runs the window until session cookies appear or the user closes it.
fn run(start: &str) -> Result<Option<Vec<Row>>, Box<dyn std::error::Error>> {
    let event_loop = EventLoopBuilder::new().build();
    let window = WindowBuilder::new()
        .with_title("Sign in to YouTube Music")
        .with_inner_size(LogicalSize::new(480.0, 720.0))
        .build(&event_loop)?;
    // Private browsing: nothing of this window stays on disk once it closes.
    let builder = WebViewBuilder::new().with_incognito(true).with_url(start);
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
                let Ok(cookies) = webview.cookies() else {
                    // Web-engine errors may contain session details. Only report
                    // the failed operation, never cookies or the raw error.
                    outcome = Err("couldn't read cookies from the sign-in window".into());
                    *flow = ControlFlow::Exit;
                    return;
                };
                let rows: Vec<Row> = cookies.iter().filter_map(row).collect();
                if rows.iter().any(signs_in) {
                    outcome = Ok(Some(rows));
                    *flow = ControlFlow::Exit;
                }
            }
            _ => {}
        }
    });
    drop(webview);
    drop(window);
    outcome.map_err(Into::into)
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
