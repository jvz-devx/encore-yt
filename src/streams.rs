//! Resolves a song's audio stream: an InnerTube `player` request as a
//! client that still gets plain URLs, the best audio format the engine
//! plays, and, for clients whose URLs carry them, the player script's
//! signature and `n` challenges solved in the embedded JS engine
//! (`crate::jsc`). What works and what doesn't (PO tokens, SABR), and what
//! happens when YouTube changes its player, is in docs/gpui/RESOLVER.md.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow, bail};
use serde_json::Value;

use crate::innertube::{Client, PlayerClient, Stream};
use crate::jsc::{Challenges, Player, Solver, Status, decipher, scripts};

/// The audio formats wanted, best first: Opus (WebM) and AAC-LC (MP4),
/// which the engine decodes. Never HE-AAC (139, 599): it has no decoder
/// for it.
const ITAGS: [u64; 7] = [774, 141, 251, 140, 250, 249, 600];

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

/// The pins file on the repository's default branch: the list of solver
/// releases (by SHA-256) the app may download when its own solver can't
/// use a player. Changing it takes a reviewed commit to `main`.
const PINS_URL: &str =
    "https://raw.githubusercontent.com/jvz-devx/ytfast-gpui/main/src/jsc/pins.txt";

/// Where yt-dlp-ejs publishes its release assets.
const EJS_RELEASES: &str = "https://github.com/yt-dlp/ejs/releases/download";

/// A failure that holds for every song until the player version or the
/// solver changes: logged once per player version.
#[derive(Debug)]
pub struct PlayerFailure {
    pub player: String,
    pub preparing: bool,
    pub reason: String,
}

impl std::fmt::Display for PlayerFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.preparing {
            true => write!(f, "player {} is being prepared", self.player),
            false => write!(
                f,
                "the solver can't use player {}: {}",
                self.player, self.reason
            ),
        }
    }
}

impl std::error::Error for PlayerFailure {}

/// The current player script, as last checked.
#[derive(Clone)]
struct Current {
    player: Player,
    sts: u32,
    checked: Instant,
}

pub struct Native {
    innertube: Arc<Client>,
    /// `<cache>/player`: `<id>.js`, `<id>.ejs-<solver>.js`, and `current`
    /// (the id).
    dir: PathBuf,
    /// `<cache>/ejs`: a downloaded solver release and the fetched pins.
    ejs: PathBuf,
    solver: Arc<Solver>,
    /// Players a newer solver was looked for, at most once each.
    refreshed: Mutex<std::collections::HashSet<String>>,
    current: tokio::sync::Mutex<Option<Current>>,
    /// Player-wide failures already logged, so each is logged once per
    /// player version instead of once per song.
    logged: Mutex<std::collections::HashSet<String>>,
    /// Where to save each player response (for the live check example).
    dump: Option<PathBuf>,
}

impl Native {
    /// The solver is the newest pinned release among the vendored copy,
    /// `<cache>/ejs/` and `<config>/ejs/`.
    pub fn new(innertube: Arc<Client>, cache: &Path, config: &Path) -> Self {
        let ejs = cache.join(scripts::DIR);
        let dirs = [ejs.clone(), config.join(scripts::DIR)];
        let solver =
            Solver::with_scripts(scripts::Scripts::best(&dirs, &ejs.join(scripts::PINS_FILE)));
        Self {
            innertube,
            dir: cache.join("player"),
            ejs,
            solver: Arc::new(solver),
            refreshed: Mutex::default(),
            current: tokio::sync::Mutex::default(),
            logged: Mutex::default(),
            dump: None,
        }
    }

    /// Whether the InnerTube session is signed in.
    pub fn signed_in(&self) -> bool {
        self.innertube.signed_in()
    }

    /// Saves every player response to `dir` as `<client>-<video id>.json`.
    pub fn dump_responses(&mut self, dir: PathBuf) {
        self.dump = Some(dir);
    }

    /// Downloads and prepares the current player script now, so the first
    /// song that needs its challenges doesn't wait for it.
    pub async fn prepare(&self) -> Result<String> {
        let current = self.player().await?;
        self.solver.load(&current.player).await?;
        Ok(current.player.id)
    }

