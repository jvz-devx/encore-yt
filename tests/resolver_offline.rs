//! Checks the embedded JS challenge solver against yt-dlp's, offline. The
//! player script is YouTube's, so it isn't committed: capture one under
//! `artifacts/resolver/` (`<id>.js`, see docs/gpui/RESOLVER.md) and write the
//! answers yt-dlp's EJS gives in deno with `scripts/ejs-expected.sh`. Without
//! a capture the test says so and passes. scripts/resolver-canary.sh runs
//! it on the current player in CI.

use std::collections::HashMap;
use std::path::PathBuf;

use serde::Deserialize;
use ytfast::jsc::scripts::{self, Scripts};
use ytfast::jsc::{Challenges, Engine, Player, decipher};

#[derive(Deserialize)]
struct Expected {
    n: HashMap<String, String>,
    sig: HashMap<usize, Vec<usize>>,
}

/// `artifacts/resolver/`, or `YTFAST_RESOLVER_CAPTURES` (the canary's
/// fresh capture of the current player).
fn captures() -> Vec<(Player, Expected)> {
    let dir = std::env::var_os("YTFAST_RESOLVER_CAPTURES")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("artifacts/resolver"));
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut found = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let Some(id) = name.strip_suffix(".expected.json") else {
            continue;
        };
        let expected = std::fs::read_to_string(entry.path()).expect("expected answers");
        let expected: Expected = serde_json::from_str(&expected).expect("expected answers");
        let path = dir.join(format!("{id}.js"));
        // A fresh copy so the test also runs the preprocessing step.
        let scratch =
            std::env::temp_dir().join(format!("ytfast-jsc-{}-{id}.js", std::process::id()));
        std::fs::copy(&path, &scratch).expect("player script");
        found.push((
            Player {
                id: id.to_owned(),
                path: scratch,
            },
            expected,
        ));
    }
    found
}

#[test]
fn quickjs_solves_like_ytdlp() {
    // The engine's own thread size: meriyah recurses deeply on the player,
    // deeper than a test thread's 2 MB allow.
    std::thread::Builder::new()
        .stack_size(64 << 20)
        .spawn(solve_captures)
        .expect("solver thread")
        .join()
        .expect("solver thread");
}

fn solve_captures() {
    let captures = captures();
    if captures.is_empty() {
        eprintln!("no player captures under artifacts/resolver; nothing to compare");
        return;
    }
    let mut engine = Engine::new().expect("engine");
    for (player, expected) in captures {
        let challenges = Challenges {
            n: expected.n.keys().cloned().collect(),
            sig_lengths: expected.sig.keys().copied().collect(),
        };
        let started = std::time::Instant::now();
        let solved = engine.solve(&player, &challenges);
        let cold = started.elapsed();
        let preprocessed = engine.preprocessed(&player);
        let _ = std::fs::remove_file(&player.path);
        let solved = solved.unwrap_or_else(|e| panic!("player {}: {e:#}", player.id));
        assert_eq!(solved.n, expected.n, "n answers for player {}", player.id);
        assert_eq!(
            solved.sig, expected.sig,
            "signature specs for player {}",
            player.id
        );
        // A new engine loads the saved preprocessed player instead.
        let started = std::time::Instant::now();
        let reloaded = Engine::new().expect("engine").solve(&player, &challenges);
        let load = started.elapsed();
        let _ = std::fs::remove_file(&preprocessed);
        let reloaded = reloaded.unwrap_or_else(|e| panic!("player {} reloaded: {e:#}", player.id));
        assert_eq!(
            reloaded.n, expected.n,
            "n answers after reloading {}",
            player.id
        );
        let started = std::time::Instant::now();
        engine
            .solve(
                &player,
                &Challenges {
                    n: vec!["warmwarmwarm1234".into()],
                    sig_lengths: vec![],
                },
            )
            .expect("warm solve");
        eprintln!(
            "player {}: cold solve (preprocess + load) {:.2}s, load saved {:.2}s, warm n {:.1}ms",
            player.id,
            cold.as_secs_f64(),
            load.as_secs_f64(),
            started.elapsed().as_secs_f64() * 1000.0
        );
        let spec = &expected.sig[&100];
        let s: String = (0..100)
            .map(|i| char::from(b'A' + (i % 26) as u8))
            .collect();
        assert_eq!(decipher(&s, spec).map(|d| d.len()), Some(spec.len()));
    }
}

