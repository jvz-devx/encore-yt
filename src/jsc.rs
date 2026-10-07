//! Solves the JS challenges in YouTube's player script (the signature
//! cipher and the `n` parameter) in an embedded QuickJS.
//!
//! The extraction is yt-dlp's EJS solver (github.com/yt-dlp/ejs, Unlicense,
//! bundling meriyah and astring), the same scripts yt-dlp runs in deno: it
//! parses the player, finds the functions and rewrites the player into a
//! "preprocessed" script that hands them out. That rewrite is the slow part
//! (about a second), so it is saved per player version next to the player,
//! and the engine keeps the last player's functions loaded. A signature
//! transform depends only on the signature's length (it reorders and drops
//! characters), so it is solved once per length as an index list and then
//! applied in Rust.
//!
//! QuickJS contexts aren't `Send`: the engine lives on its own thread and
//! takes jobs over a channel.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::mpsc;

use anyhow::{Context as _, Result, anyhow, bail};
use rquickjs::{CatchResultExt, Context, Function, Object, Runtime};

const EJS_LIB: &str = include_str!("jsc/ejs-lib.min.js");
const EJS_CORE: &str = include_str!("jsc/ejs-core.min.js");

/// A player script saved on disk: `<dir>/<id>.js`, with its preprocessed
/// form written beside it as `<id>.ejs.js`.
#[derive(Clone, Debug)]
pub struct Player {
    pub id: String,
    pub path: PathBuf,
}

impl Player {
    fn preprocessed(&self) -> PathBuf {
        self.path.with_extension("ejs.js")
    }
}

/// What a player request needs solved.
#[derive(Default, Debug)]
pub struct Challenges {
    pub n: Vec<String>,
    pub sig_lengths: Vec<usize>,
}

/// The answers: `n` values by challenge, signature index lists by length.
#[derive(Default, Debug, Clone)]
pub struct Solutions {
    pub n: HashMap<String, String>,
    pub sig: HashMap<usize, Vec<usize>>,
}

/// Applies a signature index list to an encrypted signature.
pub fn decipher(s: &str, spec: &[usize]) -> Option<String> {
    let chars: Vec<char> = s.chars().collect();
    spec.iter().map(|&i| chars.get(i).copied()).collect()
}

struct Job {
    player: Player,
    challenges: Challenges,
    reply: tokio::sync::oneshot::Sender<Result<Solutions>>,
}

/// The engine thread and the answers it gave, per player version.
pub struct Solver {
    jobs: Mutex<Option<mpsc::Sender<Job>>>,
    known: Mutex<HashMap<String, Solutions>>,
}

impl Default for Solver {
    fn default() -> Self {
        Self::new()
    }
}

impl Solver {
    pub fn new() -> Self {
        Self {
            jobs: Mutex::default(),
            known: Mutex::default(),
        }
    }

    /// Solves the challenges, from memory where they were solved before.
    pub async fn solve(&self, player: &Player, challenges: Challenges) -> Result<Solutions> {
        let (known, missing) = self.split(player, challenges);
        if missing.n.is_empty() && missing.sig_lengths.is_empty() {
            return Ok(known);
        }
        let (reply, answer) = tokio::sync::oneshot::channel();
        self.sender()?
            .send(Job {
                player: player.clone(),
                challenges: missing,
                reply,
            })
            .map_err(|_| anyhow!("the JS engine stopped"))?;
        let solved = answer.await.context("the JS engine stopped")??;
        Ok(self.merge(player, known, solved))
    }

    /// Whether the player's functions can be loaded without preprocessing
    /// (a ~15 s job in QuickJS, once per player version).
    pub fn prepared(&self, player: &Player) -> bool {
        self.known
            .lock()
            .expect("solutions lock")
            .contains_key(&player.id)
            || player.preprocessed().exists()
    }

    /// Preprocesses and loads a player, waiting for it.
    pub async fn load(&self, player: &Player) -> Result<()> {
        let (reply, answer) = tokio::sync::oneshot::channel();
        self.sender()?
            .send(Job {
                player: player.clone(),
                challenges: Challenges::default(),
                reply,
            })
            .map_err(|_| anyhow!("the JS engine stopped"))?;
        answer.await.context("the JS engine stopped")??;
        self.known
            .lock()
            .expect("solutions lock")
            .entry(player.id.clone())
            .or_default();
        Ok(())
    }

