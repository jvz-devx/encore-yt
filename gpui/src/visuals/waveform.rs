//! A whole-song waveform (peaks for the seek bar), computed beside playback.
//!
//! mpv already holds the song: its demuxer cache (64 MiB) takes a whole
//! YouTube audio stream within seconds. Once mpv reports `eof-cached`, the
//! `dump-cache` command writes the cached bytes to a file and ffmpeg decodes
//! that to 8 kHz mono, so nothing is downloaded twice. Before that point (or
//! if the dump fails) ffmpeg reads the stream URL itself (mpv's `path`).
//!
//! The spike asks mpv over its IPC socket; in the app this belongs in the
//! backend, which owns the mpv handle and the resolved URL.

use std::io::{BufRead as _, BufReader, Read as _, Write as _};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context as _, Result, anyhow, bail};
use serde_json::{Value, json};

/// Peaks per song.
pub const BUCKETS: usize = 400;
const DECODE_RATE: u32 = 8000;

#[derive(Clone, Default)]
pub struct Waveform {
    pub video_id: String,
    /// 0..1, `BUCKETS` long once done.
    pub peaks: Vec<f32>,
    /// How it was made and how long it took, for the overlay and the log.
    pub how: String,
}

/// Starts making the waveform of what mpv plays now; `out` gets it.
pub fn start(socket: PathBuf, video_id: String, out: Arc<Mutex<Option<Waveform>>>) {
    let spawned = thread::Builder::new()
        .name("visuals-waveform".into())
        .spawn(move || match make(&socket, &video_id) {
            Ok(waveform) => {
                log::info!("visuals: waveform of {video_id}: {}", waveform.how);
                *out.lock().expect("waveform") = Some(waveform);
            }
            Err(e) => log::warn!("visuals: waveform of {video_id}: {e:#}"),
        });
    if let Err(e) = spawned {
        log::warn!("visuals: no waveform thread: {e}");
    }
}

fn make(socket: &Path, video_id: &str) -> Result<Waveform> {
    let started = Instant::now();
    let mut ipc = Ipc::connect(socket)?;
    // mpv may still be opening the song.
    let mut url = None;
    for _ in 0..20 {
        if let Ok(Value::String(path)) = ipc.get("path") {
            url = Some(path);
            break;
        }
        thread::sleep(Duration::from_millis(500));
    }
    let url = url.context("mpv has no song open")?;
    // Wait up to 20 s for the whole song to be cached, then dump it.
    let mut source = None;
    for _ in 0..40 {
        // Unavailable until the demuxer runs.
        let state = ipc.get("demuxer-cache-state").unwrap_or_default();
        if state["eof-cached"] == true && state["bof-cached"] == true {
            // Beside mpv's socket: the runtime directory is private (0700).
            let file = socket.with_file_name(format!("waveform-{video_id}"));
            let dumped = Instant::now();
            ipc.command(json!(["dump-cache", 0, "no", file.to_string_lossy()]))?;
            let size = std::fs::metadata(&file).map(|m| m.len()).unwrap_or(0);
            log::info!(
                "visuals: dump-cache wrote {size} bytes in {:.0} ms",
                dumped.elapsed().as_secs_f64() * 1000.0
            );
            source = Some((file.to_string_lossy().into_owned(), true));
            break;
        }
        thread::sleep(Duration::from_millis(500));
    }
    let (input, from_cache) = source.unwrap_or((url, false));
    let decoding = Instant::now();
    let samples = decode(&input);
    if from_cache {
        let _ = std::fs::remove_file(&input);
    }
    let samples = samples?;
    let decode_ms = decoding.elapsed().as_secs_f64() * 1000.0;
    let how = format!(
        "{} s of audio from {} decoded in {decode_ms:.0} ms ({:.0} ms after the song started loading here)",
        samples.len() / DECODE_RATE as usize,
        if from_cache {
            "mpv's cache"
        } else {
            "the stream URL"
        },
        started.elapsed().as_secs_f64() * 1000.0,
    );
    Ok(Waveform {
        video_id: video_id.to_owned(),
        peaks: peaks(&samples),
        how,
    })
}

/// ffmpeg decodes to 8 kHz mono f32 on stdout.
fn decode(input: &str) -> Result<Vec<f32>> {
    let mut child = Command::new("ffmpeg")
        .args(["-nostdin", "-loglevel", "error", "-i", input])
        .args(["-vn", "-ac", "1", "-ar", &DECODE_RATE.to_string()])
        .args(["-f", "f32le", "-"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .context("starting ffmpeg")?;
    let mut bytes = Vec::new();
    child
        .stdout
        .take()
        .context("ffmpeg stdout")?
        .read_to_end(&mut bytes)?;
    let status = child.wait()?;
    if !status.success() {
        bail!("ffmpeg exited with {status}");
    }
    Ok(bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|b| f32::from_le_bytes(*b))
        .collect())
}

/// Peak per bucket, scaled so the loudest is 1.
fn peaks(samples: &[f32]) -> Vec<f32> {
    let per = samples.len().div_ceil(BUCKETS).max(1);
    let raw: Vec<f32> = samples
        .chunks(per)
        .map(|c| c.iter().fold(0.0f32, |m, s| m.max(s.abs())))
        .collect();
    let loudest = raw.iter().copied().fold(1e-6f32, f32::max);
    raw.iter().map(|p| p / loudest).collect()
}

/// A second client on mpv's JSON IPC socket (mpv takes many).
struct Ipc {
    reader: BufReader<UnixStream>,
    writer: UnixStream,
    next: u64,
}

impl Ipc {
    fn connect(socket: &Path) -> Result<Self> {
        let stream = UnixStream::connect(socket)
            .with_context(|| format!("connecting to {}", socket.display()))?;
        stream.set_read_timeout(Some(Duration::from_secs(5)))?;
        Ok(Self {
            reader: BufReader::new(stream.try_clone()?),
            writer: stream,
            next: 1,
        })
    }

    fn get(&mut self, property: &str) -> Result<Value> {
        self.command(json!(["get_property", property]))
    }

    fn command(&mut self, command: Value) -> Result<Value> {
        let id = self.next;
        self.next += 1;
        let line = json!({ "command": command, "request_id": id }).to_string();
        self.writer.write_all(format!("{line}\n").as_bytes())?;
        let mut reply = String::new();
        loop {
            reply.clear();
            if self.reader.read_line(&mut reply)? == 0 {
                bail!("mpv closed the socket");
            }
            let message: Value = serde_json::from_str(&reply)?;
            if message["request_id"] != id {
                continue;
            }
            if message["error"] != "success" {
                return Err(anyhow!("mpv: {}", message["error"]));
            }
            return Ok(message["data"].clone());
        }
    }
}
