//! The 3D scenes (M30): the PS3's XMB wave, a flight over ridges made of
//! the spectrum, and an aurora, from the browser spike (spikes/3d-web/,
//! docs/gpui/SPIKE-3D.md). Each fills its frame, opaque, offscreen and
//! read back like the other effects.
//!
//! The shaders share `scene_common.wgsl` (the parameters, noise, and the
//! backdrop's tone mapping, so a scene behind text keeps the backdrop's
//! promise to it) and keep the spike's parameter layout. [`Pace`] keeps
//! what moves them with the music: slow envelopes, and clocks whose speed
//! follows the music (an envelope never scales time directly, which would
//! jump).

use anyhow::Result;

use crate::gpu::Gpu;
use crate::renderer::{Look, colour_kept};
use crate::scrim::Scrim;
use crate::spectrum::BANDS;
use crate::target::{Frame, Target};
use crate::uniform::Uniform;

/// 29 vec4s: audio, output, tune, env, seed, clock, 4 palette colours,
/// 8 of bands, the motion clocks and the scrim's 10 ([`Scrim`]).
const PARAMS_SIZE: u64 = 29 * 16;
/// The XMB wave's grid: 100 x 100 vertices, two triangles a cell.
const XMB_WAVE_VERTICES: u32 = 99 * 99 * 6;
const XMB_SPARKLES: u32 = 2000;

/// Which scene.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SceneKind {
    /// The PS3's XMB wave and its sparkles over a gradient of the cover.
    Xmb,
    /// A slow flight over land whose ridges are the spectrum.
    Ridges,
    /// Curtains of light in the cover's colours over a lake or snow.
    Aurora,
}

/// What moves a scene with the music, kept between frames.
#[derive(Clone, Copy, Debug, Default)]
pub struct Pace {
    /// Slow envelopes, 0..1: bass (about 0.6 s), mids (0.9 s), highs
    /// (0.7 s), level (2 s).
    pub env: [f32; 4],
    /// Fast envelopes, 0..1: the music's energy and its highs over a quiet
    /// floor, quick to rise (0.08 s) and slow to fall (0.7 s).
    pub drive: [f32; 2],
    /// The scene's clock: a little faster while the music is loud.
    pub clock: f64,
    /// The XMB wave's clock: 0.6x when quiet to about 2.8x on a loud hit.
    pub flow: f64,
    /// The XMB sparkles' clock, which runs with the highs.
    pub sparkle: f64,
}

impl Pace {
    /// Moves on by `dt` seconds of the music's `levels` (the analysis's
    /// bands), `bass` and `level`.
    pub fn update(&mut self, dt: f32, levels: &[f32; BANDS], bass: f32, level: f32) {
        let mean = |r: std::ops::Range<usize>| {
            let n = r.len() as f32;
            levels[r].iter().sum::<f32>() / n
        };
        let (mids, highs) = (mean(8..20), mean(20..BANDS));
        let targets = [bass, mids, highs, level];
        let taus = [0.6, 0.9, 0.7, 2.0];
        for ((env, target), tau) in self.env.iter_mut().zip(targets).zip(taus) {
            *env += (target - *env) * (1.0 - (-dt / tau).exp());
        }
        let drives = [
            ((level.max(bass * 0.85) - 0.2) / 0.55).clamp(0.0, 1.0),
            ((highs - 0.15) / 0.5).clamp(0.0, 1.0),
        ];
        for (drive, target) in self.drive.iter_mut().zip(drives) {
            let tau = if target > *drive { 0.08 } else { 0.7 };
            *drive += (target - *drive) * (1.0 - (-dt / tau).exp());
        }
        let dt = f64::from(dt);
        self.clock += dt * f64::from(0.85 + 0.3 * self.env[3]);
        self.flow += dt * f64::from(0.6 + 2.2 * self.drive[0]);
        self.sparkle += dt * f64::from(0.6 + 2.4 * self.drive[1]);
    }

    /// Starts the clocks again from 0, keeping the envelopes: for a new
    /// song, whose seed changes the whole scene at once anyway (so the
    /// jump doesn't show), and which keeps what the shaders get small.
    pub fn restart_clocks(&mut self) {
        self.clock = 0.0;
        self.flow = 0.0;
        self.sparkle = 0.0;
    }

