//! The backdrop's sparkles (after the PS3's XrossMediaBar): fine points of
//! light, mostly a pixel or two across, drifting slowly in three depths and
//! gathering loosely round the backdrop's light wave.
//!
//! They are GPUI quads painted over the backdrop's frame, not a shader
//! frame: a few hundred small quads cost the GPU next to nothing, need no
//! read-back or upload, sit on the window's own pixels at any scale and
//! move with every window frame, so they stay fluid while the blurred
//! backdrop under them changes more slowly. Each sparkle is one per grid
//! cell from a hash of its cell, which stays put while the grid drifts:
//! nothing pops or shimmers. They twinkle over seconds and the music only
//! lifts their brightness a little (a smoothed level, never the kick).

use std::f32::consts::TAU;
use std::time::Instant;

use encore_visuals::Look;
use gpui_kit::component::Colorize as _;
use gpui_kit::*;

use super::config::{self, ParticleColour};

/// The sparkles' clocks: drift and twinkle, each at its own speed, so a
/// speed changes without a jump. f64, and only used on the CPU (in f64
/// where they meet `sin` or the grid), so they stay precise for good.
#[derive(Default)]
pub struct Clocks {
    drift: f64,
    twinkle: f64,
    last: Option<Instant>,
}

impl Clocks {
    /// Moves the clocks on while the sparkles move (music plays, motion
    /// isn't reduced); they hold still otherwise.
    pub fn advance(&mut self, moving: bool) {
        let now = Instant::now();
        if moving {
            if let Some(last) = self.last {
                let dt = (now - last).as_secs_f64().min(0.1);
                let p = &config::get().particles;
                self.drift += dt * f64::from(p.speed);
                self.twinkle += dt * f64::from(p.twinkle_speed);
            }
            self.last = Some(now);
        } else {
            self.last = None;
        }
    }
}

/// What a frame's sparkles need, captured for painting.
#[derive(Clone)]
pub struct Field {
    drift: f64,
    twinkle: f64,
    wave_clock: f32,
    /// The music's smoothed level, 0..1.
    level: f32,
    look: Look,
    /// The cover's palette (display RGB) and the theme's accent.
    palette: [[f32; 4]; 4],
    accent: Hsla,
}

impl Field {
    /// The sparkles to paint now, or `None` while they are off.
    pub fn new(
        clocks: &Clocks,
        wave_clock: f32,
        level: f32,
        look: Look,
        palette: [[f32; 4]; 4],
        accent: Hsla,
    ) -> Option<Self> {
        let p = &config::get().particles;
        (p.on && p.amount > 0.).then_some(Self {
            drift: clocks.drift,
            twinkle: clocks.twinkle,
            wave_clock,
            level,
            look,
            palette,
            accent,
        })
    }

    /// Paints the sparkles over `area` (window coordinates), clipped to it.
    ///
    /// In one layer: each quad then takes the layer's place in the scene's
    /// order instead of a search and an insert of its own in GPUI's bounds
    /// tree, which was most of what a sparkle cost the CPU (and made every
    /// view painted after them search a tree a few hundred leaves larger).
    pub fn paint(&self, area: Bounds<Pixels>, window: &mut Window) {
        let config = config::get();
        let scale = window.scale_factor();
        let tints = self.tints(config.particles.colour);
        window.with_content_mask(Some(ContentMask { bounds: area }), |window| {
            window.paint_layer(area, |window| {
                for layer in 0..3 {
                    self.layer(layer, area, scale, &config, &tints, window);
                }
            });
        });
    }

