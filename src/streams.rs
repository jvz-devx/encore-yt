//! Resolves a song's audio stream without yt-dlp: an InnerTube `player`
//! request as a client that still gets plain URLs, the best audio format,
//! and, for clients whose URLs carry them, the player script's signature
//! and `n` challenges solved in the embedded JS engine (`crate::jsc`).
//!
//! Opt-in with `YTFAST_RESOLVER=rust`; `crate::resolver` falls back to
//! yt-dlp on any failure. What works and what doesn't (PO tokens, SABR) is
//! in docs/gpui/RESOLVER.md.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow, bail};
use serde_json::Value;

use crate::innertube::{Client, PlayerClient, Stream};
use crate::jsc::{Challenges, Player, Solver, decipher};

/// The audio formats wanted, best first (the same list yt-dlp gets).
const ITAGS: [u64; 7] = [774, 141, 251, 140, 250, 249, 139];

/// How long a player version is trusted before asking which is current.
const PLAYER_CHECK: Duration = Duration::from_secs(6 * 3600);

const WEB_AGENT: &str = "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/140.0.0.0 Safari/537.36";

/// Plain URLs, no challenges and no PO token, signed out (yt-dlp's
/// signed-out default since 2026-07).
pub const VISIONOS: PlayerClient = PlayerClient {
    name: "VISIONOS",
    version: "1.02",
    number: 101,
    user_agent: Some(
        "Mozilla/5.0 (Macintosh; Intel Mac OS X 15_7_3) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/26.0 Safari/605.1.15",
    ),
    extra: &[
        ("deviceMake", "Apple"),
        ("deviceModel", "RealityDevice17,1"),
        ("osName", "visionOS"),
        ("osVersion", "26.5.23O471"),
    ],
};

/// An older TV client: takes cookies, needs the JS challenges, no PO token.
pub const TV_DOWNGRADED: PlayerClient = PlayerClient {
    name: "TVHTML5",
    version: "5.20260707",
    number: 7,
    user_agent: Some("Mozilla/5.0 (ChromiumStylePlatform) Cobalt/Version"),
    extra: &[],
};

/// Studio's web client: signed in only; Premium needs no PO token.
pub const WEB_CREATOR: PlayerClient = PlayerClient {
    name: "WEB_CREATOR",
    version: "1.20260708.06.00",
    number: 62,
    user_agent: Some(WEB_AGENT),
    extra: &[],
};

/// Whether `YTFAST_RESOLVER=rust` asks for this resolver.
pub fn enabled() -> bool {
    std::env::var("YTFAST_RESOLVER").is_ok_and(|v| v == "rust")
}

/// The current player script, as last checked.
#[derive(Clone)]
struct Current {
    player: Player,
    sts: u32,
    checked: Instant,
}

pub struct Native {
    innertube: Arc<Client>,
    /// `<cache>/player`: `<id>.js`, `<id>.ejs.js`, and `current` (the id).
    dir: PathBuf,
    solver: Solver,
    current: tokio::sync::Mutex<Option<Current>>,
    /// The last player response's client, for the log.
    last: Mutex<&'static str>,
    /// Where to save each player response (for the live check example).
    dump: Option<PathBuf>,
}

impl Native {
    pub fn new(innertube: Arc<Client>, cache: &Path) -> Self {
        Self {
            innertube,
            dir: cache.join("player"),
            solver: Solver::new(),
            current: tokio::sync::Mutex::default(),
            last: Mutex::new(""),
            dump: None,
        }
    }

    /// Saves every player response to `dir` as `<client>-<video id>.json`.
    pub fn dump_responses(&mut self, dir: PathBuf) {
        self.dump = Some(dir);
    }

