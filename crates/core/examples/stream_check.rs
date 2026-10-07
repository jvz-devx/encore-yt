//! Real-stream check of the audio engine (PLAN M19, M23): resolves one song
//! with the stream resolver in several formats at once (one search
//! suggestion for the visitor id, the player script only when it isn't
//! cached, and one `player` request), then plays each
//! format through `ytfast-audio`: first audio, a seek to the middle, a seek
//! to 3 s before the end with the same stream queued behind it, and the
//! join between the two (the played length against the container's, and
//! the frames between one track's end and the next one's start).
//!
//! Nothing is reported to the account's history: nothing here sends
//! YouTube's playback tracking. Signed out by default (VISIONOS);
//! `--signed-in` asks as the account, as the app does (WEB_REMIX, then
//! WEB_CREATOR, then VISIONOS signed out), for the Premium formats, so use
//! it only when that is wanted. The client that answered is printed, and
//! why the ones before it failed.
//!
//! cargo run --example stream_check --
//!     [--signed-in] [--formats 251,250,249,140] VIDEO_ID
//!
//! (Premium: `--signed-in --formats 774,141`.) The resolved formats are kept
//! for an hour in the runtime directory (0600), so running it again plays
//! them without resolving the song again.

use std::sync::mpsc::Receiver;
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use ytfast_audio::{Engine, Event, Load};

static START: OnceLock<Instant> = OnceLock::new();

fn now() -> f64 {
    START.get_or_init(Instant::now).elapsed().as_secs_f64()
}

macro_rules! step {
    ($($arg:tt)*) => { println!("[{:8.3}] {}", now(), format!($($arg)*)) };
}

struct Logger;

impl log::Log for Logger {
    fn enabled(&self, m: &log::Metadata) -> bool {
        m.level() <= log::Level::Info
            && (m.target().starts_with("ytfast_audio") || m.target().starts_with("ytfast::streams"))
    }
    fn log(&self, r: &log::Record) {
        if self.enabled(r.metadata()) {
            println!("[{:8.3}]   {}", now(), r.args());
        }
    }
    fn flush(&self) {}
}

/// One format of the song, as the resolver gave it.
struct Format {
    itag: String,
    url: String,
    user_agent: Option<String>,
}

fn main() -> Result<()> {
    now();
    log::set_logger(&Logger).map_err(|e| anyhow::anyhow!("{e}"))?;
    log::set_max_level(log::LevelFilter::Info);
    let mut args = std::env::args().skip(1);
    let (mut signed_in, mut formats, mut video) = (false, "251,250,249,140".to_owned(), None);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--signed-in" => signed_in = true,
            "--formats" => formats = args.next().context("--formats needs a list")?,
            _ => video = Some(arg),
        }
    }
    let video = video.context("usage: stream_check [--signed-in] [--formats LIST] VIDEO_ID")?;
    let formats = resolve(&video, &formats, signed_in)?;
    let engine = Engine::start()?;
    let events = engine.take_events().context("engine events")?;
    let mut rows = Vec::new();
    for format in &formats {
        step!("== itag {}", format.itag);
        match check(&engine, &events, format) {
            Ok(row) => rows.push(row),
            Err(error) => rows.push(format!("itag {}: FAILED: {error:#}", format.itag)),
        }
        engine.stop(0);
    }
    println!();
    for row in rows {
        println!("{row}");
    }
    report_usage();
    Ok(())
}