    /// Preprocesses and loads a player in the background.
    pub fn warm(&self, player: &Player) {
        let Ok(sender) = self.sender() else { return };
        let (reply, answer) = tokio::sync::oneshot::channel();
        let job = Job {
            player: player.clone(),
            challenges: Challenges::default(),
            reply,
        };
        if sender.send(job).is_ok() {
            let id = player.id.clone();
            std::thread::spawn(move || {
                if let Ok(Err(error)) = answer.blocking_recv() {
                    log::warn!("preparing player {id} failed: {error:#}");
                }
            });
        }
    }

    /// Splits the request into what is already known and what isn't.
    fn split(&self, player: &Player, challenges: Challenges) -> (Solutions, Challenges) {
        let known = self.known.lock().expect("solutions lock");
        let cached = known.get(&player.id);
        let mut have = Solutions::default();
        let mut missing = Challenges::default();
        for n in challenges.n {
            match cached.and_then(|c| c.n.get(&n)) {
                Some(value) => {
                    have.n.insert(n, value.clone());
                }
                None if !missing.n.contains(&n) => missing.n.push(n),
                None => {}
            }
        }
        for length in challenges.sig_lengths {
            match cached.and_then(|c| c.sig.get(&length)) {
                Some(spec) => {
                    have.sig.insert(length, spec.clone());
                }
                None if !missing.sig_lengths.contains(&length) => missing.sig_lengths.push(length),
                None => {}
            }
        }
        (have, missing)
    }

    fn merge(&self, player: &Player, mut have: Solutions, solved: Solutions) -> Solutions {
        let mut known = self.known.lock().expect("solutions lock");
        // One player version at a time: a new one replaces the old answers.
        known.retain(|id, _| *id == player.id);
        let entry = known.entry(player.id.clone()).or_default();
        // `n` answers are per song; keep the memory bounded.
        if entry.n.len() > 512 {
            entry.n.clear();
        }
        entry.n.extend(solved.n.clone());
        entry.sig.extend(solved.sig.clone());
        have.n.extend(solved.n);
        have.sig.extend(solved.sig);
        have
    }

    fn sender(&self) -> Result<mpsc::Sender<Job>> {
        let mut jobs = self.jobs.lock().expect("jobs lock");
        if let Some(sender) = jobs.as_ref() {
            return Ok(sender.clone());
        }
        let (sender, receiver) = mpsc::channel::<Job>();
        std::thread::Builder::new()
            .name("ytfast-jsc".into())
            // meriyah recurses deeply on the player's nested expressions.
            .stack_size(64 << 20)
            .spawn(move || run(receiver))
            .context("starting the JS engine")?;
        *jobs = Some(sender.clone());
        Ok(sender)
    }
}

fn run(jobs: mpsc::Receiver<Job>) {
    let mut engine = None;
    for job in jobs {
        let result = (|| {
            let engine = match &mut engine {
                Some(engine) => engine,
                None => engine.insert(Engine::new()?),
            };
            engine.solve(&job.player, &job.challenges)
        })();
        let _ = job.reply.send(result);
    }
}

/// A QuickJS context with one player's functions loaded.
pub struct Engine {
    _runtime: Runtime,
    context: Context,
    loaded: Option<String>,
    has_ejs: bool,
}

impl Engine {
    pub fn new() -> Result<Self> {
        let runtime = Runtime::new().context("creating the JS runtime")?;
        runtime.set_max_stack_size(48 << 20);
        runtime.set_memory_limit(1 << 30);
        let context = Context::full(&runtime).context("creating the JS context")?;
        Ok(Self {
            _runtime: runtime,
            context,
            loaded: None,
            has_ejs: false,
        })
    }

    /// Solves on the calling thread (the engine must stay on it).
    pub fn solve(&mut self, player: &Player, challenges: &Challenges) -> Result<Solutions> {
        if self.loaded.as_deref() != Some(&player.id) {
            self.load(player)?;
        }
        self.context.with(|ctx| {
            let solver: Object = ctx.globals().get("__ytfast").map_err(|e| js(&ctx, e))?;
            let mut solutions = Solutions::default();
            if !challenges.n.is_empty() {
                let n: Function = solver.get("n").map_err(|e| js(&ctx, e))?;
                for challenge in &challenges.n {
                    let answer: String = n
                        .call((challenge.as_str(),))
                        .catch(&ctx)
                        .map_err(|e| anyhow!("n function: {e}"))?;
                    if answer == *challenge || answer.starts_with("enhanced_except") {
                        bail!("the n function returned {answer:?}");
                    }
                    solutions.n.insert(challenge.clone(), answer);
                }
            }
            if !challenges.sig_lengths.is_empty() {
                let sig: Function = solver.get("sig").map_err(|e| js(&ctx, e))?;
                for &length in &challenges.sig_lengths {
                    let probe: String = (0..length as u32).filter_map(char::from_u32).collect();
                    let answer: String = sig
                        .call((probe,))
                        .catch(&ctx)
                        .map_err(|e| anyhow!("signature function: {e}"))?;
                    let spec: Vec<usize> = answer.chars().map(|c| c as usize).collect();
                    if spec.is_empty() || spec.iter().any(|&i| i >= length) {
                        bail!("the signature function gave no index list");
                    }
                    solutions.sig.insert(length, spec);
                }
            }
            Ok(solutions)
        })
    }