    /// The clocks as the shaders get them: `[clock, ridges' rows, ridges'
    /// camera z, flow, sparkle]`. Ridges' camera flies `RIDGES_SPEED` a
    /// second of the clock from `RIDGES_START`; it goes over as whole ridge
    /// rows (`spacing`, from the seed as `ridge_look` has it) and the
    /// camera's z within them, so the shader's line widths and normals
    /// work on small numbers however far it has flown.
    pub fn gpu_clocks(&self, seed: &[f32; 4]) -> [f32; 5] {
        let spacing = f64::from(0.7 + 0.25 * seed[0]);
        let travelled = self.clock * RIDGES_SPEED + RIDGES_START;
        let rows = (travelled / spacing).floor();
        let z = -(travelled - rows * spacing);
        [
            self.clock as f32,
            rows as f32,
            z as f32,
            self.flow as f32,
            self.sparkle as f32,
        ]
    }
}

/// Ridges' camera speed along -z, per second of the scene's clock, and
/// where it starts (`scene_ridges.wgsl`).
const RIDGES_SPEED: f64 = 0.22;
const RIDGES_START: f64 = 30.0;

/// Four values 0..1 from a video id, the same for the same id (FNV-1a,
/// then PCG steps; the spike's `seed` in cover.js gives the same numbers).
pub fn seed(id: &str) -> [f32; 4] {
    let mut h: u32 = 0x811c_9dc5;
    for c in id.chars() {
        h ^= c as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    let mut out = [0.0; 4];
    for v in &mut out {
        h = h.wrapping_mul(747_796_405).wrapping_add(2_891_336_453);
        let w = ((h >> ((h >> 28) + 4)) ^ h).wrapping_mul(277_803_737);
        *v = ((w >> 22) ^ w) as f32 / u32::MAX as f32;
    }
    out
}

/// Inputs for one frame.
#[derive(Clone, Copy, Debug)]
pub struct SceneParams<'a> {
    pub kind: SceneKind,
    pub look: Look,
    /// Full strength (the full-window visualiser) rather than toned down
    /// behind text (Now Playing, Stage).
    pub visualiser: bool,
    pub seconds: f32,
    pub bass: f32,
    pub kick: f32,
    pub level: f32,
    pub levels: &'a [f32; BANDS],
    /// The cover's four colours, display space.
    pub palette: [[f32; 4]; 4],
    /// From [`seed`] of the video id.
    pub seed: [f32; 4],
    pub pace: &'a Pace,
    /// How far the scene stands out from its own haze, 1 for the default.
    pub strength: f32,
    /// The raymarched scenes' step count, 1 for this GPU's default (lower
    /// on Adreno and GL, [`Gpu::scene_detail`]).
    pub quality: f32,
    /// How much a scene behind text follows the music, 0..1 (0.5 as
    /// designed); the visualiser follows all of it.
    pub reaction: f32,
    /// The light look's scrim behind text (off: the old whole-scene tone).
    pub scrim: Scrim,
}

pub struct Scene {
    gpu: Gpu,
    uniforms: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    target: Target,
}

