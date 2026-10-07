//! Times the 3D scenes (M30) as the app draws them: each scene's frames
//! rendered and read back through the same two-slot path, at the sizes the
//! app renders them for a window.
//!
//!     cargo run -p encore-visuals --release --example scene_bench [frames]
//!
//! Per scene and size: wall time per frame in a steady loop (GPU and
//! readback, the CPU never waiting unless the GPU is behind), and the
//! split the target reports (submit, waiting on the GPU, copying the bytes
//! out). Run it on the target machines (the UHD 630, a MacBook) with the
//! app closed.

use std::time::Instant;

use encore_visuals::{BANDS, Gpu, Look, Pace, Scene, SceneKind, SceneParams, seed};

fn main() -> anyhow::Result<()> {
    let frames: usize = std::env::args()
        .nth(1)
        .and_then(|a| a.parse().ok())
        .unwrap_or(240);
    let gpu = Gpu::new()?;
    println!("{}, {frames} frames each", gpu.adapter());
    println!("| Scene | Look | Pixels | ms a frame | submit | wait | copy |");
    println!("|---|---|---|---|---|---|---|");
    let mut levels = [0.0; BANDS];
    for (i, l) in levels.iter_mut().enumerate() {
        *l = 0.75 - 0.4 * i as f32 / BANDS as f32;
    }
    let palette = [
        [0.48, 0.55, 0.47, 1.0],
        [0.7, 0.66, 0.55, 1.0],
        [0.35, 0.42, 0.46, 1.0],
        [0.27, 0.3, 0.28, 1.0],
    ];
    // A 1280x800 window at 2x, as the app renders each scene for it: the
    // full window (visualiser) and Now Playing's panel (backdrop).
    let sizes = [
        (SceneKind::Xmb, 1.0),
        (SceneKind::Ridges, 0.6),
        (SceneKind::Aurora, 0.5),
    ];
    for (kind, scale) in sizes {
        for (visualiser, w, h) in [(true, 2560.0, 1600.0), (false, 1700.0, 1400.0)] {
            let size = ((w * scale) as u32, (h * scale) as u32);
            let mut scene = Scene::new(&gpu, size.0, size.1);
            let mut pace = Pace::default();
            let (mut submit, mut wait, mut copy) = (0.0, 0.0, 0.0);
            let mut run = |n: usize, pace: &mut Pace, keep: bool| -> anyhow::Result<f64> {
                let started = Instant::now();
                for i in 0..n {
                    let t = i as f32 / 60.0;
                    let bass = 0.5 + 0.4 * (t * 2.0).sin();
                    pace.update(1.0 / 60.0, &levels, bass, 0.6);
                    let p = SceneParams {
                        kind,
                        look: Look::Dark,
                        visualiser,
                        seconds: t,
                        bass,
                        kick: 0.0,
                        level: 0.6,
                        levels: &levels,
                        palette,
                        seed: seed("dQw4w9WgXcQ"),
                        pace,
                        strength: 1.0,
                        quality: 1.0,
                        reaction: 0.5,
                    };
                    if let Some(frame) = scene.frame(&p)?
                        && keep
                    {
                        submit += f64::from(frame.cost.submit);
                        wait += f64::from(frame.cost.wait);
                        copy += f64::from(frame.cost.copy);
                    }
                }
                Ok(started.elapsed().as_secs_f64() * 1000.0 / n as f64)
            };
            run(30, &mut pace, false)?;
            let per = run(frames, &mut pace, true)?;
            let n = frames as f64;
            println!(
                "| {kind:?} | {} | {}x{} | {per:.2} | {:.2} | {:.2} | {:.2} |",
                if visualiser { "visualiser" } else { "backdrop" },
                size.0,
                size.1,
                submit / n,
                wait / n,
                copy / n
            );
        }
    }
    Ok(())
}