/// The vendored scripts are the release `pins.txt` names (what
/// scripts/ejs-bump.sh keeps in step), and a solver on disk runs only when
/// both of its files are pinned to one release.
#[test]
fn runs_only_pinned_solver_scripts() {
    let vendored = Scripts::vendored();
    assert_ne!(
        vendored.version, "vendored",
        "the vendored solver isn't pinned"
    );
    let dir = std::env::temp_dir().join(format!("ytfast-ejs-{}", std::process::id()));
    let disk = dir.join("cache");
    std::fs::create_dir_all(&disk).expect("scratch dir");
    std::fs::write(disk.join(scripts::LIB), &*vendored.lib).expect("lib");
    std::fs::write(disk.join(scripts::CORE), &*vendored.core).expect("core");
    // The same files pinned as a newer release by a fetched pins file.
    let fetched = dir.join("pins.txt");
    let pin = |file: &str, text: &str| {
        format!("{}  99.0.0  {file}\n", scripts::sha256_hex(text.as_bytes()))
    };
    let pins = pin(scripts::LIB, &vendored.lib) + &pin(scripts::CORE, &vendored.core);
    std::fs::write(&fetched, pins).expect("pins");
    let best = Scripts::best(std::slice::from_ref(&disk), &fetched);
    assert_eq!(best.version, "99.0.0");
    // A changed file is no longer pinned: the vendored copy runs.
    std::fs::write(disk.join(scripts::CORE), format!("{};", vendored.core)).expect("core");
    let best = Scripts::best(std::slice::from_ref(&disk), &fetched);
    assert_eq!(best.version, vendored.version);
    // Nor do pins that aren't the app's or fetched ones.
    std::fs::write(disk.join(scripts::CORE), &*vendored.core).expect("core");
    let best = Scripts::best(std::slice::from_ref(&disk), &dir.join("missing"));
    assert_eq!(best.version, vendored.version);
    let _ = std::fs::remove_dir_all(&dir);
}

fn capture(name: &str) -> Option<String> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("artifacts/resolver")
        .join(name);
    std::fs::read_to_string(path).ok()
}

/// The saved player responses: VISIONOS's plain URLs, the web client's
/// SABR-only formats, WEB_CREATOR's ciphered ones and a bot check.
#[test]
fn picks_audio_from_saved_player_responses() {
    let json = |name: &str| {
        capture(name).map(|t| serde_json::from_str::<serde_json::Value>(&t).expect(name))
    };
    if let Some(visionos) = json("player-visionos-ok-wU26xVT_vBU.json") {
        let format = ytfast::streams::best_audio(&visionos).expect("an audio format");
        assert_eq!(format.itag, 251);
        assert!(format.url.is_some_and(|u| !u.contains("&n=")));
    }
    if let Some(web) = json("player-web-initial-wU26xVT_vBU.json") {
        let error = ytfast::streams::best_audio(&web).unwrap_err().to_string();
        assert!(error.contains("no audio format with a URL"), "{error}");
    }
    if let Some(creator) = json("dump/WEB_CREATOR-qXI87eMP-bs.json") {
        let format = ytfast::streams::best_audio(&creator).expect("an audio format");
        assert_eq!(format.itag, 251);
        assert!(format.url.is_none() && format.cipher.is_some());
    }
    if let Some(bot) = json("player-visionos-wU26xVT_vBU.json") {
        let error = ytfast::streams::best_audio(&bot).unwrap_err().to_string();
        assert!(error.contains("LOGIN_REQUIRED"), "{error}");
    }
}

#[test]
fn reads_player_version_and_timestamp() {
    if let Some(iframe) = capture("iframe_api.js") {
        assert_eq!(
            ytfast::streams::player_id(&iframe).as_deref(),
            Some("1b3be681")
        );
    }
    if let Some(player) = capture("1b3be681.js") {
        assert_eq!(ytfast::streams::signature_timestamp(&player), Some(20728));
    }
}
