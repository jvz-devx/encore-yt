//! Live check of the Rust stream resolver (`encore_core::streams`): resolves the
//! given songs and fetches the first KB of each URL. Each song costs one
//! `player` request and one range fetch; signed out, one search-suggestions
//! call first gives the visitor id; the player script is fetched only when
//! `<cache>/player` has no current one. Mind YouTube's rate limits.
//!
//! `cargo run --example resolve_rust -- [--signed-in]
//! [--client visionos|tv|creator] [--cache DIR] VIDEO_ID...` (`--client`
//! asks only that client, so a failure costs no second request; `--no-fetch`
//! skips the range fetch; `--dump DIR` saves the player responses). Exits
//! with status 1 if the player can't be prepared, a song doesn't resolve or
//! a range fetch doesn't answer 200 or 206, and with 3 if every song met
//! YouTube's bot check ("Sign in to confirm you're not a bot", which
//! datacenter IPs get; that says nothing about the resolver). The resolver
//! canary (`scripts/resolver-canary.sh`) runs this signed out.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use anyhow::{Context, Result};
use encore_core::innertube::Client;
use encore_core::paths::Paths;
use encore_core::streams::{self, Native};

#[tokio::main]
async fn main() -> Result<()> {
    let mut args = std::env::args().skip(1).peekable();
    let mut signed_in = false;
    let mut cache = None;
    let mut only = None;
    let mut fetch = true;
    let mut dump = None;
    let mut ids = Vec::new();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--signed-in" => signed_in = true,
            "--cache" => cache = args.next().map(PathBuf::from),
            "--no-fetch" => fetch = false,
            "--dump" => dump = args.next().map(PathBuf::from),
            "--client" => {
                only = Some(match args.next().as_deref() {
                    Some("visionos") => &streams::VISIONOS,
                    // YouTube Music's web client, the app's first when signed in.
                    Some("music") => &streams::WEB_REMIX,
                    Some("tv") => &streams::TV_DOWNGRADED,
                    Some("creator") => &streams::WEB_CREATOR,
                    other => anyhow::bail!("unknown client {other:?}"),
                })
            }
            _ => ids.push(arg),
        }
    }
    let paths = Paths::new()?;
    let client = Arc::new(Client::new());
    let mut requests = 0;
    if signed_in {
        let preferred = encore_core::settings::Settings::load(&paths).browser_profile;
        let session =
            encore_core::auth::load(&paths.runtime, preferred.as_deref()).context("signing in")?;
        println!("signed in from {}", session.source);
        client.set_session(Some(session));
    }
    {
        let started = Instant::now();
        client
            .suggestions("a")
            .await
            .map_err(|e| anyhow::anyhow!("{e}"))?;
        requests += 1;
        println!(
            "visitor id: {} ({:.2}s)",
            client.visitor_data().is_some(),
            started.elapsed().as_secs_f64()
        );
    }
    let (mut failures, mut bot_checks, mut resolved) = (0, 0, 0);
    let mut native = Native::new(client.clone(), &cache.unwrap_or(paths.cache), &paths.config);
    println!("EJS solver {}", native.solver_version());
    if let Some(dir) = &dump {
        native.dump_responses(dir.clone());
    }
    let started = Instant::now();
    let player = native.prepare().await?;
    println!(
        "player {player} ready in {:.2}s",
        started.elapsed().as_secs_f64()
    );
    for id in ids {
        let started = Instant::now();
        let stream = match only {
            Some(client) => native
                .resolve_as(client, &id, signed_in)
                .await
                .map(|s| (s, client.name)),
            None => native.resolve(&id, signed_in).await,
        };
        requests += 1;
        let elapsed = started.elapsed().as_secs_f64();
        let (stream, from) = match stream {
            Ok(resolved) => resolved,
            Err(error) if format!("{error:#}").contains("LOGIN_REQUIRED") => {
                println!("{id}: bot check after {elapsed:.2}s: {error:#}");
                bot_checks += 1;
                continue;
            }
            Err(error) => {
                println!("{id}: failed after {elapsed:.2}s: {error:#}");
                failures += 1;
                continue;
            }
        };
        resolved += 1;
        println!(
            "{id}: itag {} from {} in {elapsed:.2}s, expires in {} min",
            stream.itag,
            from,
            stream.expires.saturating_sub(encore_core::resolver::now()) / 60
        );
        if let (Some(dir), Some(client)) = (&dump, only) {
            let saved = std::fs::read_to_string(dir.join(format!("{}-{id}.json", client.name)))?;
            let format = streams::best_audio(&serde_json::from_str(&saved)?)?;
            let names = |url: &str| -> Vec<String> {
                reqwest::Url::parse(url)
                    .map(|u| u.query_pairs().map(|(k, _)| k.into_owned()).collect())
                    .unwrap_or_default()
            };
            println!(
                "{id}: response had cipher {}, n in URL {}; resolved URL has {:?}",
                format.cipher.is_some(),
                format
                    .url
                    .as_deref()
                    .or(format.cipher.as_deref())
                    .is_some_and(|u| u.contains("n=")),
                names(&stream.url)
            );
        }
        if !fetch {
            continue;
        }
        let started = Instant::now();
        let mut request = client
            .http()
            .get(&stream.url)
            .header("Range", "bytes=0-1023");
        if let Some(agent) = &stream.user_agent {
            request = request.header("User-Agent", agent);
        }
        let response = request.send().await;
        requests += 1;
        match response {
            Ok(response) => {
                let status = response.status();
                if !matches!(status.as_u16(), 200 | 206) {
                    failures += 1;
                }
                let bytes = response.bytes().await.map(|b| b.len()).unwrap_or(0);
                println!(
                    "{id}: range 0-1023 -> {status}, {bytes} bytes in {:.2}s",
                    started.elapsed().as_secs_f64()
                );
            }
            Err(error) => {
                println!("{id}: range fetch failed: {error}");
                failures += 1;
            }
        }
    }
    println!("YouTube requests: {requests} (plus the player script if it was downloaded)");
    if failures > 0 {
        anyhow::bail!("{failures} of the checks failed");
    }
    if resolved == 0 && bot_checks > 0 {
        println!("every song met the bot check: inconclusive");
        std::process::exit(3);
    }
    Ok(())
}
