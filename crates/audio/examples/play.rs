//! Plays a track over HTTP, seeks, turns on an EQ preset and continues
//! gaplessly into a second track, printing a timestamp for every step.
//!
//! cargo run -p encore-audio --example play -- <url> [<next-url>]
//!
//! Environment: SEEK_AT / SEEK_TO (default 5 / 30 s; SEEK_AT=0 skips the
//! seek), EQ_AT (default 8 s), TAIL (seconds of the next track, default 5),
//! GAIN_DB (loudness gain for both tracks, default 0), STOP_AT (stop after
//! this many seconds whatever happens, for measurements).

use std::sync::OnceLock;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use encore_audio::{Engine, Event, Load};

/// The backend's "Bass boost" preset (src/equalizer.rs).
const BASS_BOOST: [f32; 10] = [6.0, 5.0, 4.0, 2.0, 0.5, 0.0, 0.0, 0.0, 0.0, 0.0];

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
        // DEBUG=<target prefix> shows that module's debug lines too.
        let debug = std::env::var("DEBUG").is_ok_and(|t| m.target().starts_with(&t));
        m.level() <= log::Level::Info || debug
    }
    fn log(&self, r: &log::Record) {
        if self.enabled(r.metadata()) {
            eprintln!("[{:8.3}] {} {}", now(), r.level(), r.args());
        }
    }
    fn flush(&self) {}
}

fn env(name: &str, default: f64) -> f64 {
    std::env::var(name)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

fn main() -> Result<()> {
    now();
    log::set_logger(&Logger).map_err(|e| anyhow::anyhow!("{e}"))?;
    log::set_max_level(log::LevelFilter::Debug);
    let mut args = std::env::args().skip(1);
    let first = args.next().context("usage: play <url> [<next-url>]")?;
    let second = args.next();
    let (seek_at, seek_to) = (env("SEEK_AT", 5.0), env("SEEK_TO", 30.0));
    let (eq_at, tail) = (env("EQ_AT", 8.0), env("TAIL", 5.0));
    let stop_at = env("STOP_AT", f64::INFINITY);
    let load = Load {
        gain_db: env("GAIN_DB", 0.0) as f32,
        ..Load::default()
    };

    let engine = Engine::start()?;
    let events = engine
        .take_events()
        .context("audio event channel is unavailable")?;
    step!("output open at {} Hz", engine.output_rate());
    let asked = now();
    let a = engine.load(0, &first, load.clone())?;
    step!("load track {a}: {first}");
    let mut b = None;
    let (mut started_at, mut seek_asked, mut eq_done) = (None::<f64>, None::<f64>, false);
    let mut ended_at = None::<Duration>;
    let mut finish_at = None::<f64>;
    loop {
        while let Ok(event) = events.try_recv() {
            match event {
                Event::Started { track, at, .. } => {
                    step!(
                        "track {track} started (output time {:.6} s)",
                        at.as_secs_f64()
                    );
                    if track == a {
                        step!("first audio {:.0} ms after load", (now() - asked) * 1000.0);
                        started_at = Some(now());
                        if let Some(next) = &second {
                            let queued = engine.queue(0, next, load.clone())?;
                            b = Some(queued);
                            step!("queued track {queued} for a gapless join: {next}");
                        }
                    } else if Some(track) == b {
                        if let Some(end) = ended_at {
                            let gap = at.as_secs_f64() - end.as_secs_f64();
                            step!(
                                "gap at the join: {:.0} frames",
                                gap * engine.output_rate() as f64
                            );
                        }
                        finish_at = Some(now() + tail);
                    }
                }
                Event::Seeked { track, at, .. } => {
                    let took = seek_asked.map_or(0.0, |s| now() - s) * 1000.0;
                    step!(
                        "track {track} seeked: first frame out at {:.3} s, {took:.0} ms after the seek",
                        at.as_secs_f64()
                    );
                }
                Event::Ended {
                    track, at, length, ..
                } => {
                    step!(
                        "track {track} ended at {length:.6} s of its own time ({:.0} frames), output time {:.6} s",
                        length * engine.output_rate() as f64,
                        at.as_secs_f64()
                    );
                    ended_at = Some(at);
                    if second.is_none() || Some(track) == b {
                        finish_at = Some(now());
                    }
                }
                Event::Error { track, message, .. } => {
                    step!("track {track} error: {message}");
                    finish_at = Some(now());
                }
            }
        }
        if let Some(start) = started_at {
            let t = now() - start;
            if seek_at > 0.0 && seek_asked.is_none() && t >= seek_at {
                seek_asked = Some(now());
                step!("seek to {seek_to} s");
                engine.seek(0, seek_to);
            }
            if !eq_done && t >= eq_at {
                eq_done = true;
                engine.set_equalizer(Some(BASS_BOOST));
                step!("equalizer: Bass boost");
            }
        }
        if finish_at.is_some_and(|f| now() >= f) || now() >= stop_at {
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    if let Some(stats) = engine.stats(0) {
        step!(
            "track {} at {:.2} s of {:?}, starved {:.3} s, http: {} requests, {} of {} bytes",
            stats.track,
            stats.position,
            stats.duration,
            stats.starved,
            stats.http.requests,
            stats.http.downloaded,
            stats.http.len
        );
    }
    report_usage();
    Ok(())
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
    // USER_HZ is 100 on Linux.
    let cpu = ticks / 100.0;
    let status = std::fs::read_to_string("/proc/self/status").unwrap_or_default();
    let peak = status
        .lines()
        .find(|l| l.starts_with("VmHWM"))
        .unwrap_or("VmHWM: ?");
    step!(
        "cpu {:.2} s over {:.1} s wall = {:.1} % of one core; {}",
        cpu,
        now(),
        cpu / now() * 100.0,
        peak.split_whitespace()
            .skip(1)
            .collect::<Vec<_>>()
            .join(" ")
    );
}
