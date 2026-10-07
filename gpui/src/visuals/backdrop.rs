//! The animated cover backdrop over the page panel while Now Playing shows
//! (M8): `ytfast_visuals::Renderer` frames, painted under the app.

use std::time::{Duration, Instant};

use gpui_kit::component::Colorize as _;
use gpui_kit::*;
use ytfast_visuals::{Cover, CoverShadow, FrameCost, FrameParams, Renderer, Tune};

use super::effects::Tick;
use super::frames::Frames;
use crate::theme::Colors;

/// The backdrop renders at `SCALE` of the panel's size, at most this wide;
/// GPUI scales it to the panel. A blurred image and soft motes look the
/// same at 0.4 as at 0.5, for 36% fewer pixels of a costly shader (about
/// 2% of the GPU at 10 frames a second on the UHD 630).
const MAX_WIDTH: f32 = 512.;
const SCALE: f32 = 0.4;

#[derive(Default)]
pub struct Backdrop {
    renderer: Option<Renderer>,
    frames: Frames,
    /// The cover uploaded, and its palette for the fallback gradient.
    cover: Option<SharedString>,
    palette: Option<[[f32; 4]; 4]>,
    /// The cover shadow last drawn.
    shadow: Option<CoverShadow>,
    /// The flow's clock (it runs at the swirl speed), and the animation
    /// time it last moved with.
    flow: f32,
    seconds: f32,
    /// The settings' revision last drawn with.
    revision: u64,
    /// Frames still to render although nothing moves (a new cover or size
    /// under reduced motion or while paused; the first frame shows a frame
    /// late).
    pending: u8,
    stats: Stats,
}

impl Backdrop {
    pub fn image(&self) -> Option<std::sync::Arc<RenderImage>> {
        self.frames.image()
    }

    pub fn has_renderer(&self) -> bool {
        self.renderer.is_some()
    }

    /// A cover is still fading in: frames should keep coming even while
    /// paused.
    pub fn fading(&self) -> bool {
        self.renderer.as_ref().is_some_and(Renderer::fading)
    }

    pub fn pending(&self) -> bool {
        self.pending > 0
    }

    /// The look or reduced motion changed: draw the still picture again.
    pub fn redraw(&mut self) {
        self.pending = self.pending.max(2);
    }

    pub fn forget(&mut self) {
        self.frames.forget();
        self.pending = 2;
    }

    pub fn release(&mut self, cx: &mut App) {
        log::info!("visuals: releasing the backdrop renderer");
        self.renderer = None;
        self.cover = None;
        self.frames.clear(cx);
    }

    /// Uploads `cover` if it is new, then renders the next frame if one is
    /// due or the still picture changed.
    pub fn update(
        &mut self,
        tick: &Tick,
        panel: Bounds<Pixels>,
        cover: Option<(&SharedString, &Cover)>,
        shadow: Option<Shadow>,
        window: &mut Window,
    ) {
        let size = render_size(panel);
        let shadow = shadow.map(|s| s.in_frame(panel, size));
        if shadow != self.shadow {
            self.shadow = shadow;
            self.pending = self.pending.max(1);
        }
        let renderer = self.renderer.get_or_insert_with(|| {
            log::info!("visuals: backdrop {}x{}", size.0, size.1);
            let started = std::time::Instant::now();
            let renderer = Renderer::new(tick.gpu, size.0, size.1);
            super::timing::setup(started);
            renderer
        });
        if renderer.size() != size {
            renderer.resize(size.0, size.1);
            self.pending = self.pending.max(2);
        }
        if let Some((url, art)) = cover
            && self.cover.as_ref() != Some(url)
        {
            let fade = self.cover.is_some() && !tick.reduce;
            renderer.set_cover(art, fade);
            self.palette = Some(art.palette);
            self.cover = Some(url.clone());
            self.pending = self.pending.max(2);
        }
        let config = super::config::get();
        let b = &config.backdrop;
        if config_changed(&mut self.revision) {
            self.pending = self.pending.max(2);
        }
        self.flow += (tick.seconds - self.seconds).max(0.) * b.swirl;
        self.seconds = tick.seconds;
        if !tick.due && self.pending == 0 {
            return;
        }
        let params = FrameParams {
            seconds: tick.seconds,
            bass: (tick.bass * b.bass_pulse).min(1.5),
            kick: (tick.kick * b.bass_pulse).min(1.5),
            level: tick.level,
            look: tick.look,
            particles: b.motes && !tick.reduce && !super::effects::skip("particles"),
            shadow: self.shadow,
            flow: self.flow,
            tune: Tune {
                blur: b.blur,
                motes: b.motes_amount,
                mote_size: b.mote_size,
                bloom: b.bloom,
                intensity: b.intensity,
            },
        };
        match renderer.frame(&params) {
            Ok(Some(frame)) => {
                self.pending = self.pending.saturating_sub(1);
                let cost = frame.cost;
                self.frames.push(frame, window);
                self.stats.record(cost, size);
            }
            Ok(None) => {}
            Err(e) => log::warn!("visuals: frame: {e:#}"),
        }
    }

