//! Checks the embedded JS challenge solver against yt-dlp's, offline. The
//! player script is YouTube's, so it isn't committed: capture one under
//! `artifacts/resolver/` (`<id>.js`, see docs/gpui/RESOLVER.md) and write the
//! answers yt-dlp's EJS gives in deno with `scripts/ejs-expected.sh`. Without
//! a capture the test says so and passes.

use std::collections::HashMap;
use std::path::PathBuf;

use serde::Deserialize;
use ytfast::jsc::{Challenges, Engine, Player, decipher};

#[derive(Deserialize)]
struct Expected {
    n: HashMap<String, String>,
    sig: HashMap<usize, Vec<usize>>,
}

fn captures() -> Vec<(Player, Expected)> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("artifacts/resolver");
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
        let preprocessed = player.path.with_extension("ejs.js");
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
