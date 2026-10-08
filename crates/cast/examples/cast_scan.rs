#![allow(
    clippy::print_stdout,
    reason = "command-line example output is intentional"
)]

//! Lists the Cast devices (mDNS `_googlecast._tcp`) and DLNA renderers
//! (SSDP `MediaRenderer:1`) on the local network, with what each DLNA
//! renderer says it plays. Read-only: nothing is launched or changed.
//!
//! cargo run -p encore-cast --example cast_scan -- [--seconds N]

use std::collections::BTreeSet;
use std::time::Duration;

use anyhow::Result;
use encore_cast::{dlna, mdns, ssdp};

#[tokio::main]
async fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let mut seconds = 3;
    while let Some(arg) = args.next() {
        if arg == "--seconds" {
            seconds = args.next().and_then(|s| s.parse().ok()).unwrap_or(seconds);
        }
    }
    let wait = Duration::from_secs(seconds);
    let started = std::time::Instant::now();
    let (cast, renderers) =
        tokio::join!(mdns::scan(wait), dlna::scan(ssdp::MULTICAST.parse()?, wait));
    let cast = cast?;
    let renderers = renderers?;
    println!("Cast devices (mDNS _googlecast._tcp): {}", cast.len());
    for d in &cast {
        let mut notes = Vec::new();
        if d.is_group() {
            notes.push("group".to_owned());
        }
        if !d.status.is_empty() {
            notes.push(format!("showing: {}", d.status));
        }
        println!(
            "  {:<28} {:<22} {:<22} {}",
            d.name,
            d.model,
            d.addr,
            notes.join(", ")
        );
    }
    println!("DLNA renderers (SSDP MediaRenderer:1): {}", renderers.len());
    for r in &renderers {
        let formats = match r.sink_formats().await {
            Ok(sink) => audio_types(&sink),
            Err(error) => format!("formats unknown ({error:#})"),
        };
        println!(
            "  {:<28} {:<22} {:<16} {}",
            r.name,
            format!("{} {}", r.manufacturer, r.model),
            r.ip,
            formats
        );
    }
    println!("scan took {:.1}s", started.elapsed().as_secs_f64());
    Ok(())
}

/// The audio MIME types in a ConnectionManager Sink list.
fn audio_types(sink: &[String]) -> String {
    let types: BTreeSet<&str> = sink
        .iter()
        .filter_map(|entry| entry.split(':').nth(2))
        .filter(|mime| mime.starts_with("audio/"))
        .collect();
    types.into_iter().collect::<Vec<_>>().join(" ")
}
