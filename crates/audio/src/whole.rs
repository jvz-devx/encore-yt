//! A whole stream decoded at once, outside playback: for the song's
//! waveform under Now Playing. The same source (range requests, or a local
//! file), demuxers and decoders as playback, mixed down to mono and to
//! about the rate asked for by averaging blocks of samples.

use std::time::Instant;

use anyhow::{Context, Result, anyhow};
use reqwest::blocking::Client;
use symphonia::core::codecs::audio::AudioDecoderOptions;
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::{FormatOptions, TrackType};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;

use crate::decode::{CODECS, hint, local_path};
use crate::http::HttpSource;

/// Mono samples of a whole stream and their rate.
pub struct Decoded {
    pub samples: Vec<f32>,
    pub rate: u32,
}

/// Downloads and decodes all of `url` (blocks: a few seconds for a song)
/// to mono at `rate` or a little above it.
pub fn decode_mono(url: &str, rate: u32) -> Result<Decoded> {
    let started = Instant::now();
    let source = match local_path(url) {
        Some(path) => HttpSource::local(path)?,
        None => HttpSource::open(&Client::builder().build()?, url, Default::default())?,
    };
    let mss = MediaSourceStream::new(Box::new(source), Default::default());
    let mut format = symphonia::default::get_probe()
        .probe(
            &hint(url),
            mss,
            FormatOptions::default(),
            MetadataOptions::default(),
        )
        .context("unsupported container")?;
    let track = format
        .default_track(TrackType::Audio)
        .context("no audio track")?;
    let params = track
        .codec_params
        .as_ref()
        .and_then(|p| p.audio())
        .context("no audio codec parameters")?
        .clone();
    let track_id = track.id;
    let codec_rate = params
        .sample_rate
        .filter(|rate| *rate > 0)
        .context("no valid sample rate")?;
    let mut decoder = CODECS
        .make_audio_decoder(&params, &AudioDecoderOptions::default())
        .context("unsupported codec")?;
    let block = (codec_rate / rate.max(1)).max(1) as usize;
    let mut mono = Mono::new(block);
    let mut samples = Vec::new();
    loop {
        let packet = match format.next_packet() {
            Ok(Some(packet)) => packet,
            Ok(None) => break,
            Err(SymphoniaError::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                break;
            }
            Err(e) => return Err(anyhow!(e).context("demux")),
        };
        if packet.track_id != track_id {
            continue;
        }
        let decoded = match decoder.decode(&packet) {
            Ok(decoded) => decoded,
            Err(SymphoniaError::DecodeError(_)) => continue,
            Err(e) => return Err(anyhow!(e).context("decode")),
        };
        let channels = decoded.spec().channels().count().max(1);
        decoded.copy_to_vec_interleaved::<f32>(&mut samples);
        mono.push(&samples, channels);
    }
    let rate = codec_rate / block as u32;
    log::info!(
        "decoded {:.0} s whole at {rate} Hz in {:.2} s",
        mono.out.len() as f64 / f64::from(rate),
        started.elapsed().as_secs_f64()
    );
    Ok(Decoded {
        samples: mono.out,
        rate,
    })
}

/// Mixes frames to mono and averages blocks of them.
struct Mono {
    block: usize,
    sum: f32,
    count: usize,
    out: Vec<f32>,
}

impl Mono {
    fn new(block: usize) -> Self {
        Self {
            block,
            sum: 0.0,
            count: 0,
            out: Vec::new(),
        }
    }

    fn push(&mut self, interleaved: &[f32], channels: usize) {
        for frame in interleaved.chunks_exact(channels) {
            self.sum += frame.iter().sum::<f32>() / channels as f32;
            self.count += 1;
            if self.count == self.block {
                self.out.push(self.sum / self.block as f32);
                self.sum = 0.0;
                self.count = 0;
            }
        }
    }
}
