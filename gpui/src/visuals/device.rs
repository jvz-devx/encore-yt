//! The effects' GPU device, made on a thread of its own (M17). Making it
//! takes 80 ms or more (Vulkan's instance, adapter and device), which held
//! up the first frame whenever a song was restored; now that frame shows
//! the plain bar and the effects take over once the device is there.

use std::time::Instant;

use ytfast_visuals::Gpu;

pub type Made = Result<Gpu, String>;

/// Starts making the device; the receiver gets it once made.
pub fn start() -> smol::channel::Receiver<Made> {
    let (tx, rx) = smol::channel::bounded(1);
    let spawned = std::thread::Builder::new()
        .name("ytfast-gpu".into())
        .spawn(move || {
            let _ = tx.send_blocking(make());
        });
    if let Err(e) = spawned {
        log::warn!("visuals: no thread for the GPU device: {e}");
    }
    rx
}

fn make() -> Made {
    let started = Instant::now();
    let made = Gpu::new().map_err(|e| format!("{e:#}"));
    match &made {
        Ok(gpu) => log::info!(
            "visuals: GPU {} (set up in {:.0} ms)",
            gpu.adapter(),
            started.elapsed().as_secs_f64() * 1000.0
        ),
        Err(e) => log::warn!("visuals: no GPU, effects fall back: {e}"),
    }
    made
}
