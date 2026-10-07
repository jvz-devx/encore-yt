//! Resolves a song's audio stream: an InnerTube `player` request as a
//! client that still gets plain URLs, the best audio format the engine
//! plays, and, for clients whose URLs carry them, the player script's
//! signature and `n` challenges solved in the embedded JS engine
//! (`crate::jsc`). What works and what doesn't (PO tokens, SABR), and what
//! happens when YouTube changes its player, is in docs/gpui/RESOLVER.md.

mod tv;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU8, Ordering};
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

/// Premium's own formats: a response with one of them proves Premium.
const PREMIUM_ITAGS: [u32; 2] = [774, 141];

/// The account clients, best first: both need Premium for URLs without a
/// PO token.
const ACCOUNT_CLIENTS: [&PlayerClient; 2] = [&WEB_REMIX, &WEB_CREATOR];

/// What the session's account is known to be.
const UNKNOWN: u8 = 0;
const PREMIUM: u8 = 1;
const NOT_PREMIUM: u8 = 2;

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
    host: tv::WWW,
};

/// An older TV client (yt-dlp's `tv_downgraded`): no PO token, the JS
/// challenges of the TV player variant. Not asked by the app (see
/// `Native::tv_sts`); `examples/resolve_rust.rs --client tv` asks it.
pub const TV_DOWNGRADED: PlayerClient = PlayerClient {
    name: "TVHTML5",
    version: "5.20260707",
    number: 7,
    user_agent: Some("Mozilla/5.0 (ChromiumStylePlatform) Cobalt/Version"),
    extra: &[],
    host: tv::WWW,
};

/// YouTube Music's web client (yt-dlp's `web_music`) on
/// music.youtube.com: with a Premium session its URLs need no PO token,
/// and it offers 774 and 141. Without Premium they need one.
pub const WEB_REMIX: PlayerClient = PlayerClient {
    name: "WEB_REMIX",
    version: "1.20260707.12.00",
    number: 67,
    user_agent: Some(WEB_AGENT),
    extra: &[],
    host: "https://music.youtube.com",
};

/// Studio's web client: signed in only; Premium needs no PO token.
pub const WEB_CREATOR: PlayerClient = PlayerClient {
    name: "WEB_CREATOR",
    version: "1.20260708.06.00",
    number: 62,
    user_agent: Some(WEB_AGENT),
    extra: &[],
    host: tv::WWW,
};