/// One resolve for every format in `list` (comma-separated), or the
/// formats an earlier run saved within the hour.
fn resolve(video: &str, list: &str, signed_in: bool) -> Result<Vec<Format>> {
    let who = if signed_in { "signed-in" } else { "signed-out" };
    let saved = ytfast::paths::Paths::new()?
        .runtime
        .join(format!("stream-check-{video}-{who}.tsv"));
    let fresh = std::fs::metadata(&saved)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.elapsed().ok())
        .is_some_and(|age| age < Duration::from_secs(3600));
    if fresh {
        let formats = parse(&std::fs::read_to_string(&saved)?);
        if list.split(',').all(|f| formats.iter().any(|g| g.itag == f)) {
            step!(
                "using the {} formats resolved within the hour",
                formats.len()
            );
            return Ok(formats
                .into_iter()
                .filter(|f| list.split(',').any(|l| l == f.itag))
                .collect());
        }
    }
    let started = Instant::now();
    let text = tokio::runtime::Runtime::new()?.block_on(resolve_now(video, signed_in))?;
    save(&saved, &text)?;
    let formats: Vec<Format> = parse(&text)
        .into_iter()
        .filter(|f| list.split(',').any(|l| l == f.itag))
        .collect();
    step!(
        "resolved {} of the wanted formats in {:.1} s ({})",
        formats.len(),
        started.elapsed().as_secs_f64(),
        if signed_in { "signed in" } else { "signed out" }
    );
    Ok(formats)
}

/// Every audio format the app would pick from, one per line: itag, user
/// agent (`NA`), URL.
async fn resolve_now(video: &str, signed_in: bool) -> Result<String> {
    let paths = ytfast::paths::Paths::new()?;
    let client = Arc::new(ytfast::innertube::Client::new());
    if signed_in {
        let preferred = ytfast::settings::Settings::load(&paths).browser_profile;
        let session = ytfast::auth::load(&paths.runtime, preferred.as_deref())?;
        client.set_session(Some(session));
    }
    // The visitor id the stream requests need, as the app has it from
    // YouTube Music's answers.
    client
        .suggestions("a")
        .await
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    let native = ytfast::streams::Native::new(client, &paths.cache, &paths.config);
    if signed_in {
        native.prepare().await?;
    }
    let (streams, who) = native.streams(video, signed_in, usize::MAX).await?;
    let itags: Vec<String> = streams.iter().map(|s| s.itag.to_string()).collect();
    step!("{who} answered with itags {}", itags.join(","));
    Ok(streams
        .iter()
        .map(|s| format!("{}\tNA\t{}\n", s.itag, s.url))
        .collect())
}

/// The saved lines: format id, user agent, URL.
fn parse(text: &str) -> Vec<Format> {
    text.lines()
        .filter_map(|line| {
            let mut parts = line.splitn(3, '\t');
            let (itag, agent, url) = (parts.next()?, parts.next()?, parts.next()?);
            Some(Format {
                itag: itag.to_owned(),
                url: url.to_owned(),
                user_agent: (agent != "NA").then(|| agent.to_owned()),
            })
        })
        .collect()
}

/// Stream URLs are good for hours: only this user may read them.
fn save(path: &std::path::Path, text: &str) -> Result<()> {
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    std::io::Write::write_all(&mut options.open(path)?, text.as_bytes())?;
    Ok(())
}

