//! Resolves a song's audio stream: an InnerTube `player` request as a
//! client that still gets plain URLs, the best audio format the engine
//! plays, and, for clients whose URLs carry them, the player script's
//! signature and `n` challenges solved in the embedded JS engine
//! (`crate::jsc`). What works and what doesn't (PO tokens, SABR), and what
//! happens when YouTube changes its player, is in docs/gpui/RESOLVER.md.

mod tv;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
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

/// How long the TV client is left out after [`TV_STRIKES`] songs in a row
/// failed with it.
const TV_PAUSE: Duration = Duration::from_secs(3600);
const TV_STRIKES: u32 = 3;

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

/// An older TV client (yt-dlp's `tv_downgraded`): signed in only (signed
/// out it answers "The page needs to be reloaded"), needs the JS
/// challenges, no PO token, and gives a Premium account 774 and 141.
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
    /// The session's www.youtube.com config for the TV client, and when it
    /// was read (or failed to be).
    web: tokio::sync::Mutex<Option<(tv::WebConfig, Instant, bool)>>,
    /// The TV client's failures in a row, and until when it is left out.
    tv: Mutex<(u32, Option<Instant>)>,
    /// False once a WEB_CREATOR stream failed to play: without Premium its
    /// URLs need a PO token, so it isn't asked again this session.
    creator_plays: AtomicBool,
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
            web: tokio::sync::Mutex::default(),
            tv: Mutex::default(),
            creator_plays: AtomicBool::new(true),
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
    /// song that needs its challenges doesn't wait for it: signed in, the
    /// version the session's page names.
    pub async fn prepare(&self) -> Result<String> {
        let want = match self.signed_in() {
            true => self.web_config().await.player,
            false => None,
        };
        let current = self.player_for(want.as_deref()).await?;
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

    /// The best audio stream and the client it came from.
    pub async fn resolve(&self, video_id: &str, account: bool) -> Result<(Stream, &'static str)> {
        let (mut streams, client) = self.streams(video_id, account, 1).await?;
        Ok((streams.remove(0), client))
    }

    /// Up to `limit` of the wanted formats, best first, and the client they
    /// came from. With `account`: the TV client with the session (774 and
    /// 141 for Premium, 251 without, no PO token either way), then
    /// WEB_CREATOR (Premium only), then signed out through VISIONOS,
    /// which needs no JS. One `player` request per song while the first
    /// client works.
    pub async fn streams(
        &self,
        video_id: &str,
        account: bool,
        limit: usize,
    ) -> Result<(Vec<Stream>, &'static str)> {
        let mut errors = Vec::new();
        let mut player_wide = false;
        if account && self.tv_open() {
            match self.streams_as(&TV_DOWNGRADED, video_id, true, limit).await {
                Ok(streams) => {
                    self.tv_result(true);
                    return Ok((streams, TV_DOWNGRADED.name));
                }
                Err(error) => {
                    // A player being prepared fails WEB_CREATOR as well.
                    player_wide = error.downcast_ref::<PlayerFailure>().is_some();
                    if !player_wide {
                        self.tv_result(false);
                    }
                    self.log_failure(video_id, &error);
                    errors.push(format!("{}: {error:#}", TV_DOWNGRADED.name));
                }
            }
        }
        if account && !player_wide && self.creator_plays.load(Ordering::Relaxed) {
            match self.streams_as(&WEB_CREATOR, video_id, true, limit).await {
                Ok(streams) => return Ok((streams, WEB_CREATOR.name)),
                Err(error) => {
                    self.log_failure(video_id, &error);
                    errors.push(format!("{}: {error:#}", WEB_CREATOR.name));
                }
            }
        }
        match self.streams_as(&VISIONOS, video_id, false, limit).await {
            Ok(streams) => Ok((streams, VISIONOS.name)),
            Err(error) => {
                self.log_failure(video_id, &error);
                errors.push(format!("{}: {error:#}", VISIONOS.name));
                bail!("{}", errors.join("; "))
            }
        }
    }

    /// A stream from `url` failed to play. A WEB_CREATOR one means the
    /// account has no Premium (its URLs then need a PO token), so that
    /// client isn't asked again this session.
    pub fn stream_failed(&self, url: &str) {
        if url_param(url, "c").as_deref() == Some(WEB_CREATOR.name)
            && self.creator_plays.swap(false, Ordering::Relaxed)
        {
            log::warn!("WEB_CREATOR streams don't play for this account; not asking it again");
        }
    }

    /// Whether the TV client is asked (it isn't for [`TV_PAUSE`] after
    /// [`TV_STRIKES`] songs in a row failed with it).
    fn tv_open(&self) -> bool {
        let state = self.tv.lock().expect("tv lock");
        state.1.is_none_or(|until| Instant::now() >= until)
    }

    fn tv_result(&self, ok: bool) {
        let mut state = self.tv.lock().expect("tv lock");
        if ok {
            *state = (0, None);
            return;
        }
        state.0 += 1;
        if state.0 >= TV_STRIKES {
            *state = (0, Some(Instant::now() + TV_PAUSE));
            log::warn!(
                "the TV client failed {TV_STRIKES} songs in a row; leaving it out for {} min",
                TV_PAUSE.as_secs() / 60
            );
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
        // The TV client is asked as yt-dlp asks it, with the session's own
        // page config (its player version first of all).
        let web = match authed && client.name == TV_DOWNGRADED.name {
            true => Some(self.web_config().await),
            false => None,
        };
        let want = web.as_ref().and_then(|w| w.player.clone());
        let mut current = if needs_js {
            Some(self.player_for(want.as_deref()).await?)
        } else {
            None
        };
        let sts = current.as_ref().map(|c| c.sts);
        let response = match &web {
            Some(web) => self.player_tv(client, video_id, sts, web).await?,
            None => self
                .innertube
                .player_as(client, video_id, sts, authed)
                .await
                .map_err(|e| anyhow!("{e}"))?,
        };
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
                        None => current.insert(self.player_for(None).await?).clone(),
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
        self.player_for(None).await
    }

    /// The player script: `want` (the version the session's page names)
    /// or else the current one, downloaded once per version.
    async fn player_for(&self, want: Option<&str>) -> Result<Current> {
        let mut current = self.current.lock().await;
        if let Some(known) = current
            .as_ref()
            .filter(|c| c.checked.elapsed() < PLAYER_CHECK && want.is_none_or(|w| w == c.player.id))
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
        let saved = std::fs::read_to_string(&marker)
            .ok()
            .map(|id| id.trim().to_owned());
        let id = match (want, saved.filter(|_| fresh)) {
            (Some(want), saved) if saved.as_deref() != Some(want) => {
                log::info!("the session's page names player {want}");
                crate::paths::write_atomic(&marker, want.as_bytes())
                    .context("saving the player id")?;
                want.to_owned()
            }
            (_, Some(id)) => id,
            (_, None) => {
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

    /// The session's www.youtube.com config (`tv::PAGE`), read at most
    /// every [`PLAYER_CHECK`] (after a failure, every ten minutes;
    /// meanwhile the TV client is asked without it).
    async fn web_config(&self) -> tv::WebConfig {
        let mut web = self.web.lock().await;
        if let Some((config, read, ok)) = web.as_ref() {
            let keep = if *ok {
                PLAYER_CHECK
            } else {
                Duration::from_secs(600)
            };
            if read.elapsed() < keep {
                return config.clone();
            }
        }
        let read = self.read_web_config().await;
        if let Err(error) = &read {
            log::warn!("reading the session's page config failed: {error:#}");
        }
        let ok = read.is_ok();
        let config = read.unwrap_or_default();
        *web = Some((config.clone(), Instant::now(), ok));
        config
    }

    async fn read_web_config(&self) -> Result<tv::WebConfig> {
        let mut request = self
            .innertube
            .http()
            .get(tv::PAGE)
            .header("User-Agent", WEB_AGENT)
            .header("Accept-Language", "en-us,en;q=0.5");
        if let Some(cookies) = self.innertube.cookie_header() {
            request = request.header("Cookie", cookies);
        }
        let page = request
            .send()
            .await
            .context("asking for the session's page")?
            .error_for_status()?
            .text()
            .await?;
        let config = tv::parse_ytcfg(&page);
        if config.player.is_none() && config.visitor.is_none() {
            bail!("the page has no config");
        }
        Ok(config)
    }

    /// A `player` request as `client` with the session, as yt-dlp sends it
    /// (`tv::player_body`, `tv::player_headers`).
    async fn player_tv(
        &self,
        client: &PlayerClient,
        video_id: &str,
        sts: Option<u32>,
        web: &tv::WebConfig,
    ) -> Result<Value> {
        let it = &self.innertube;
        let mut web = web.clone();
        if web.visitor.is_none() {
            web.visitor = it.visitor_data();
        }
        // The app decides which channel to act as, not the browser's page.
        web.delegated_session = it.page_id();
        let sids = tv::Sids {
            sapisid: it
                .cookie("SAPISID")
                .or_else(|| it.cookie("__Secure-3PAPISID")),
            one_p: it.cookie("__Secure-1PAPISID"),
            three_p: it.cookie("__Secure-3PAPISID"),
        };
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let authorization = tv::sid_authorization(&sids, tv::WWW, web.user_session.as_deref(), now);
        let body = tv::player_body(client, video_id, sts);
        let mut request = it
            .http()
            .post(format!("{}/youtubei/v1/player?prettyPrint=false", tv::WWW))
            .body(body.to_string());
        for (name, value) in tv::player_headers(client, &web, authorization) {
            request = request.header(name, value);
        }
        if let Some(cookies) = it.cookie_header() {
            request = request.header("Cookie", cookies);
        }
        let response = request.send().await.context("asking for the streams")?;
        let status = response.status();
        if !status.is_success() {
            bail!("YouTube answered the player request with HTTP {status}");
        }
        response.json().await.context("reading the player response")
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
            && f.get("drmFamilies").is_none()
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

    /// Premium's formats come first: 774 (Opus ~256 kbps), then 141 (AAC
    /// 256 kbps), then 251, whatever order the response lists them in.
    #[test]
    fn premium_formats_first() {
        let picked = audio_formats(&response(&[249, 251, 141, 140, 774])).expect("formats");
        let itags: Vec<u32> = picked.iter().map(|f| f.itag).collect();
        assert_eq!(itags, [774, 141, 251, 140, 249]);
    }

    /// DRC copies and DRM-protected formats (a TV experiment) are skipped.
    #[test]
    fn skips_drc_and_drm() {
        let response = json!({
            "playabilityStatus": {"status": "OK"},
            "streamingData": {"adaptiveFormats": [
                {"itag": 774, "url": "https://example.invalid/a", "drmFamilies": ["WIDEVINE"]},
                {"itag": 141, "url": "https://example.invalid/b", "isDrc": true},
                {"itag": 141, "signatureCipher": "s=x&url=https%3A%2F%2Fexample.invalid%2Fc"},
            ]},
        });
        let picked = audio_formats(&response).expect("formats");
        assert_eq!(picked.len(), 1);
        assert_eq!(picked[0].itag, 141);
        assert!(picked[0].cipher.is_some());
    }

    fn native() -> Native {
        let dir = std::env::temp_dir().join("ytfast-streams-test");
        Native::new(Arc::new(Client::new()), &dir, &dir)
    }

    /// A WEB_CREATOR stream that fails to play means no Premium: that
    /// client is left out for the session. Other clients' failures don't.
    #[test]
    fn creator_left_out_after_its_stream_fails() {
        let native = native();
        native.stream_failed("https://example.invalid/videoplayback?c=TVHTML5&n=x");
        assert!(native.creator_plays.load(Ordering::Relaxed));
        native.stream_failed("https://example.invalid/videoplayback?c=WEB_CREATOR&n=x");
        assert!(!native.creator_plays.load(Ordering::Relaxed));
    }

    /// The TV client is left out after three songs in a row failed with
    /// it; a success in between resets the count.
    #[test]
    fn tv_left_out_after_three_failures() {
        let native = native();
        native.tv_result(false);
        native.tv_result(false);
        native.tv_result(true);
        native.tv_result(false);
        native.tv_result(false);
        assert!(native.tv_open());
        native.tv_result(false);
        assert!(!native.tv_open());
    }
}