    /// The client the last stream came from.
    pub fn last_client(&self) -> &'static str {
        *self.last.lock().expect("last lock")
    }

    /// Downloads and prepares the current player script now, so the first
    /// song that needs its challenges doesn't fall back to yt-dlp.
    pub async fn prepare(&self) -> Result<String> {
        let current = self.player().await?;
        self.solver.load(&current.player).await?;
        Ok(current.player.id)
    }

    /// The best audio stream: signed out through VISIONOS, signed in
    /// through WEB_CREATOR with the session's cookies (Premium needs no PO
    /// token there; other accounts' URLs fail and yt-dlp takes over). The
    /// TV client answered "The page needs to be reloaded" on 2026-10-07
    /// both ways, so it isn't asked.
    pub async fn resolve(&self, video_id: &str, signed_in: bool) -> Result<Stream> {
        let clients: &[&PlayerClient] = if signed_in {
            &[&WEB_CREATOR]
        } else {
            &[&VISIONOS]
        };
        let mut errors = Vec::new();
        for client in clients {
            match self.resolve_as(client, video_id, signed_in).await {
                Ok(stream) => {
                    *self.last.lock().expect("last lock") = client.name;
                    return Ok(stream);
                }
                Err(error) => errors.push(format!("{}: {error:#}", client.name)),
            }
        }
        bail!("{}", errors.join("; "))
    }

    /// One client's best stream.
    pub async fn resolve_as(
        &self,
        client: &PlayerClient,
        video_id: &str,
        authed: bool,
    ) -> Result<Stream> {
        let needs_js = client.name != VISIONOS.name;
        let current = if needs_js {
            Some(self.player().await?)
        } else {
            None
        };
        let response = self
            .innertube
            .player_as(client, video_id, current.as_ref().map(|c| c.sts), authed)
            .await
            .map_err(|e| anyhow!("{e}"))?;
        if let Some(dir) = &self.dump {
            let path = dir.join(format!("{}-{video_id}.json", client.name));
            let _ = std::fs::write(path, response.to_string());
        }
        let format = best_audio(&response)?;
        let url = match (format.url, format.cipher) {
            (Some(url), _) if !needs_challenges(&url) => url,
            (url, cipher) => {
                let current = match current {
                    Some(current) => current,
                    None => self.player().await?,
                };
                self.solve(&current.player, url, cipher).await?
            }
        };
        Ok(Stream {
            itag: format.itag,
            expires: crate::innertube::expiry(&url),
            url,
            user_agent: None,
        })
    }

    /// Deciphers the signature and transforms `n` into a playable URL.
    async fn solve(
        &self,
        player: &Player,
        url: Option<String>,
        cipher: Option<String>,
    ) -> Result<String> {
        let (mut url, signature) = match (url, cipher) {
            (Some(url), _) => (url, None),
            (None, Some(cipher)) => {
                let fields = query(&cipher);
                let url = field(&fields, "url").context("the cipher has no URL")?;
                let s = field(&fields, "s").context("the cipher has no signature")?;
                let sp = field(&fields, "sp").unwrap_or_else(|| "signature".into());
                (url, Some((s, sp)))
            }
            (None, None) => bail!("the format has no URL"),
        };
        if !self.solver.prepared(player) {
            self.solver.warm(player);
            bail!("player {} is being prepared", player.id);
        }
        let n = url_param(&url, "n");
        let challenges = Challenges {
            n: n.iter().cloned().collect(),
            sig_lengths: signature.iter().map(|(s, _)| s.chars().count()).collect(),
        };
        let solved = self.solver.solve(player, challenges).await?;
        if let Some((s, sp)) = signature {
            let spec = solved
                .sig
                .get(&s.chars().count())
                .context("no signature answer")?;
            let signature = decipher(&s, spec).context("the signature answer doesn't fit")?;
            url = set_param(&url, &sp, &signature)?;
        }
        if let Some(n) = n {
            let answer = solved.n.get(&n).context("no n answer")?;
            url = set_param(&url, "n", answer)?;
        }
        Ok(url)
    }

    /// The current player script, downloaded once per version.
    async fn player(&self) -> Result<Current> {
        let mut current = self.current.lock().await;
        if let Some(known) = current
            .as_ref()
            .filter(|c| c.checked.elapsed() < PLAYER_CHECK)
        {
            return Ok(known.clone());
        }
        std::fs::create_dir_all(&self.dir).context("creating the player cache")?;
        let marker = self.dir.join("current");
        let fresh = std::fs::metadata(&marker)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.elapsed().ok())
            .is_some_and(|age| age < PLAYER_CHECK);
        let saved = std::fs::read_to_string(&marker).ok();
        let id = match saved.filter(|_| fresh) {
            Some(id) => id.trim().to_owned(),
            None => {
                let id = self.player_version().await?;
                crate::paths::write_atomic(&marker, id.as_bytes())
                    .context("saving the player id")?;
                id
            }
        };
        let path = self.dir.join(format!("{id}.js"));
        if !path.exists() {
            self.download_player(&id, &path).await?;
            self.prune(&id);
        }
        let source = std::fs::read_to_string(&path).context("reading the player")?;
        let sts = signature_timestamp(&source).context("the player has no signature timestamp")?;
        let known = Current {
            player: Player { id, path },
            sts,
            checked: Instant::now(),
        };
        *current = Some(known.clone());
        Ok(known)
    }

    /// The current player version, from the iframe API script.
    async fn player_version(&self) -> Result<String> {
        let text = self
            .innertube
            .http()
            .get("https://www.youtube.com/iframe_api")
            .header("User-Agent", WEB_AGENT)
            .send()
            .await
            .context("asking for the player version")?
            .error_for_status()?
            .text()
            .await?;
        player_id(&text).context("the iframe API names no player")
    }

    async fn download_player(&self, id: &str, path: &Path) -> Result<()> {
        let url = format!("https://www.youtube.com/s/player/{id}/player_ias.vflset/en_US/base.js");
        let started = Instant::now();
        let bytes = self
            .innertube
            .http()
            .get(&url)
            .header("User-Agent", WEB_AGENT)
            .send()
            .await
            .context("downloading the player")?
            .error_for_status()?
            .bytes()
            .await?;
        crate::paths::write_atomic(path, &bytes).context("saving the player")?;
        log::info!(
            "downloaded player {id} ({} KB) in {:.1}s",
            bytes.len() / 1024,
            started.elapsed().as_secs_f64()
        );
        Ok(())
    }

    /// Keeps only the current player's files.
    fn prune(&self, id: &str) {
        let Ok(entries) = std::fs::read_dir(&self.dir) else {
            return;
        };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.ends_with(".js") && !name.starts_with(&format!("{id}.")) {
                let _ = std::fs::remove_file(entry.path());
            }
        }
    }
}