    /// The cover's first colour, faded into the surface: what shows before
    /// the first frame, or without a GPU device.
    pub fn fallback(&self, c: &Colors) -> Background {
        let Some(palette) = self.palette else {
            return c.surface.into();
        };
        let [r, g, b, _] = palette[0];
        let tint = c.surface.mix_oklab(Rgba { r, g, b, a: 1. }.into(), 0.75);
        linear_gradient(
            160.,
            linear_color_stop(tint, 0.),
            linear_color_stop(c.surface, 1.),
        )
    }

    /// Remembers the cover for the fallback gradient when there is no GPU.
    pub fn set_fallback(&mut self, palette: [[f32; 4]; 4]) {
        self.palette = Some(palette);
    }
}

/// Whether the settings changed since `revision` (which is moved on).
pub fn config_changed(revision: &mut u64) -> bool {
    let now = super::config::revision();
    std::mem::replace(revision, now) != now
}

/// The large cover's drop shadow, which the backdrop draws in place of
/// GPUI's (`theme::elevation::high`).
#[derive(Clone, Copy, Debug)]
pub struct Shadow {
    /// The cover, in window coordinates.
    pub cover: Bounds<Pixels>,
    pub radius: Pixels,
    /// The theme's shadow colour's alpha.
    pub opacity: f32,
}

impl Shadow {
    /// In the coordinates of a frame of `size` painted cover-fit over
    /// `panel` (as the effects layer paints it).
    fn in_frame(self, panel: Bounds<Pixels>, size: (u32, u32)) -> CoverShadow {
        let (w, h) = (size.0 as f32, size.1 as f32);
        let (pw, ph) = (f32::from(panel.size.width), f32::from(panel.size.height));
        // Window pixels per frame pixel.
        let scale = (pw / w).max(ph / h);
        let left = f32::from(panel.center().x) - w * scale / 2.;
        let top = f32::from(panel.center().y) - h * scale / 2.;
        let at =
            |x: Pixels, y: Pixels| [(f32::from(x) - left) / scale, (f32::from(y) - top) / scale];
        let [l, t] = at(self.cover.left(), self.cover.top());
        let [r, b] = at(self.cover.right(), self.cover.bottom());
        CoverShadow {
            rect: [l, t, r, b],
            radius: f32::from(self.radius) / scale,
            scale: 1. / scale,
            opacity: self.opacity,
        }
    }
}

/// The render size for a panel: `SCALE` of its size, at most `MAX_WIDTH` wide,
/// rounded to 16 px so small resizes don't re-make the targets.
fn render_size(panel: Bounds<Pixels>) -> (u32, u32) {
    let (w, h) = (f32::from(panel.size.width), f32::from(panel.size.height));
    let scale = (MAX_WIDTH / w).min(SCALE);
    let round = |v: f32| ((v * scale / 16.).round().max(1.) * 16.) as u32;
    (round(w), round(h))
}

/// Frame costs, logged every five seconds while animating.
#[derive(Default)]
struct Stats {
    frames: u32,
    cost: FrameCost,
    since: Option<Instant>,
}

impl Stats {
    fn record(&mut self, cost: FrameCost, size: (u32, u32)) {
        let since = *self.since.get_or_insert_with(Instant::now);
        self.frames += 1;
        self.cost.submit += cost.submit;
        self.cost.wait += cost.wait;
        self.cost.copy += cost.copy;
        if since.elapsed() < Duration::from_secs(5) {
            return;
        }
        let n = self.frames as f32;
        log::info!(
            "visuals: {}x{}: {:.0} fps; submit {:.2} ms, wait {:.2} ms, copy {:.2} ms",
            size.0,
            size.1,
            n / since.elapsed().as_secs_f32(),
            self.cost.submit / n,
            self.cost.wait / n,
            self.cost.copy / n,
        );
        *self = Self {
            frames: 0,
            cost: FrameCost::default(),
            since: None,
        };
    }
}
