//! The cpal output stream, on a thread of its own (cpal streams aren't
//! `Send` on every host, and the backend wants a `Send` engine).

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::thread;

use anyhow::{Context, Result, anyhow};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{SampleFormat, StreamConfig};
use rtrb::{Consumer, Producer};

use crate::mixer::{Command, MixEvent, Mixer};

/// Frames per period: ~43 ms at 48 kHz, two periods buffered (~85 ms).
/// cpal's default on PipeWire's ALSA plugin is two ~21 ms periods, which
/// underran on a loaded machine; mpv buffers 200 ms.
const PERIOD: u32 = 2048;

/// Starts the output; returns its thread and the device's sample rate.
pub fn spawn(
    commands: Consumer<Command>,
    events: Producer<MixEvent>,
    stop: Arc<AtomicBool>,
) -> Result<(thread::JoinHandle<()>, u32)> {
    let (ready_tx, ready_rx) = mpsc::channel();
    let handle = thread::Builder::new()
        .name("audio-output".into())
        .spawn(move || {
            let stream = match open(commands, events) {
                Ok((stream, rate)) => {
                    let _ = ready_tx.send(Ok(rate));
                    stream
                }
                Err(e) => {
                    let _ = ready_tx.send(Err(e));
                    return;
                }
            };
            while !stop.load(Ordering::Acquire) {
                thread::park();
            }
            drop(stream);
        })?;
    let rate = ready_rx
        .recv()
        .map_err(|_| anyhow!("audio output thread ended"))??;
    Ok((handle, rate))
}

fn open(commands: Consumer<Command>, events: Producer<MixEvent>) -> Result<(cpal::Stream, u32)> {
    let host = host();
    let device = host
        .default_output_device()
        .context("no audio output device")?;
    let default = device.default_output_config()?;
    let rate = default.sample_rate();
    let channels = default.channels();
    if default.sample_format() != SampleFormat::F32 {
        let f32_ok = device.supported_output_configs()?.any(|c| {
            c.sample_format() == SampleFormat::F32
                && c.channels() == channels
                && (c.min_sample_rate()..=c.max_sample_rate()).contains(&rate)
        });
        anyhow::ensure!(f32_ok, "the output device has no f32 format");
    }
    let fits = match default.buffer_size() {
        cpal::SupportedBufferSize::Range { min, max } => (*min..=*max).contains(&PERIOD),
        cpal::SupportedBufferSize::Unknown => true,
    };
    let config = StreamConfig {
        channels,
        sample_rate: rate,
        buffer_size: if fits {
            cpal::BufferSize::Fixed(PERIOD)
        } else {
            cpal::BufferSize::Default
        },
    };
    log::info!(
        "audio output: {} on {:?}, {rate} Hz, {channels} channels, {:?}",
        device.id().map(|d| d.to_string()).unwrap_or_default(),
        host.id(),
        config.buffer_size,
    );
    let mut mixer = Mixer::new(rate, commands, events);
    let channels = channels as usize;
    let mut first = true;
    let stream = device.build_output_stream(
        config,
        move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
            if first {
                first = false;
                log::info!(
                    "audio output: {} frames per callback",
                    data.len() / channels
                );
            }
            mixer.render(data, channels)
        },
        |e| log::warn!("audio output: {e}"),
        None,
    )?;
    stream.play()?;
    Ok((stream, rate))
}

/// On Linux the PulseAudio protocol (pipewire-pulse, pure Rust client) when
/// a server answers, else ALSA; the platform default elsewhere.
fn host() -> cpal::Host {
    #[cfg(target_os = "linux")]
    if std::env::var_os("ENCORE_AUDIO_ALSA").is_none()
        && let Ok(host) = cpal::host_from_id(cpal::HostId::PulseAudio)
    {
        return host;
    }
    cpal::default_host()
}
