//! Plays a local file on a Cast device or DLNA renderer through the relay,
//! for a few seconds, then stops it. Prints the device's status as it goes
//! and every request the device made to the relay (ranges, user agent).
//!
//! cargo run -p encore-cast --example cast_play --
//!     [--seconds N] [--seek S] [--mime TYPE] [--proxy] [--force] DEVICE FILE
//!
//! `--proxy` serves the file from a second relay on 127.0.0.1 and has the
//! LAN relay fetch it from there as a remote source, ranges passed on: the
//! path a googlevideo URL takes, without YouTube.
//!
//! DEVICE is part of a name or an IP address from `cast_scan`. A Cast
//! device that is showing something other than its idle screen is left
//! alone unless `--force` is given. Use a test file, never a signed-in
//! stream: the device fetches whatever the relay serves.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use encore_cast::castv2::{self, Client, DEFAULT_MEDIA_RECEIVER, Media};
use encore_cast::dlna::{self, Renderer, Track};
use encore_cast::relay::{Relay, Source};
use encore_cast::{Device, local_ip_for, mdns, ssdp};

struct Options {
    seconds: u64,
    seek: Option<f64>,
    mime: Option<String>,
    force: bool,
    proxy: bool,
    device: String,
    file: PathBuf,
}

#[tokio::main]
async fn main() -> Result<()> {
    struct Stdout;
    impl log::Log for Stdout {
        fn enabled(&self, m: &log::Metadata) -> bool {
            m.level() <= log::Level::Info && m.target().starts_with("encore_cast")
        }
        fn log(&self, r: &log::Record) {
            if self.enabled(r.metadata()) {
                println!("    [{}] {}", r.target(), r.args());
            }
        }
        fn flush(&self) {}
    }
    log::set_logger(&Stdout).ok();
    log::set_max_level(log::LevelFilter::Info);

    let options = options()?;
    let mime = options
        .mime
        .clone()
        .unwrap_or_else(|| mime_for(&options.file).to_owned());
    let device = find(&options.device).await?;
    println!(
        "device: {} ({}, {})",
        device.name(),
        device.model(),
        device.ip()
    );
    let local = local_ip_for(device.ip())?;
    let relay = Relay::start(local).await?;
    let upstream = Relay::start("127.0.0.1".parse()?).await?;
    let source = if options.proxy {
        Source::Remote(upstream.publish(Source::File(options.file.clone()), &mime)?)
    } else {
        Source::File(options.file.clone())
    };
    let url = relay.publish(source, &mime)?;
    println!(
        "relay: {} serving {} as {mime}",
        relay.addr(),
        options.file.display()
    );
    let started = Instant::now();
    let outcome = match &device {
        Device::Cast(d) => play_cast(d.addr, &url, &mime, &options).await,
        Device::Dlna(r) => play_dlna(r, &url, &mime, &options).await,
    };
    println!("relay requests ({:.1}s):", started.elapsed().as_secs_f64());
    for a in relay.accesses() {
        println!(
            "  {} {} range={} -> {} {} bytes  UA: {}",
            a.peer,
            a.method,
            a.range.as_deref().unwrap_or("-"),
            a.status,
            a.bytes,
            a.user_agent
        );
    }
    outcome
}

fn options() -> Result<Options> {
    let mut args = std::env::args().skip(1);
    let mut o = Options {
        seconds: 6,
        seek: None,
        mime: None,
        force: false,
        proxy: false,
        device: String::new(),
        file: PathBuf::new(),
    };
    let mut positional = Vec::new();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--seconds" => o.seconds = args.next().context("--seconds N")?.parse()?,
            "--seek" => o.seek = Some(args.next().context("--seek S")?.parse()?),
            "--mime" => o.mime = Some(args.next().context("--mime TYPE")?),
            "--force" => o.force = true,
            "--proxy" => o.proxy = true,
            _ => positional.push(arg),
        }
    }
    let [device, file] = <[String; 2]>::try_from(positional)
        .map_err(|_| anyhow::anyhow!("usage: cast_play [options] DEVICE FILE"))?;
    o.device = device;
    o.file = file.into();
    anyhow::ensure!(o.file.is_file(), "no file {}", o.file.display());
    Ok(o)
}