    /// The current player script on disk (downloaded if needed).
    pub async fn player_path(&self) -> Result<PathBuf> {
        Ok(self.player().await?.player.path)
    }

    /// The solver release in use.
    pub fn solver_version(&self) -> String {
        self.solver.version()
    }

    /// The best audio stream and the client it came from: with `account`
    /// through WEB_CREATOR with the session's cookies (Premium needs no PO
    /// token there), else, or when that fails, signed out through VISIONOS,
    /// which needs no JS. The TV client answered "The page needs to be
    /// reloaded" on 2026-10-07 both ways, so it isn't asked.
    pub async fn resolve(&self, video_id: &str, account: bool) -> Result<(Stream, &'static str)> {
        let mut errors = Vec::new();
        if account {
            match self.resolve_as(&WEB_CREATOR, video_id, true).await {
                Ok(stream) => return Ok((stream, WEB_CREATOR.name)),
                Err(error) => {
                    self.log_failure(video_id, &error);
                    errors.push(format!("{}: {error:#}", WEB_CREATOR.name));
                }
            }
        }
        match self.resolve_as(&VISIONOS, video_id, false).await {
            Ok(stream) => Ok((stream, VISIONOS.name)),
            Err(error) => {
                self.log_failure(video_id, &error);
                errors.push(format!("{}: {error:#}", VISIONOS.name));
                bail!("{}", errors.join("; "))
            }
        }
    }

    /// A failure of one song is logged for that song; one that holds for
    /// the whole player version (the solver can't use it, or it is being
    /// prepared) once per version and kind.
    fn log_failure(&self, video_id: &str, error: &anyhow::Error) {
        let Some(failure) = error.downcast_ref::<PlayerFailure>() else {
            log::warn!("resolving {video_id} failed: {error:#}");
            return;
        };
        let key = format!("{}:{}", failure.player, failure.preparing);
        let first = {
            let mut logged = self.logged.lock().expect("logged lock");
            if logged.len() > 64 {
                logged.clear();
            }
            logged.insert(key)
        };
        if first {
            log::warn!("{failure}; songs play signed out until that changes");
        } else {
            log::debug!("{video_id}: {failure}");
        }
    }

    /// One client's best stream.
    pub async fn resolve_as(
        &self,
        client: &PlayerClient,
        video_id: &str,
        authed: bool,
    ) -> Result<Stream> {
        let mut streams = self.streams_as(client, video_id, authed, 1).await?;
        Ok(streams.remove(0))
    }