/// The pins file on the repository's default branch: the list of solver
/// releases (by SHA-256) the app may download when its own solver can't
/// use a player. Changing it takes a reviewed commit to `main`.
const PINS_URL: &str =
    "https://raw.githubusercontent.com/jvz-devx/ytfast-gpui/main/crates/core/src/jsc/pins.txt";

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
    /// Whether the account has Premium ([`UNKNOWN`], [`PREMIUM`] once a
    /// response had its formats, [`NOT_PREMIUM`] once an account client's
    /// stream failed to play before that).
    premium: AtomicU8,
    /// The TV player variant's signature timestamp, by player version.
    tv_sts: Mutex<Option<(String, u32)>>,
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
            premium: AtomicU8::new(UNKNOWN),
            tv_sts: Mutex::default(),
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

    /// The best audio stream and the client it came from.
    pub async fn resolve(&self, video_id: &str, account: bool) -> Result<(Stream, &'static str)> {
        let (mut streams, client) = self.streams(video_id, account, 1).await?;
        Ok((streams.remove(0), client))
    }

    /// Up to `limit` of the wanted formats, best first, and the client they
    /// came from. With `account`, unless it is known to lack Premium:
    /// YouTube Music's web client with the session (774 and 141), then
    /// WEB_CREATOR (251); then, or else, signed out through VISIONOS, which
    /// needs no JS. One `player` request per song while the first works.
    pub async fn streams(
        &self,
        video_id: &str,
        account: bool,
        limit: usize,
    ) -> Result<(Vec<Stream>, &'static str)> {
        let mut errors = Vec::new();
        let premium = self.premium.load(Ordering::Relaxed) != NOT_PREMIUM;
        for client in ACCOUNT_CLIENTS.into_iter().filter(|_| account && premium) {
            match self.streams_as(client, video_id, true, limit).await {
                Ok(streams) => {
                    if streams.iter().any(|s| PREMIUM_ITAGS.contains(&s.itag)) {
                        self.premium.store(PREMIUM, Ordering::Relaxed);
                    }
                    return Ok((streams, client.name));
                }
                Err(error) => {
                    self.log_failure(video_id, &error);
                    errors.push(format!("{}: {error:#}", client.name));
                    // A player being prepared fails the next client too.
                    if error.downcast_ref::<PlayerFailure>().is_some() {
                        break;
                    }
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

    /// A stream from `url` failed to play. One from an account client
    /// before any response had Premium's formats means the account has no
    /// Premium (those URLs then need a PO token): the account clients
    /// aren't asked again this session, so only the first song pays a
    /// failed start.
    pub fn stream_failed(&self, url: &str) {
        let client = url_param(url, "c");
        let ours = ACCOUNT_CLIENTS
            .iter()
            .any(|c| client.as_deref() == Some(c.name));
        if ours
            && self
                .premium
                .compare_exchange(UNKNOWN, NOT_PREMIUM, Ordering::Relaxed, Ordering::Relaxed)
                .is_ok()
        {
            log::warn!("the account's streams don't play (no Premium?); resolving signed out");
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
            true => Some(self.web_config(video_id).await),
            false => None,
        };
        let want = web.as_ref().and_then(|w| w.player.clone());
        let mut current = if needs_js {
            Some(self.player_for(want.as_deref()).await?)
        } else {
            None
        };
        // The TV client wants the TV player variant's timestamp; with the
        // web player's it answers "The page needs to be reloaded".
        let sts = match (client.name == TV_DOWNGRADED.name, &current) {
            (true, Some(current)) => Some(self.tv_sts(current).await),
            (false, Some(current)) => Some(current.sts),
            (_, None) => None,
        };
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
        let formats = audio_formats(&response).map_err(|error| match authed {
            // Whether YouTube took the session, for the log.
            true => match crate::parse::logged_in(&response) {
                Some(yes) => {
                    error.context(format!("signed in: {}", if yes { "yes" } else { "no" }))
                }
                None => error,
            },
            false => error,
        })?;
        for format in formats.into_iter().take(limit) {
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

    /// The signature timestamp of the player's TV variant
    /// (`player_ias_tcl`, eight digits where the web player's has five),
    /// read once per player version and saved beside it; without it, the
    /// web player's. With it the TV client answers, but its URLs are
    /// ciphered for that variant, which the solver can't read (EJS 0.8.0
    /// finds no functions in it), and solved with the web player's
    /// functions they got 403 on 2026-10-07. So the app doesn't ask the
    /// TV client (docs/gpui/RESOLVER.md, M27).
    async fn tv_sts(&self, current: &Current) -> u32 {
        let id = &current.player.id;
        if let Some((known, sts)) = self.tv_sts.lock().expect("tv sts lock").as_ref()
            && known == id
        {
            return *sts;
        }
        let saved = self.dir.join(format!("{id}.tv-sts"));
        let read = match std::fs::read_to_string(&saved)
            .ok()
            .and_then(|t| t.trim().parse().ok())
        {
            Some(sts) => Ok(sts),
            None => self.download_tv_sts(id).await.inspect(|sts| {
                let _ = crate::paths::write_atomic(&saved, sts.to_string().as_bytes());
            }),
        };
        match read {
            Ok(sts) => {
                *self.tv_sts.lock().expect("tv sts lock") = Some((id.clone(), sts));
                sts
            }
            Err(error) => {
                log::warn!("reading player {id}'s TV timestamp failed: {error:#}");
                current.sts
            }
        }
    }

    async fn download_tv_sts(&self, id: &str) -> Result<u32> {
        let url =
            format!("https://www.youtube.com/s/player/{id}/player_ias_tcl.vflset/en_US/base.js");
        let source = self
            .innertube
            .http()
            .get(&url)
            .header("User-Agent", WEB_AGENT)
            .send()
            .await
            .context("downloading the TV player")?
            .error_for_status()?
            .text()
            .await?;
        let sts = signature_timestamp(&source).context("the TV player has no timestamp")?;
        log::info!("player {id}'s TV variant has signature timestamp {sts}");
        Ok(sts)
    }

    /// The session's www.youtube.com config, from the first song's watch
    /// page (`tv::page_url`), read at most every [`PLAYER_CHECK`] (after a failure,
    /// every ten minutes; meanwhile the TV client is asked without it).
    async fn web_config(&self, video_id: &str) -> tv::WebConfig {
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
        let read = self.read_web_config(video_id).await;
        if let Err(error) = &read {
            log::warn!("reading the session's page config failed: {error:#}");
        }
        let ok = read.is_ok();
        let config = read.unwrap_or_default();
        *web = Some((config.clone(), Instant::now(), ok));
        config
    }

    async fn read_web_config(&self, video_id: &str) -> Result<tv::WebConfig> {
        let mut request = self
            .innertube
            .http()
            .get(tv::page_url(video_id))
            .header("User-Agent", WEB_AGENT)
            .header("Accept-Language", "en-us,en;q=0.5");
        if let Some(cookies) = self.innertube.cookie_header() {
            request = request.header("Cookie", cookies);
        }
        let response = request
            .send()
            .await
            .context("asking for the session's page")?
            .error_for_status()?;
        let cookies = set_cookies(&response);
        let page = response.text().await?;
        let mut config = tv::parse_ytcfg(&page);
        config
            .cookies
            .add(cookies.iter().map(String::as_str), this_year());
        if config.player.is_none() && config.visitor.is_none() {
            bail!("the page has no config");
        }
        log::info!("the session's page config: {}", config.summary());
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
            request = request.header("Cookie", web.cookies.apply(&cookies));
        }
        let response = request.send().await.context("asking for the streams")?;
        let status = response.status();
        if !status.is_success() {
            bail!("YouTube answered the player request with HTTP {status}");
        }
        // Cookies it renews are kept for the next song, as a browser would.
        let renewed = set_cookies(&response);
        if !renewed.is_empty()
            && let Some((config, _, _)) = self.web.lock().await.as_mut()
        {
            config
                .cookies
                .add(renewed.iter().map(String::as_str), this_year());
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
            let ours = name.ends_with(".js") || name.ends_with(".tv-sts");
            if ours && !name.starts_with(&format!("{id}.")) {
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

/// A response's `Set-Cookie` headers.
fn set_cookies(response: &reqwest::Response) -> Vec<String> {
    response
        .headers()
        .get_all(reqwest::header::SET_COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok().map(str::to_owned))
        .collect()
}

/// This year (UTC, near enough to tell a cookie deletion).
fn this_year() -> u32 {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    1970 + (secs / 31_556_952) as u32
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
/// the URLs must be encrypted for: five digits in the web player, eight in
/// its TV variant (`20728` and `20728001` for player `1b3be681`).
pub fn signature_timestamp(source: &str) -> Option<u32> {
    source.split("signatureTimestamp").skip(1).find_map(|rest| {
        let rest = rest.trim_start_matches([' ', ':']);
        let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
        (5..=9)
            .contains(&digits.len())
            .then(|| digits.parse().ok())
            .flatten()
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

    #[test]
    fn reads_both_timestamps() {
        assert_eq!(
            signature_timestamp("x={signatureTimestamp:20728,y"),
            Some(20728)
        );
        assert_eq!(
            signature_timestamp("x={signatureTimestamp:20728001,y"),
            Some(20_728_001)
        );
        assert_eq!(signature_timestamp("signatureTimestamp:12,y"), None);
    }

    fn native() -> Native {
        let dir = std::env::temp_dir().join("ytfast-streams-test");
        Native::new(Arc::new(Client::new()), &dir, &dir)
    }

    /// An account client's stream that fails to play before Premium was
    /// seen means no Premium; other clients' failures don't count, and
    /// once Premium was seen nothing turns it off.
    #[test]
    fn learns_no_premium_from_a_failed_stream() {
        let native = native();
        native.stream_failed("https://example.invalid/videoplayback?c=VISIONOS&n=x");
        assert_eq!(native.premium.load(Ordering::Relaxed), UNKNOWN);
        native.stream_failed("https://example.invalid/videoplayback?c=WEB_REMIX&n=x");
        assert_eq!(native.premium.load(Ordering::Relaxed), NOT_PREMIUM);
        let premium = native_premium();
        premium.stream_failed("https://example.invalid/videoplayback?c=WEB_CREATOR&n=x");
        assert_eq!(premium.premium.load(Ordering::Relaxed), PREMIUM);
    }

    fn native_premium() -> Native {
        let native = native();
        native.premium.store(PREMIUM, Ordering::Relaxed);
        native
    }
}