    /// One depth's sparkles: 0 far (smaller, fainter, slower), 2 near.
    fn layer(
        &self,
        layer: u32,
        area: Bounds<Pixels>,
        scale: f32,
        config: &config::VisualsConfig,
        tints: &[Hsla; 4],
        window: &mut Window,
    ) {
        let p = &config.particles;
        let wave = &config.wave;
        let light = self.look == Look::Light;
        // White shows less on the light look's pale backdrop.
        let boost = if light { 2. } else { 1. };
        let near = 0.5 + (layer as f32 * 0.5 - 0.5) * p.depth;
        // Cell size in points, drift in points a second.
        // About 300 over Now Playing's panel at amount 1, each a quad (two
        // with a halo) painted in every window frame.
        let cell = 44. + 28. * near;
        let speed = 3. + 7. * near;
        let angle = f64::from(p.direction.to_radians());
        let drifted = |d: f64| d * self.drift * f64::from(speed);
        // Whole cells the grid has drifted (they pick the cells' hashes),
        // and the rest, under a cell: what places the cells.
        let (whole, offset) = {
            let cell = f64::from(cell);
            let (x, y) = (drifted(angle.cos()), drifted(-angle.sin()));
            let (wx, wy) = ((x / cell).floor(), (y / cell).floor());
            (
                (wx as i64, wy as i64),
                ((x - wx * cell) as f32, (y - wy * cell) as f32),
            )
        };
        let (w, h) = (f32::from(area.size.width), f32::from(area.size.height));
        let threshold = 1. - 0.4 * p.amount;
        let lift = 1. + 0.5 * p.reaction * self.level;
        let cells_x =
            ((-offset.0 / cell).floor() as i32)..=(((w - offset.0) / cell).floor() as i32);
        for cx in cells_x {
            let rows =
                ((-offset.1 / cell).floor() as i32)..=(((h - offset.1) / cell).floor() as i32);
            for cy in rows {
                let r = hash4(
                    (i64::from(cx) - whole.0) as i32,
                    (i64::from(cy) - whole.1) as i32,
                    layer,
                );
                if r[3] < threshold {
                    continue;
                }
                let t = self.twinkle;
                let turn = |speed: f64, at: f32| t * speed + f64::from(at * TAU);
                let sway = (
                    turn(0.21, r[2]).sin() as f32 * 0.07,
                    turn(0.17, r[0]).cos() as f32 * 0.07,
                );
                let x = (cx as f32 + 0.25 + 0.5 * r[0] + sway.0) * cell + offset.0;
                let y = (cy as f32 + 0.25 + 0.5 * r[1] + sway.1) * cell + offset.1;
                // One in fifteen a little larger.
                let big = if r[2] > 0.93 { 1.5 } else { 1. };
                let radius_px = (p.size_min + (p.size_max - p.size_min) * near) * big;
                let phase = t * f64::from(0.5 + 0.6 * r[1]) + f64::from(r[3] * 40.);
                let twinkle = 1. + (0.2 + 0.8 * smooth(phase.sin() as f32) - 1.) * p.twinkle;
                let gather = if wave.on && wave.strength > 0. {
                    0.3 + 0.7 * near_wave(x / w, y / h, self.wave_clock, wave)
                } else {
                    0.65
                };
                let alpha =
                    ((0.28 + 0.4 * near) * twinkle * gather * lift * p.brightness * boost).min(0.8);
                if alpha < 0.03 {
                    continue;
                }
                Sparkle {
                    at: point(area.origin.x + px(x), area.origin.y + px(y)),
                    radius: px(radius_px.max(0.5) / scale),
                    alpha,
                    // Only the near ones glow: a halo is a second quad.
                    softness: if near > 0.6 || big > 1. {
                        p.softness
                    } else {
                        0.
                    },
                    color: tints[((r[0] * 4.) as usize).min(3)],
                }
                .paint(scale, window);
            }
        }
    }

    /// The four colours a sparkle picks from (by its cell's hash): white,
    /// or a little of the cover's colours or the accent. Mixed once a frame,
    /// not per sparkle.
    fn tints(&self, colour: ParticleColour) -> [Hsla; 4] {
        let white: Hsla = gpui_kit::white();
        // `mix_oklab` weighs the first colour: mostly white.
        let weight = if self.look == Look::Light { 0.85 } else { 0.7 };
        std::array::from_fn(|i| match colour {
            ParticleColour::White => white,
            ParticleColour::Cover => {
                let [r, g, b, _] = self.palette[i];
                white.mix_oklab(Rgba { r, g, b, a: 1. }.into(), weight)
            }
            ParticleColour::Accent => white.mix_oklab(self.accent, weight),
        })
    }
}