impl Scene {
    pub fn new(gpu: &Gpu, width: u32, height: u32) -> Self {
        let uniforms = gpu.uniforms("scene params", PARAMS_SIZE);
        let bind_group = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("scene"),
            layout: &gpu.pipelines.scenes.layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniforms.as_entire_binding(),
            }],
        });
        Self {
            gpu: gpu.clone(),
            uniforms,
            bind_group,
            target: Target::new(gpu, width, height),
        }
    }

    pub fn size(&self) -> (u32, u32) {
        self.target.size()
    }

    /// Draws `width`×`height` frames from now on; the next frame shows
    /// nothing yet.
    pub fn resize(&mut self, width: u32, height: u32) {
        self.target.resize(&self.gpu, width, height);
    }

    /// Forgets the frame in flight (the scene was hidden).
    pub fn discard(&mut self) {
        self.target.discard();
    }

    /// Renders a frame and returns the one rendered on the previous call.
    pub fn frame(&mut self, params: &SceneParams<'_>) -> Result<Option<Frame>> {
        let uniforms = self.params_bytes(params);
        self.gpu.queue.write_buffer(&self.uniforms, 0, &uniforms);
        let (gpu, group) = (&self.gpu, &self.bind_group);
        self.target
            .frame(gpu, |pass| draw(gpu, group, params.kind, pass))
    }

    /// Renders a frame and waits for it.
    pub fn frame_now(&mut self, params: &SceneParams<'_>) -> Result<Frame> {
        let uniforms = self.params_bytes(params);
        self.gpu.queue.write_buffer(&self.uniforms, 0, &uniforms);
        let (gpu, group) = (&self.gpu, &self.bind_group);
        self.target
            .frame_now(gpu, |pass| draw(gpu, group, params.kind, pass))
    }

    fn params_bytes(&self, p: &SceneParams<'_>) -> [u8; PARAMS_SIZE as usize] {
        let flag = |on: bool| if on { 1.0 } else { 0.0 };
        let (width, height) = self.target.size();
        let pace = p.pace;
        let mut floats = Uniform::new();
        floats.extend([p.seconds, p.bass, p.kick, p.level]);
        floats.extend([
            width as f32,
            height as f32,
            flag(p.look == Look::Light),
            flag(p.visualiser),
        ]);
        // Behind text the envelopes scale with the reaction (as designed
        // at 0.5); the XMB reads the reaction itself for its clocks.
        let follow = if p.visualiser { 1.0 } else { p.reaction };
        // The detail setting is relative to what this GPU draws by default.
        let quality = p.quality * self.gpu.scene_detail();
        floats.extend([p.strength, colour_kept(&p.palette), quality, follow]);
        let scale = if p.visualiser { 1.0 } else { 2.0 * p.reaction };
        floats.extend(pace.env.map(|e| (e * scale).min(1.0)));
        floats.extend(p.seed);
        let [clock, rows, z, flow, sparkle] = pace.gpu_clocks(&p.seed);
        floats.extend([clock, rows, z, flag(self.gpu.modest())]);
        for colour in &p.palette {
            floats.extend([colour[0], colour[1], colour[2], 1.0]);
        }
        floats.extend(p.levels.iter().copied());
        floats.extend([flow, sparkle, pace.drive[0], pace.drive[1]]);
        floats.extend(p.scrim.floats());
        floats.finish()
    }
}

/// The scene's draws, in order.
fn draw(gpu: &Gpu, group: &wgpu::BindGroup, kind: SceneKind, pass: &mut wgpu::RenderPass<'_>) {
    let scenes = &gpu.pipelines.scenes;
    pass.set_bind_group(0, group, &[]);
    match kind {
        SceneKind::Xmb => {
            let [background, wave, sparkles] = &scenes.xmb;
            pass.set_pipeline(background);
            pass.draw(0..3, 0..1);
            pass.set_pipeline(wave);
            pass.draw(0..XMB_WAVE_VERTICES, 0..1);
            pass.set_pipeline(sparkles);
            pass.draw(0..6, 0..XMB_SPARKLES);
        }
        SceneKind::Ridges => {
            pass.set_pipeline(&scenes.ridges);
            pass.draw(0..3, 0..1);
        }
        SceneKind::Aurora => {
            pass.set_pipeline(&scenes.aurora);
            pass.draw(0..3, 0..1);
        }
    }
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    reason = "GPU regression fixtures require successful frames"
)]
mod tests {
    use super::*;

    /// 48 hours of songs four minutes long at 30 frames a second: what the
    /// shaders get stays small, its frame steps stay those of the f64
    /// clocks, and Ridges' camera stays within a row of its origin.
    #[test]
    fn the_clocks_stay_precise_for_days() {
        let mut pace = Pace::default();
        let seed = seed("dQw4w9WgXcQ");
        let spacing = 0.7 + 0.25 * seed[0];
        let dt = 1.0 / 30.0;
        let frames_a_song = 4 * 60 * 30;
        let mut largest = 0.0f32;
        for frame in 0..48 * 3600 * 30 {
            if frame % frames_a_song == 0 {
                pace.restart_clocks();
            }
            // Loud on every other song, so the clocks run at their fastest.
            let loud = if (frame / frames_a_song) % 2 == 0 {
                0.9
            } else {
                0.1
            };
            let before = [pace.clock, pace.flow, pace.sparkle];
            let sent = pace.gpu_clocks(&seed);
            pace.update(dt, &[loud; BANDS], loud, loud);
            let now = pace.gpu_clocks(&seed);
            let after = [pace.clock, pace.flow, pace.sparkle];
            for (k, i) in [0, 3, 4].into_iter().enumerate() {
                let step = f64::from(now[i] - sent[i]);
                let exact = after[k] - before[k];
                assert!((step - exact).abs() < 2e-4, "clock {i}: {step} for {exact}");
            }
            assert!(now[2] <= 0.0 && now[2] > -spacing - 1e-4, "z {}", now[2]);
            assert_eq!(now[1], now[1].round());
            largest = largest.max(now[0]).max(now[1]).max(now[3]).max(now[4]);
        }
        assert!(largest < 1e4, "{largest}");
    }