    /// Up to `limit` of one client's wanted audio formats, best first,
    /// from one player request (`examples/stream_check.rs` plays each).
    pub async fn streams_as(
        &self,
        client: &PlayerClient,
        video_id: &str,
        authed: bool,
        limit: usize,
    ) -> Result<Vec<Stream>> {
        let needs_js = client.name != VISIONOS.name;
        let mut current = if needs_js {
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
        let mut streams = Vec::new();
        for format in audio_formats(&response)?.into_iter().take(limit) {
            let url = match (format.url, format.cipher) {
                (Some(url), _) if !needs_challenges(&url) => url,
                (url, cipher) => {
                    let current = match &current {
                        Some(current) => current.clone(),
                        None => current.insert(self.player().await?).clone(),
                    };
                    self.solve(&current.player, url, cipher).await?
                }
            };
            streams.push(Stream {
                itag: format.itag,
                expires: crate::innertube::expiry(&url),
                url,
                user_agent: None,
            });
        }
        Ok(streams)
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
        match self.solver.status(player) {
            Status::Ready => {}
            Status::Cold => {
                self.solver.warm(player);
                return Err(PlayerFailure {
                    player: player.id.clone(),
                    preparing: true,
                    reason: String::new(),
                }
                .into());
            }
            Status::Broken(reason) => {
                self.refresh_solver(&player.id);
                return Err(PlayerFailure {
                    player: player.id.clone(),
                    preparing: false,
                    reason,
                }
                .into());
            }
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

    /// Looks for a newer pinned solver release once per broken player, in
    /// the background: the pins on the repository's `main` name the
    /// releases, the files come from yt-dlp-ejs's GitHub release, and both
    /// must match their pinned SHA-256 before they are saved or run.
    fn refresh_solver(&self, player: &str) {
        let first = self
            .refreshed
            .lock()
            .expect("refresh lock")
            .insert(player.to_owned());
        if !first {
            return;
        }
        let (http, dir, solver) = (
            self.innertube.http().clone(),
            self.ejs.clone(),
            self.solver.clone(),
        );
        tokio::spawn(async move {
            match fetch_solver(&http, &dir, &solver.version()).await {
                Ok(Some(found)) => solver.set_scripts(found),
                Ok(None) => log::info!("no newer EJS solver is pinned"),
                Err(error) => log::warn!("couldn't update the EJS solver: {error:#}"),
            }
        });
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

/// Fetches the repository's pins and, if they name a release newer than
/// `current`, downloads and checks its two files and saves them with the
/// pins under `dir`.
async fn fetch_solver(
    http: &reqwest::Client,
    dir: &Path,
    current: &str,
) -> Result<Option<scripts::Scripts>> {
    let get = |url: String| async move {
        http.get(&url)
            .send()
            .await
            .and_then(reqwest::Response::error_for_status)
            .with_context(|| format!("fetching {url}"))?
            .text()
            .await
            .with_context(|| format!("reading {url}"))
    };
    let fetched = get(PINS_URL.to_owned()).await?;
    let mut pins = scripts::parse_pins(scripts::PINS);
    pins.extend(scripts::parse_pins(&fetched));
    let Some(version) = scripts::newest_pinned(&pins, current) else {
        return Ok(None);
    };
    let mut files = Vec::new();
    for name in [scripts::LIB, scripts::CORE] {
        let text = get(format!("{EJS_RELEASES}/{version}/{name}")).await?;
        let want = scripts::pinned_hash(&pins, &version, name)?;
        if scripts::sha256_hex(text.as_bytes()) != want {
            bail!("{name} of EJS {version} doesn't match its pinned hash");
        }
        files.push(text);
    }
    let core = files.pop().expect("two files");
    let lib = files.pop().expect("two files");
    let found = scripts::Scripts::verified(lib, core, &pins)?;
    std::fs::create_dir_all(dir).context("creating the solver directory")?;
    for (name, text) in [
        (scripts::LIB, &*found.lib),
        (scripts::CORE, &*found.core),
        (scripts::PINS_FILE, fetched.as_str()),
    ] {
        crate::paths::write_atomic(&dir.join(name), text.as_bytes())
            .with_context(|| format!("saving {name}"))?;
    }
    log::info!("downloaded EJS solver {version}");
    Ok(Some(found))
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
    Ok(audio_formats(response)?.remove(0))
}

/// The wanted audio formats of a playable response, best first; at least one.
pub fn audio_formats(response: &Value) -> Result<Vec<Format>> {
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
    let chosen: Vec<&Value> = ITAGS
        .iter()
        .filter_map(|itag| {
            formats
                .iter()
                .filter(usable)
                .find(|f| f.get("itag").and_then(Value::as_u64) == Some(*itag))
        })
        .collect();
    if chosen.is_empty() {
        bail!(if formats.is_empty() {
            "no formats (SABR only?)"
        } else {
            "no audio format with a URL that Music plays"
        });
    }
    Ok(chosen
        .into_iter()
        .map(|f| {
            let text = |key: &str| f.get(key).and_then(Value::as_str).map(str::to_owned);
            Format {
                itag: f.get("itag").and_then(Value::as_u64).unwrap_or(0) as u32,
                url: text("url"),
                cipher: text("signatureCipher"),
            }
        })
        .collect())
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn response(itags: &[u64]) -> Value {
        let formats: Vec<Value> = itags
            .iter()
            .map(|itag| json!({"itag": itag, "url": format!("https://example.invalid/{itag}")}))
            .collect();
        json!({
            "playabilityStatus": {"status": "OK"},
            "streamingData": {"adaptiveFormats": formats},
        })
    }

    /// HE-AAC (139, 599) has no decoder in the engine: never picked, even
    /// when it is all there is.
    #[test]
    fn never_picks_he_aac() {
        let picked = audio_formats(&response(&[139, 599, 140, 249])).expect("formats");
        let itags: Vec<u32> = picked.iter().map(|f| f.itag).collect();
        assert_eq!(itags, [140, 249]);
        assert!(audio_formats(&response(&[139, 599])).is_err());
    }
}
