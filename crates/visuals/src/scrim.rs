//! The scrim behind text in the light look (M30). The light look used to
//! press a whole 3D scene into the backdrop's luminance 0.645-0.8 so text
//! kept 4.5:1 on top of it, and the scenes lost their shape. With the
//! scrim on, a scene keeps the visualiser's full tone and only the text
//! blocks the app names get that light range, under a mask that fades
//! into the scene over each block's feather, faster up and down than
//! sideways (`scene_common.wgsl`, `scrim_mask`).
//!
//! The rectangles are in points from the scene's top left, so the mask
//! keeps its size whatever resolution the scene renders at.

/// The most text blocks a scene keeps clear.
pub const SCRIM_BLOCKS: usize = 6;

/// Where the text sits over a scene, and how strongly it is kept clear.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Scrim {
    /// The scrim replaces the whole-scene light tone (the light look with
    /// the setting on). Off: the scene is toned as before.
    pub on: bool,
    /// 1 keeps `text_muted` at 4.5:1 under the text; less lets more of
    /// the scene through, more lays the theme's surface colour over it.
    pub strength: f32,
    /// The scene's size in points.
    pub size: [f32; 2],
    /// Text blocks: left, top, right, bottom in points; unused ones are
    /// empty.
    pub blocks: [[f32; 4]; SCRIM_BLOCKS],
    /// How far each block's light falls off round it, in points: wide
    /// round a lone title, short round a panel with its own background.
    pub feathers: [f32; SCRIM_BLOCKS],
    /// The theme's surface colour, display space, for strengths over 1.
    pub surface: [f32; 3],
}

impl Scrim {
    /// Adds a text block whose light falls off over `feather` points, if
    /// there is room for one more and it isn't empty.
    pub fn add(&mut self, block: [f32; 4], feather: f32) {
        if block[2] <= block[0] || block[3] <= block[1] {
            return;
        }
        if let Some(i) = self.blocks.iter().position(|b| b[2] <= b[0]) {
            self.blocks[i] = block;
            self.feathers[i] = feather.max(1.0);
        }
    }

    /// The shader's `scrim`, `scrim_blocks`, `scrim_feathers` and
    /// `scrim_surface` (10 vec4s).
    pub(crate) fn floats(&self) -> [f32; 40] {
        let mut out = [0.0; 40];
        out[..4].copy_from_slice(&[
            if self.on { 1.0 } else { 0.0 },
            self.strength,
            self.size[0],
            self.size[1],
        ]);
        for (i, b) in self.blocks.iter().enumerate() {
            out[4 + i * 4..8 + i * 4].copy_from_slice(b);
        }
        out[28..28 + SCRIM_BLOCKS].copy_from_slice(&self.feathers);
        out[36..39].copy_from_slice(&self.surface);
        out[39] = 1.0;
        out
    }
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    reason = "GPU regression fixtures require successful frames"
)]
mod tests {
    use super::*;
    use crate::color;
    use crate::{Gpu, Look, Pace, Scene, SceneKind, SceneParams, seed, spectrum::BANDS};

    /// The light theme's `text_muted` (theme.rs: oklch 0.47, 0.014, 285).
    fn text_muted_luminance() -> f32 {
        let (c, h) = (0.014f32, 285f32.to_radians());
        color::luminance(color::oklab_to_linear([0.47, c * h.cos(), c * h.sin()]))
    }

    fn contrast(a: f32, b: f32) -> f32 {
        (a.max(b) + 0.05) / (a.min(b) + 0.05)
    }

    #[test]
    fn blocks_fill_free_places_and_skip_empty_ones() {
        let mut s = Scrim::default();
        s.add([10.0, 10.0, 10.0, 40.0], 100.0);
        assert_eq!(s.blocks[0], [0.0; 4]);
        for i in 0..SCRIM_BLOCKS + 2 {
            s.add([0.0, 0.0, 1.0 + i as f32, 1.0], 100.0);
        }
        assert_eq!(s.blocks[SCRIM_BLOCKS - 1][2], SCRIM_BLOCKS as f32);
        let f = s.floats();
        assert_eq!(&f[4..8], &[0.0, 0.0, 1.0, 1.0]);
        assert_eq!(f[28], 100.0);
    }

