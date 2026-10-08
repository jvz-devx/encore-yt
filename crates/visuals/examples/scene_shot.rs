//! Writes frames of a 3D scene (M30) as the app renders them, for looking
//! at them closely: a run of frames at 60 a second from a moment, each as
//! a binary PPM (`magick`/`sips` convert it).
//!
//!     cargo run -p encore-visuals --release --example scene_shot -- \
//!         <xmb|ridges|aurora> <width> <height> <out dir> \
//!         [seconds=30] [frames=1] [visualiser|backdrop] [dark|light]
//!
//! The music is a fixed spectrum leaning to the bass, with a slow swell.

use std::io::Write;

use encore_visuals::{BANDS, Gpu, Look, Pace, Scene, SceneKind, SceneParams, seed};

#[allow(
    clippy::print_stdout,
    reason = "command-line example output is intentional; CODE-QUALITY.md V4"
)]
fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let arg = |i: usize, or: &str| args.get(i).cloned().unwrap_or_else(|| or.to_string());
    let kind = match arg(0, "ridges").as_str() {
        "xmb" => SceneKind::Xmb,
        "aurora" => SceneKind::Aurora,
        _ => SceneKind::Ridges,
    };
    let (width, height): (u32, u32) = (arg(1, "1536").parse()?, arg(2, "984").parse()?);
    let out = std::path::PathBuf::from(arg(3, "."));
    let start: f32 = arg(4, "30").parse()?;
    let frames: usize = arg(5, "1").parse()?;
    let visualiser = arg(6, "visualiser") == "visualiser";
    let look = if arg(7, "dark") == "light" {
        Look::Light
    } else {
        Look::Dark
    };
    std::fs::create_dir_all(&out)?;
    let gpu = Gpu::new()?;
    let mut scene = Scene::new(&gpu, width, height);
    let mut pace = Pace::default();
    let palette = [
        [0.48, 0.55, 0.47, 1.0],
        [0.7, 0.66, 0.55, 1.0],
        [0.35, 0.42, 0.46, 1.0],
        [0.27, 0.3, 0.28, 1.0],
    ];
    let dt = 1.0 / 60.0;
    // Run up to the moment so the envelopes and clocks are where the app's
    // would be.
    let steps = (start / dt) as usize;
    let levels_at = |t: f32| {
        let mut levels = [0.0; BANDS];
        for (i, l) in levels.iter_mut().enumerate() {
            let x = i as f32 / BANDS as f32;
            *l = (0.8 - 0.45 * x + 0.12 * (x * 9.0 + t * 0.7).sin()).clamp(0.0, 1.0);
        }
        levels
    };
    for i in 0..steps {
        let t = i as f32 * dt;
        pace.update(dt, &levels_at(t), 0.6 + 0.2 * (t * 0.5).sin(), 0.6);
    }
    for f in 0..frames {
        let t = start + f as f32 * dt;
        let levels = levels_at(t);
        pace.update(dt, &levels, 0.6 + 0.2 * (t * 0.5).sin(), 0.6);
        let p = SceneParams {
            kind,
            look,
            visualiser,
            seconds: t,
            bass: 0.6,
            kick: 0.0,
            level: 0.6,
            levels: &levels,
            palette,
            seed: seed("dQw4w9WgXcQ"),
            pace: &pace,
            strength: 1.0,
            quality: 1.0,
            reaction: 0.5,
            scrim: Default::default(),
        };
        let frame = scene.frame_now(&p)?;
        let path = out.join(format!("{f:04}.ppm"));
        let mut file = std::io::BufWriter::new(std::fs::File::create(&path)?);
        write!(file, "P6\n{} {}\n255\n", frame.width, frame.height)?;
        for px in frame.bgra.chunks(4) {
            file.write_all(&[px[2], px[1], px[0]])?;
        }
    }
    println!("{frames} frames in {}", out.display());
    Ok(())
}