fn mime_for(path: &std::path::Path) -> &'static str {
    match path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or_default()
    {
        "webm" => "audio/webm",
        "m4a" | "mp4" => "audio/mp4",
        "ogg" | "opus" => "audio/ogg",
        "mp3" => "audio/mpeg",
        "flac" => "audio/flac",
        "wav" => "audio/wav",
        _ => "application/octet-stream",
    }
}

async fn find(wanted: &str) -> Result<Device> {
    let wait = Duration::from_secs(3);
    let (cast, renderers) =
        tokio::join!(mdns::scan(wait), dlna::scan(ssdp::MULTICAST.parse()?, wait));
    let all: Vec<Device> = cast?
        .into_iter()
        .map(Device::Cast)
        .chain(renderers?.into_iter().map(Device::Dlna))
        .collect();
    let wanted_lower = wanted.to_lowercase();
    all.into_iter()
        .find(|d| d.ip().to_string() == wanted || d.name().to_lowercase().contains(&wanted_lower))
        .with_context(|| format!("no Cast device or DLNA renderer matches '{wanted}'"))
}

async fn play_cast(addr: std::net::SocketAddr, url: &str, mime: &str, o: &Options) -> Result<()> {
    let mut client = Client::connect(addr).await.context("connect")?;
    let status = client.receiver_status().await?;
    println!(
        "receiver: volume {:?}, apps {:?}",
        status.volume,
        status
            .apps
            .iter()
            .map(|a| &a.display_name)
            .collect::<Vec<_>>()
    );
    if let Some(busy) = status.busy_app()
        && !o.force
    {
        bail!(
            "the device is showing {}; not interrupting it (--force to anyway)",
            busy.display_name
        );
    }
    let app = client.launch(DEFAULT_MEDIA_RECEIVER).await?;
    println!("launched {} (session {})", app.display_name, app.session_id);
    let media = Media {
        url: url.to_owned(),
        content_type: mime.to_owned(),
        title: "Encore cast test".into(),
        artist: "Encore".into(),
        ..Media::default()
    };
    let loaded = client.load(&app, &media).await;
    match &loaded {
        Ok(s) => println!("LOAD -> {} at {:.1}s", s.player_state, s.current_time),
        Err(error) => println!("LOAD failed: {error:#}"),
    }
    if let Ok(loaded) = loaded {
        let id = loaded.media_session_id;
        let deadline = Instant::now() + Duration::from_secs(o.seconds);
        let mut sought = o.seek.is_none();
        while Instant::now() < deadline {
            tokio::select! {
                Some(event) = client.events.recv() => {
                    if let Some(s) = castv2::MediaStatus::parse(&event.payload) {
                        println!("  status: {} at {:.1}s {}", s.player_state, s.current_time, s.idle_reason.unwrap_or_default());
                    }
                }
                _ = tokio::time::sleep(Duration::from_millis(1500)) => {
                    if let Some(s) = client.media(&app, "GET_STATUS", id).await? {
                        println!("  poll: {} at {:.1}s", s.player_state, s.current_time);
                        if !sought && s.player_state == "PLAYING" {
                            sought = true;
                            let to = o.seek.unwrap_or_default();
                            let after = client.seek(&app, id, to).await?;
                            println!("  SEEK {to}s -> {:?}", after.map(|s| (s.player_state, s.current_time)));
                        }
                    }
                }
            }
        }
    }
    client.stop_app(&app).await?;
    println!("stopped the app");
    Ok(())
}

async fn play_dlna(r: &Renderer, url: &str, mime: &str, o: &Options) -> Result<()> {
    let track = Track {
        url: url.to_owned(),
        mime: mime.to_owned(),
        title: "Encore cast test".into(),
        artist: "Encore".into(),
        ..Track::default()
    };
    r.set_uri(&track).await?;
    r.play().await?;
    println!("SetAVTransportURI + Play sent");
    let deadline = Instant::now() + Duration::from_secs(o.seconds);
    let mut sought = o.seek.is_none();
    while Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(1500)).await;
        let state = r.state().await?;
        let (at, length) = r.position().await?;
        println!("  poll: {state} at {at:?} of {length:?}");
        if !sought && state == "PLAYING" {
            sought = true;
            let to = o.seek.unwrap_or_default();
            match r.seek(Duration::from_secs_f64(to)).await {
                Ok(()) => println!("  Seek {to}s sent"),
                Err(error) => println!("  Seek failed: {error:#}"),
            }
        }
    }
    r.stop().await?;
    println!("stopped");
    Ok(())
}
