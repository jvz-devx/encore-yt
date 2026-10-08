//! Backend-to-renderer check, exclusively against a stand-in started by the
//! caller on this computer. Requires local-only mode, a fake stream and the
//! renderer's exact UUID; refuses to run with a signed-in session.
#![allow(clippy::print_stdout, reason = "check evidence")]

use std::time::{Duration, Instant};

use anyhow::{Context, Result, ensure};
use encore_core::backend::{Backend, Command, Event};
use encore_core::casting::Session;
use encore_core::model::{Playback, Track};
use encore_core::paths::Paths;

fn wait(backend: &Backend, label: &str, mut accepts: impl FnMut(Event) -> bool) -> Result<()> {
    let deadline = Instant::now() + Duration::from_secs(15);
    while Instant::now() < deadline {
        if let Ok(event) = backend.events.recv_timeout(Duration::from_millis(250)) {
            if let Event::Cast(state) = &event {
                println!("    cast={:?} error={:?}", state.session, state.error);
                ensure!(
                    state.error.is_none(),
                    "casting failed during {label}: {:?}",
                    state.error
                );
            }
            if let Event::Playback(p) = &event {
                println!(
                    "    index={:?} position={:.2} playing={} loading={}",
                    p.index, p.position, p.playing, p.loading
                );
            }
            if accepts(event) {
                println!("ok  {label}");
                return Ok(());
            }
        }
    }
    anyhow::bail!("timed out: {label}")
}

fn playback(backend: &Backend, label: &str, accepts: impl Fn(&Playback) -> bool) -> Result<()> {
    wait(
        backend,
        label,
        |event| matches!(event, Event::Playback(p) if accepts(&p)),
    )
}

fn track(n: usize) -> Track {
    Track {
        video_id: format!("local-cast-{n}"),
        title: format!("Local song {n}"),
        artists: Vec::new(),
        album: None,
        thumbnail: None,
        duration: Some(180),
        like: None,
        set_video_id: None,
    }
}

fn main() -> Result<()> {
    ensure!(
        std::env::var("ENCORE_CAST_LOCAL_ONLY").as_deref() == Ok("1"),
        "local-only mode is required"
    );
    ensure!(
        encore_core::resolver::fake_stream().is_some(),
        "a local fake stream is required"
    );
    let local_seek = std::env::args().skip(1).any(|arg| arg == "--local-seek");
    let uuid = if local_seek {
        String::new()
    } else {
        std::env::var("ENCORE_CAST_TEST_UDN").context("set the UUID of the stand-in you started")?
    };
    struct Logger;
    impl log::Log for Logger {
        fn enabled(&self, _: &log::Metadata<'_>) -> bool {
            true
        }
        fn log(&self, record: &log::Record<'_>) {
            if self.enabled(record.metadata()) {
                eprintln!("{}", record.args());
            }
        }
        fn flush(&self) {}
    }
    let _ = log::set_logger(&Logger);
    log::set_max_level(if local_seek {
        log::LevelFilter::Info
    } else {
        log::LevelFilter::Warn
    });
    let root = std::env::temp_dir().join(format!("cast-backend-check-{}", std::process::id()));
    let paths = Paths {
        config: root.join("config"),
        cache: root.join("cache"),
        runtime: root.join("runtime"),
    };
    for dir in [&paths.config, &paths.cache, &paths.runtime] {
        std::fs::create_dir_all(dir)?;
    }
    let backend = Backend::start(paths, || {})?;
    backend.send(Command::Autoplay(false));
    backend.send(Command::PlayTracks {
        tracks: vec![track(1), track(2), track(3)],
        start: 0,
    });
    playback(&backend, "local playback", |p| p.playing && !p.loading)?;
    if local_seek {
        for target in [0.5, 6.76, 50.76] {
            backend.send(Command::Seek(target));
            playback(
                &backend,
                &format!("native decoded seek at {target} seconds"),
                |p| {
                    p.playing
                        && !p.loading
                        && p.position > target + 0.1
                        && p.position < target + 1.0
                },
            )?;
        }
        backend.shutdown();
        return Ok(());
    }
    backend.send(Command::Seek(12.0));
    playback(&backend, "local seek to 12 seconds", |p| {
        p.position >= 12.0 && p.position < 14.0
    })?;
    backend.send(Command::CastScan(true));
    let mut found = None;
    wait(&backend, "SSDP discovers our renderer", |event| {
        if let Event::Cast(state) = event {
            found = state.devices.into_iter().find(|d| d.id == uuid);
        }
        found.is_some()
    })?;
    let device = found.context("our stand-in wasn't found")?;
    backend.send(Command::CastScan(false));
    backend.send(Command::CastConnect {
        id: device.id,
        kind: device.kind,
        takeover: false,
    });
    wait(
        &backend,
        "remote session active",
        |event| matches!(event, Event::Cast(s) if matches!(s.session, Some(Session::Active(_)))),
    )?;
    playback(&backend, "remote keeps the local position", |p| {
        p.playing && p.position >= 12.0 && p.position < 18.0
    })?;
    backend.send(Command::TogglePause);
    playback(&backend, "remote pause", |p| !p.playing && !p.loading)?;
    backend.send(Command::Seek(35.0));
    playback(&backend, "remote seek to 35 seconds", |p| {
        p.position >= 35.0 && p.position < 37.0
    })?;
    backend.send(Command::Volume(25.0));
    playback(&backend, "remote volume command", |p| p.volume == 25.0)?;
    backend.send(Command::TogglePause);
    playback(&backend, "remote play", |p| p.playing)?;
    backend.send(Command::Next);
    playback(&backend, "next song starts remotely", |p| {
        p.index == Some(1) && p.playing && !p.loading && p.position < 2.0
    })?;
    backend.send(Command::Previous);
    playback(&backend, "previous song starts remotely", |p| {
        p.index == Some(0) && p.playing && !p.loading && p.position < 2.0
    })?;
    backend.send(Command::Seek(178.0));
    playback(&backend, "natural EOF advances the remote queue", |p| {
        p.index == Some(1) && p.playing && !p.loading && p.position < 2.0
    })?;
    backend.send(Command::TogglePause);
    playback(&backend, "pause before returning locally", |p| {
        !p.playing && !p.loading
    })?;
    backend.send(Command::Seek(42.0));
    playback(&backend, "position before disconnect", |p| {
        p.position >= 42.0 && p.position < 44.0
    })?;
    backend.send(Command::CastDisconnect);
    wait(
        &backend,
        "casting ends",
        |event| matches!(event, Event::Cast(s) if s.session.is_none()),
    )?;
    playback(&backend, "local handoff stays paused at 42 seconds", |p| {
        p.index == Some(1) && !p.playing && !p.loading && p.position >= 42.0 && p.position < 44.0
    })?;
    backend.send(Command::TogglePause);
    playback(&backend, "local playback resumes from the handoff", |p| {
        p.index == Some(1) && p.playing && !p.loading && p.position >= 42.0 && p.position < 45.0
    })?;
    backend.shutdown();
    Ok(())
}