    /// Loud music moves the envelopes up and runs the clocks faster than
    /// silence does; no clock ever goes back.
    #[test]
    fn the_pace_follows_the_music() {
        let (mut quiet, mut loud) = (Pace::default(), Pace::default());
        let dt = 1.0 / 60.0;
        for _ in 0..120 {
            let (q, l) = ((quiet.flow, quiet.clock), (loud.flow, loud.clock));
            quiet.update(dt, &[0.0; BANDS], 0.0, 0.0);
            loud.update(dt, &[0.8; BANDS], 0.8, 0.8);
            assert!(quiet.flow > q.0 && quiet.clock > q.1);
            assert!(loud.flow > l.0 && loud.clock > l.1);
        }
        assert!(loud.env.iter().all(|&e| e > 0.5), "{:?}", loud.env);
        assert!(loud.drive.iter().all(|&d| d > 0.9), "{:?}", loud.drive);
        assert!(loud.flow > quiet.flow * 3.0);
        assert!(loud.sparkle > quiet.sparkle * 3.0);
        // Back to silence: the fast envelope falls within a couple of
        // seconds, the slow one is still on its way down.
        for _ in 0..120 {
            loud.update(dt, &[0.0; BANDS], 0.0, 0.0);
        }
        assert!(loud.drive[0] < 0.1, "{:?}", loud.drive);
        assert!(
            loud.env[3] > 0.1 && loud.env[3] > loud.drive[0] * 3.0,
            "{:?}",
            loud.env
        );
    }

    /// The same id gives the same seed, as the spike's cover.js gives it;
    /// another id another.
    #[test]
    fn seeds_are_stable() {
        let a = seed("dQw4w9WgXcQ");
        assert_eq!(a, seed("dQw4w9WgXcQ"));
        assert_ne!(a, seed("kJQP7kiw5Fk"));
        assert!(a.iter().all(|v| (0.0..=1.0).contains(v)));
    }

    /// Every scene draws a lit, opaque frame with music and without.
    #[test]
    fn every_scene_draws() {
        let _one = crate::gpu_test_lock();
        let gpu = match Gpu::new() {
            Ok(gpu) => gpu,
            Err(e) => {
                eprintln!("skipped, no GPU: {e:#}");
                return;
            }
        };
        let mut scene = Scene::new(&gpu, 160, 100);
        let mut levels = [0.0; BANDS];
        for (i, l) in levels.iter_mut().enumerate() {
            *l = 0.3 + 0.6 * (i as f32 * 0.7).sin().abs();
        }
        let mut pace = Pace::default();
        for _ in 0..60 {
            pace.update(1.0 / 60.0, &levels, 0.7, 0.6);
        }
        let palette = [
            [0.8, 0.3, 0.25, 1.0],
            [0.95, 0.6, 0.3, 1.0],
            [0.25, 0.35, 0.75, 1.0],
            [0.4, 0.2, 0.5, 1.0],
        ];
        for kind in [SceneKind::Xmb, SceneKind::Ridges, SceneKind::Aurora] {
            for (visualiser, look) in [
                (true, Look::Dark),
                (false, Look::Dark),
                (false, Look::Light),
            ] {
                let p = SceneParams {
                    kind,
                    look,
                    visualiser,
                    seconds: 30.0,
                    bass: 0.7,
                    kick: 0.2,
                    level: 0.6,
                    levels: &levels,
                    palette,
                    seed: seed("dQw4w9WgXcQ"),
                    pace: &pace,
                    strength: 1.0,
                    quality: 1.0,
                    reaction: 0.5,
                    scrim: Scrim::default(),
                };
                let frame = scene.frame_now(&p).expect("frame");
                let px = frame.bgra.chunks(4);
                let opaque = px.clone().all(|p| p[3] == 255);
                let lit = px.filter(|p| p[0].max(p[1]).max(p[2]) > 4).count();
                assert!(opaque, "{kind:?} {look:?}: not opaque");
                assert!(lit > 160 * 100 / 2, "{kind:?} {look:?}: {lit} lit pixels");
            }
        }
    }
}