fn check(engine: &Engine, events: &Receiver<Event>, format: &Format) -> Result<String> {
    let load = Load {
        headers: format
            .user_agent
            .iter()
            .map(|a| ("User-Agent".to_owned(), a.clone()))
            .collect(),
        ..Load::default()
    };
    let asked = now();
    let a = engine.load(0, &format.url, load.clone())?;
    wait(
        events,
        |e| matches!(e, Event::Started { track, .. } if *track == a),
    )?;
    let first = (now() - asked) * 1000.0;
    // A seek far ahead at once, before the download gets there: a new
    // range request at the target.
    let early = engine.stats(0).and_then(|s| s.duration).unwrap_or(60.0) * 0.8;
    let early_http = engine.stats(0).map_or(0, |s| s.http.downloaded);
    let asked = now();
    engine.seek(0, early.floor());
    wait(
        events,
        |e| matches!(e, Event::Seeked { track, .. } if *track == a),
    )?;
    let seek_early = (now() - asked) * 1000.0;
    step!(
        "seeked to {:.0} s right away ({early_http} bytes downloaded) in {seek_early:.0} ms",
        early.floor()
    );
    engine.seek(0, 0.0);
    wait(
        events,
        |e| matches!(e, Event::Seeked { track, .. } if *track == a),
    )?;
    std::thread::sleep(Duration::from_secs(3));
    let stats = engine.stats(0).context("no stats")?;
    let duration = stats.duration.context("the container gave no length")?;
    step!("playing at {:.2} s of {duration:.3} s", stats.position);

    let middle = (duration / 2.0).floor();
    let asked = now();
    engine.seek(0, middle);
    wait(
        events,
        |e| matches!(e, Event::Seeked { track, .. } if *track == a),
    )?;
    let seek_middle = (now() - asked) * 1000.0;
    std::thread::sleep(Duration::from_secs(2));
    let at = engine.stats(0).map_or(0.0, |s| s.position);
    step!("seeked to {middle} s in {seek_middle:.0} ms; 2 s later at {at:.2} s");

    let near_end = duration - 3.0;
    let asked = now();
    engine.seek(0, near_end);
    wait(
        events,
        |e| matches!(e, Event::Seeked { track, .. } if *track == a),
    )?;
    let seek_end = (now() - asked) * 1000.0;
    let b = engine.queue(0, &format.url, load)?;
    let (ended_at, length) = match wait(
        events,
        |e| matches!(e, Event::Ended { track, .. } if *track == a),
    )? {
        Event::Ended { at, length, .. } => (at, length),
        _ => unreachable!(),
    };
    let started_at = match wait(
        events,
        |e| matches!(e, Event::Started { track, .. } if *track == b),
    )? {
        Event::Started { at, .. } => at,
        _ => unreachable!(),
    };
    let gap = (started_at.as_secs_f64() - ended_at.as_secs_f64()) * f64::from(engine.output_rate());
    std::thread::sleep(Duration::from_secs(2));
    let next = engine.stats(0).context("no stats after the join")?;
    let http = &stats.http;
    let row = format!(
        "itag {}: first audio {first:.0} ms; early seek {seek_early:.0} ms; seek to middle {seek_middle:.0} ms, near end {seek_end:.0} ms; \
         played {length:.3} s of {duration:.3} s ({:+.1} ms); join gap {gap:.0} frames; \
         next track at {:.2} s; starved {:.3} s; http {} requests, {} of {} bytes at 3 s",
        format.itag,
        (length - duration) * 1000.0,
        next.position,
        next.starved,
        http.requests,
        http.downloaded,
        http.len
    );
    step!("{row}");
    Ok(row)
}

/// Waits up to 30 s for an event `want` accepts; an error event fails.
fn wait(events: &Receiver<Event>, want: impl Fn(&Event) -> bool) -> Result<Event> {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        let event = events.recv_timeout(left).context("timed out")?;
        if let Event::Error { message, .. } = &event {
            bail!("{message}");
        }
        if want(&event) {
            return Ok(event);
        }
    }
}

/// CPU time over wall time and peak memory, from /proc (Linux).
fn report_usage() {
    let stat = std::fs::read_to_string("/proc/self/stat").unwrap_or_default();
    let fields: Vec<&str> = stat
        .rsplit(')')
        .next()
        .unwrap_or("")
        .split_whitespace()
        .collect();
    let ticks: f64 = fields.get(11..13).map_or(0.0, |f| {
        f.iter().filter_map(|v| v.parse::<f64>().ok()).sum()
    });
    let status = std::fs::read_to_string("/proc/self/status").unwrap_or_default();
    let peak = status
        .lines()
        .find(|l| l.starts_with("VmHWM"))
        .unwrap_or("VmHWM: ?");
    step!(
        "cpu {:.2} s over {:.1} s wall; peak {}",
        ticks / 100.0,
        now(),
        peak.split_whitespace()
            .skip(1)
            .collect::<Vec<_>>()
            .join(" ")
    );
}