/// One sparkle: a soft point of light.
struct Sparkle {
    at: Point<Pixels>,
    radius: Pixels,
    alpha: f32,
    softness: f32,
    color: Hsla,
}

/// Halos wider than this (device pixels) are blurred instead of flat. The
/// default sizes stay under it (at most about 4.6), where a flat disc reads
/// as a soft point; the larger sizes Settings offers would show its edge.
const FLAT_HALO_PX: f32 = 5.;

impl Sparkle {
    /// A halo (as soft as the setting) under a core.
    fn paint(&self, scale: f32, window: &mut Window) {
        let dot = |r: Pixels, alpha: f32, window: &mut Window| {
            let bounds = Bounds::new(self.at - point(r, r), size(r * 2., r * 2.));
            window.paint_quad(fill(bounds, self.color.opacity(alpha)).corner_radii(r));
        };
        if self.softness > 0. {
            let halo = self.radius * (1. + 1.5 * self.softness);
            let alpha = self.alpha * 0.3 * self.softness;
            if f32::from(halo) * scale <= FLAT_HALO_PX {
                dot(halo, alpha, window);
            } else {
                // A disc of half the halo's radius blurred over most of
                // the rest: about the flat disc's light, fading to nothing
                // a little past its edge.
                let r = halo * 0.5;
                let bounds = Bounds::new(self.at - point(r, r), size(r * 2., r * 2.));
                window.paint_drop_shadows(
                    bounds,
                    Corners::all(r),
                    &[BoxShadow {
                        color: self.color.opacity((alpha * 2.5).min(1.)),
                        offset: point(px(0.), px(0.)),
                        blur_radius: halo * 0.4,
                        spread_radius: px(0.),
                        inset: false,
                    }],
                );
            }
        }
        dot(self.radius * (1. - 0.3 * self.softness), self.alpha, window);
    }
}

/// The wave's ribbon `i` at `x` (0..1 across): its middle, in heights from
/// the top, as `backdrop.wgsl`'s `ribbon` draws it.
fn ribbon(i: u32, x: f32, t: f32, height: f32) -> f32 {
    let k = i as f32;
    let tau = std::f32::consts::TAU;
    let a = (tau * (x * (0.55 + 0.12 * k) + 0.23 * k) + t * (0.09 + 0.025 * k)).sin();
    let b = (tau * (x * (1.15 - 0.2 * k) - 0.31 * k) - t * (0.05 + 0.02 * k)).sin();
    height + 0.03 * (k - 1.) + (0.1 - 0.02 * k) * a + 0.04 * b
}

/// How close a point (in fractions of the area) is to the wave, 0..1.
fn near_wave(x: f32, y: f32, t: f32, wave: &config::Wave) -> f32 {
    let d = (0..wave.ribbons.clamp(1, 3))
        .map(|i| (y - ribbon(i, x, t, wave.height)).abs())
        .fold(1., f32::min);
    (-(d * d) / 0.03).exp()
}

/// -1..1 eased into 0..1.
fn smooth(v: f32) -> f32 {
    let t = ((v + 1.) * 0.5).clamp(0., 1.);
    t * t * (3. - 2. * t)
}

/// Four numbers in 0..1 for a cell, the same every frame.
fn hash4(x: i32, y: i32, layer: u32) -> [f32; 4] {
    let mut h = (x as u32).wrapping_mul(0x8da6_b343)
        ^ (y as u32).wrapping_mul(0xd816_3841)
        ^ layer.wrapping_add(1).wrapping_mul(0xcb1a_b31f);
    std::array::from_fn(|_| {
        h = h.wrapping_add(0x9e37_79b9);
        let mut z = h;
        z = (z ^ (z >> 16)).wrapping_mul(0x85eb_ca6b);
        z = (z ^ (z >> 13)).wrapping_mul(0xc2b2_ae35);
        z ^= z >> 16;
        z as f32 / u32::MAX as f32
    })
}