    /// Loads a player's functions, preprocessing it first if that wasn't
    /// done before for this version.
    fn load(&mut self, player: &Player) -> Result<()> {
        self.loaded = None;
        let saved = player.preprocessed();
        let preprocessed = match std::fs::read_to_string(&saved) {
            Ok(code) => code,
            Err(_) => {
                let source = std::fs::read_to_string(&player.path)
                    .with_context(|| format!("reading {}", player.path.display()))?;
                let started = std::time::Instant::now();
                let code = self.preprocess(&source)?;
                log::info!(
                    "preprocessed player {} in {:.1}s",
                    player.id,
                    started.elapsed().as_secs_f64()
                );
                write_atomic(&saved, code.as_bytes());
                code
            }
        };
        self.context.with(|ctx| {
            ctx.globals()
                .set("__ytfast_player", preprocessed)
                .map_err(|e| js(&ctx, e))?;
            ctx.eval::<(), _>(
                "globalThis.__ytfast = { n: null, sig: null };\n\
                 Function('_result', globalThis.__ytfast_player)(globalThis.__ytfast);\n\
                 delete globalThis.__ytfast_player;",
            )
            .catch(&ctx)
            .map_err(|e| anyhow!("loading the player: {e}"))?;
            let solver: Object = ctx.globals().get("__ytfast").map_err(|e| js(&ctx, e))?;
            for name in ["n", "sig"] {
                if solver
                    .get::<_, Option<Function>>(name)
                    .ok()
                    .flatten()
                    .is_none()
                {
                    bail!("the player has no {name} function the solver recognises");
                }
            }
            Ok(())
        })?;
        self.loaded = Some(player.id.clone());
        Ok(())
    }

    /// Runs EJS over the player source and returns the preprocessed script.
    fn preprocess(&mut self, source: &str) -> Result<String> {
        self.context.with(|ctx| {
            if !self.has_ejs {
                ctx.eval::<(), _>(EJS_LIB)
                    .catch(&ctx)
                    .map_err(|e| anyhow!("loading the EJS library: {e}"))?;
                ctx.eval::<(), _>("Object.assign(globalThis, lib);")
                    .catch(&ctx)
                    .map_err(|e| anyhow!("{e}"))?;
                ctx.eval::<(), _>(EJS_CORE)
                    .catch(&ctx)
                    .map_err(|e| anyhow!("loading the EJS solver: {e}"))?;
                self.has_ejs = true;
            }
            let jsc: Function = ctx.globals().get("jsc").map_err(|e| js(&ctx, e))?;
            let input = Object::new(ctx.clone()).map_err(|e| js(&ctx, e))?;
            input.set("type", "player").map_err(|e| js(&ctx, e))?;
            input.set("player", source).map_err(|e| js(&ctx, e))?;
            input
                .set("requests", rquickjs::Array::new(ctx.clone()))
                .map_err(|e| js(&ctx, e))?;
            input
                .set("output_preprocessed", true)
                .map_err(|e| js(&ctx, e))?;
            let output: Object = jsc
                .call((input,))
                .catch(&ctx)
                .map_err(|e| anyhow!("EJS: {e}"))?;
            let kind: String = output.get("type").map_err(|e| js(&ctx, e))?;
            if kind != "result" {
                let error: Option<String> = output.get("error").ok();
                bail!("EJS: {}", error.unwrap_or(kind));
            }
            output
                .get::<_, String>("preprocessed_player")
                .map_err(|e| js(&ctx, e))
        })
    }
}

fn js(ctx: &rquickjs::Ctx<'_>, error: rquickjs::Error) -> anyhow::Error {
    if error.is_exception() {
        let caught = rquickjs::CaughtError::from_error(ctx, error);
        anyhow!("{caught}")
    } else {
        anyhow!("{error}")
    }
}

fn write_atomic(path: &Path, bytes: &[u8]) {
    if let Err(error) = crate::paths::write_atomic(path, bytes) {
        log::warn!("couldn't save {}: {error}", path.display());
    }
}