    /// Under the scrim the darkest pixel keeps `text_muted` at 4.5:1, in
    /// every scene behind text and in the full window, with a vivid, a
    /// muted and a near-monochrome cover; the scene round it keeps its
    /// range (darker than the old light floor somewhere).
    #[test]
    fn text_keeps_its_contrast_under_the_scrim() {
        let _one = crate::gpu_test_lock();
        let gpu = match Gpu::new() {
            Ok(gpu) => gpu,
            Err(e) => {
                eprintln!("skipped, no GPU: {e:#}");
                return;
            }
        };
        let (w, h) = (320u32, 200u32);
        let mut scene = Scene::new(&gpu, w, h);
        let mut levels = [0.0; BANDS];
        for (i, l) in levels.iter_mut().enumerate() {
            *l = 0.3 + 0.6 * (i as f32 * 0.7).sin().abs();
        }
        let mut pace = Pace::default();
        for _ in 0..90 {
            pace.update(1.0 / 60.0, &levels, 0.7, 0.6);
        }
        let covers = [
            (
                "vivid",
                [
                    [0.85, 0.12, 0.2, 1.0],
                    [0.1, 0.3, 0.9, 1.0],
                    [0.95, 0.75, 0.1, 1.0],
                    [0.05, 0.05, 0.15, 1.0],
                ],
            ),
            (
                "muted",
                [
                    [0.48, 0.55, 0.47, 1.0],
                    [0.7, 0.66, 0.55, 1.0],
                    [0.35, 0.42, 0.46, 1.0],
                    [0.27, 0.3, 0.28, 1.0],
                ],
            ),
            (
                "mono",
                [
                    [0.12, 0.12, 0.13, 1.0],
                    [0.5, 0.5, 0.5, 1.0],
                    [0.85, 0.84, 0.82, 1.0],
                    [0.3, 0.3, 0.31, 1.0],
                ],
            ),
        ];
        // A text block in the middle; the mask is solid over it.
        let block = [90.0, 70.0, 230.0, 130.0];
        let mut scrim = Scrim {
            on: true,
            strength: 1.0,
            size: [w as f32, h as f32],
            surface: [0.99, 0.99, 0.995],
            ..Scrim::default()
        };
        scrim.add(block, 40.0);
        let text = text_muted_luminance();
        let mut worst = f32::MAX;
        for kind in [SceneKind::Xmb, SceneKind::Ridges, SceneKind::Aurora] {
            for (name, palette) in covers {
                for visualiser in [false, true] {
                    let p = SceneParams {
                        kind,
                        look: Look::Light,
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
                        scrim,
                    };
                    let frame = scene.frame_now(&p).expect("frame");
                    let lum = |x: u32, y: u32| {
                        let i = ((y * frame.width + x) * 4) as usize;
                        let px = &frame.bgra[i..i + 4];
                        let rgb = [px[2], px[1], px[0]].map(|v| v as f32 / 255.0);
                        color::luminance(color::to_linear(rgb))
                    };
                    let (mut under, mut around) = (f32::MAX, f32::MAX);
                    for y in 0..h {
                        for x in 0..w {
                            let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
                            let inside = fx >= block[0]
                                && fx <= block[2]
                                && fy >= block[1]
                                && fy <= block[3];
                            let l = lum(x, y);
                            if inside {
                                under = under.min(l);
                            } else if fy < 20.0 || fy > 180.0 {
                                around = around.min(l);
                            }
                        }
                    }
                    let ratio = contrast(under, text);
                    eprintln!(
                        "{kind:?} {name} {}: under the text luminance {under:.3}, \
                         text_muted {ratio:.2}:1; darkest away from it {around:.3}",
                        if visualiser {
                            "full window"
                        } else {
                            "behind text"
                        },
                    );
                    worst = worst.min(ratio);
                    assert!(ratio >= 4.5, "{kind:?} {name} {visualiser}: {ratio:.2}:1");
                }
            }
        }
        eprintln!("worst: {worst:.2}:1");
    }
}
