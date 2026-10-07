//! A whole song's loudness outline, for the waveform under Now Playing.
//!
//! ffmpeg decodes the stream URL that mpv plays (asked over mpv's IPC
//! socket) to 8 kHz mono; [`BUCKETS`] RMS values, scaled for display, are
//! cached per video id in the cache directory, so a song is decoded once.
//! Everything here blocks: call [`load`] off the UI thread.
//!
//! Decoding downloads the song a second time (~4 MB for itag 251) and takes
//! about 1.5 s for a 4-minute song (NOTES-visuals.md). mpv's `dump-cache`
//! would avoid the download but came out truncated when tested.

use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context as _, Result, bail};
use serde_json::Value;

use crate::mpv::Ipc;

/// Values per song.
pub const BUCKETS: usize = 160;
const DECODE_RATE: u32 = 8000;
/// The quietest value drawn, so silence still shows a line.
const FLOOR: f32 = 0.08;

/// The cached outline of `video_id`, if it was made before.
pub fn cached(cache_dir: &Path, video_id: &str) -> Option<Vec<f32>> {
    let bytes = std::fs::read(cache_file(cache_dir, video_id)).ok()?;
    let values: Vec<f32> = bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|b| f32::from_le_bytes(*b))
        .collect();
    (values.len() == BUCKETS).then_some(values)
}

/// The outline of what plays now (`video_id`): from the cache, or decoded
/// from `url` (or, without one, the URL mpv at `socket` plays) and then
/// cached. Blocks for a few seconds.
pub fn load(
    url: Option<String>,
    socket: &Path,
    cache_dir: &Path,
    video_id: &str,
) -> Result<Vec<f32>> {
    if let Some(values) = cached(cache_dir, video_id) {
        return Ok(values);
    }
    let started = Instant::now();
    let url = match url {
        Some(url) => url,
        None => stream_url(socket)?,
    };
    let samples = decode(&url)?;
    let values = outline(&samples);
    log::info!(
        "visuals: waveform of {video_id}: {} s of audio in {:.0} ms",
        samples.len() / DECODE_RATE as usize,
        started.elapsed().as_secs_f64() * 1000.0
    );
    let file = cache_file(cache_dir, video_id);
    if let Some(dir) = file.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let bytes: Vec<u8> = values.iter().flat_map(|v| v.to_le_bytes()).collect();
    if let Err(e) = std::fs::write(&file, bytes) {
        log::warn!("visuals: caching the waveform: {e}");
    }
    Ok(values)
}

fn cache_file(cache_dir: &Path, video_id: &str) -> PathBuf {
    // Video ids are [A-Za-z0-9_-]; anything else is dropped.
    let name: String = video_id
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-')
        .collect();
    cache_dir.join("waveforms").join(format!("{name}.f32"))
}

/// mpv's `path`: the resolved stream URL. mpv may still be opening it.
fn stream_url(socket: &Path) -> Result<String> {
    let mut ipc = Ipc::connect(socket)?;
    for _ in 0..20 {
        if let Ok(Value::String(path)) = ipc.get("path") {
            return Ok(path);
        }
        thread::sleep(Duration::from_millis(500));
    }
    bail!("mpv has no song open")
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

/// RMS per bucket, stretched between a quiet reference (half the 10th
/// percentile) and the loudest bucket, so loud masters still show their
/// shape instead of a solid block. 0..1, at least `FLOOR`.
pub fn outline(samples: &[f32]) -> Vec<f32> {
    let per = samples.len().div_ceil(BUCKETS).max(1);
    let mut rms: Vec<f32> = samples
        .chunks(per)
        .map(|c| (c.iter().map(|s| s * s).sum::<f32>() / c.len() as f32).sqrt())
        .collect();
    rms.resize(BUCKETS, 0.0);
    let mut sorted = rms.clone();
    sorted.sort_by(f32::total_cmp);
    let quiet = sorted[BUCKETS / 10] * 0.5;
    let loudest = sorted[BUCKETS - 1].max(quiet + 1e-6);
    rms.iter()
        .map(|r| FLOOR + (1.0 - FLOOR) * ((r - quiet) / (loudest - quiet)).clamp(0.0, 1.0))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A quiet intro, a loud part and a fade keep their order in the
    /// outline, which spans the full range.
    #[test]
    fn outline_keeps_the_song_shape() {
        let mut samples = Vec::new();
        for (amplitude, seconds) in [(0.05f32, 20), (0.8, 60), (0.4, 20)] {
            let n = seconds * DECODE_RATE as usize;
            samples.extend((0..n).map(|i| (i as f32 * 0.3).sin() * amplitude));
        }
        let values = outline(&samples);
        assert_eq!(values.len(), BUCKETS);
        let (intro, loud, fade) = (values[5], values[BUCKETS / 2], values[BUCKETS - 5]);
        assert!(intro < fade && fade < loud, "{intro} {fade} {loud}");
        assert!((loud - 1.0).abs() < 1e-3 && intro >= FLOOR);
    }
}