/// An audio format from a player response.
#[derive(Debug, PartialEq)]
pub struct Format {
    pub itag: u32,
    pub url: Option<String>,
    pub cipher: Option<String>,
}

/// The best wanted audio format of a playable response.
pub fn best_audio(response: &Value) -> Result<Format> {
    let status = crate::parse::at(response, &["playabilityStatus", "status"])
        .and_then(Value::as_str)
        .unwrap_or("none");
    if status != "OK" {
        let reason = crate::parse::at(response, &["playabilityStatus", "reason"])
            .and_then(Value::as_str)
            .unwrap_or("");
        bail!("not playable ({status}): {reason}");
    }
    let formats = crate::parse::at(response, &["streamingData", "adaptiveFormats"])
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default();
    let usable = |f: &&Value| {
        // Dynamic-range-compressed copies share the itag; skip them.
        f.get("isDrc").and_then(Value::as_bool) != Some(true)
            && (f.get("url").is_some() || f.get("signatureCipher").is_some())
    };
    let chosen = ITAGS
        .iter()
        .find_map(|itag| {
            formats
                .iter()
                .filter(usable)
                .find(|f| f.get("itag").and_then(Value::as_u64) == Some(*itag))
        })
        .context(if formats.is_empty() {
            "no formats (SABR only?)"
        } else {
            "no audio format with a URL"
        })?;
    let text = |key: &str| chosen.get(key).and_then(Value::as_str).map(str::to_owned);
    Ok(Format {
        itag: chosen.get("itag").and_then(Value::as_u64).unwrap_or(0) as u32,
        url: text("url"),
        cipher: text("signatureCipher"),
    })
}

/// Whether a direct URL still carries an `n` challenge.
fn needs_challenges(url: &str) -> bool {
    url_param(url, "n").is_some()
}

/// The player id in the iframe API script (`player\/1b3be681\/`).
pub fn player_id(text: &str) -> Option<String> {
    text.split("player").skip(1).find_map(|rest| {
        let rest = rest.trim_start_matches(['\\', '/']);
        let id: String = rest.chars().take_while(char::is_ascii_hexdigit).collect();
        let after = rest[id.len()..].trim_start_matches('\\');
        (id.len() == 8 && after.starts_with('/')).then_some(id)
    })
}

/// The player's `signatureTimestamp`, which tells InnerTube which cipher
/// the URLs must be encrypted for.
pub fn signature_timestamp(source: &str) -> Option<u32> {
    source.split("signatureTimestamp").skip(1).find_map(|rest| {
        let rest = rest.trim_start_matches([' ', ':']);
        let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
        (digits.len() == 5).then(|| digits.parse().ok()).flatten()
    })
}

fn query(text: &str) -> Vec<(String, String)> {
    reqwest::Url::parse(&format!("http://q/?{text}"))
        .map(|u| u.query_pairs().into_owned().collect())
        .unwrap_or_default()
}

fn field(fields: &[(String, String)], name: &str) -> Option<String> {
    fields
        .iter()
        .find(|(k, _)| k == name)
        .map(|(_, v)| v.clone())
}

fn url_param(url: &str, name: &str) -> Option<String> {
    reqwest::Url::parse(url)
        .ok()?
        .query_pairs()
        .find(|(k, _)| k == name)
        .map(|(_, v)| v.into_owned())
}

/// Sets one query parameter, keeping the others in order.
fn set_param(url: &str, name: &str, value: &str) -> Result<String> {
    let mut parsed = reqwest::Url::parse(url).context("a bad stream URL")?;
    let mut pairs: Vec<(String, String)> = parsed.query_pairs().into_owned().collect();
    match pairs.iter_mut().find(|(k, _)| k == name) {
        Some(pair) => pair.1 = value.to_owned(),
        None => pairs.push((name.to_owned(), value.to_owned())),
    }
    parsed.query_pairs_mut().clear().extend_pairs(pairs);
    Ok(parsed.into())
}
