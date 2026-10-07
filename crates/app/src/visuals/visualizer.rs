//! The audio visualiser (M21) on the effects layer: `encore_visuals`'s
//! `Visualizer` frames painted over the backdrop, in Now Playing (when
//! Settings → Visuals puts it there), in Stage (when switched on) and in
//! the full-window visualiser (V).
//!
//! Where it draws depends on the style: bars, mirrored bars, the line
//! spectrum and the scope take a band (Now Playing's strip above the title;
//! in Stage and the full window a band along the bottom, or through the
//! cover for mirrored bars), the ring goes round the cover, and particles fill the
//! space round the cover (Now Playing) or the whole scene. The 3D scenes
//! (M30: XMB, Ridges, Aurora) fill Now Playing's panel or the whole scene,
//! opaque, in place of the backdrop; behind text (Now Playing, Stage) they
//! are toned like it, full strength only in the full-window visualiser.
//! Frames come only with the paced frames while music plays; paused,
//! hidden or under reduced motion it draws nothing.

use std::sync::Arc;
use std::time::Instant;

use encore_visuals::{
    BANDS, BarSettings, Bars, Look, Pace, SPAN, Scene, SceneKind, SceneParams, Scope, Visualizer,
    VisualizerParams, color, seed,
};
use gpui_kit::*;

use super::config::{self, Palette, Spacing, Style};
use super::effects::Tick;
use super::frames::Frames;
use super::slots::{Slot, Slots};
use crate::theme;

/// Where the visualiser shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Place {
    NowPlaying,
    Stage,
    /// The full-window visualiser (V).
    Full,
}

#[derive(Default)]
pub struct Vis {
    renderer: Option<Visualizer>,
    /// The 3D scenes' renderer, and what moves them with the music.
    scene: Option<Scene>,
    pace: Pace,
    stats: super::frames::Stats,
    /// When the scene last drew (its pace and clocks move by the time
    /// since).
    scene_at: Option<Instant>,
    frames: Frames,
    bars: Bars,
    scope: Scope,
    /// The scope's newest samples, kept between frames.
    samples: Vec<[f32; 2]>,
    /// The particles' clock.
    travel: f32,
    /// Where the frame on screen goes, in window coordinates, and its
    /// corners.
    region: Option<(Bounds<Pixels>, Corners<Pixels>)>,
    /// The region the frame in flight was rendered for.
    pending_region: Option<(Bounds<Pixels>, Corners<Pixels>)>,
}

impl Vis {
    /// The frame to paint, where and with what corners, while one shows.
    pub fn image(&self) -> Option<(Arc<RenderImage>, Bounds<Pixels>, Corners<Pixels>)> {
        let (bounds, corners) = self.region?;
        Some((self.frames.image()?, bounds, corners))
    }

    /// Stops showing (hidden, paused): the next start begins from silence.
    pub fn hide(&mut self, cx: &mut App) {
        if self.region.is_some() || self.pending_region.is_some() {
            self.region = None;
            self.pending_region = None;
            self.bars = Bars::default();
            self.scope = Scope::default();
            if let Some(r) = &mut self.renderer {
                r.discard();
            }
            if let Some(s) = &mut self.scene {
                s.discard();
            }
            self.scene_at = None;
            self.frames.clear(cx);
        }
    }

    pub fn forget(&mut self) {
        self.frames.forget();
        self.region = None;
        self.pending_region = None;
    }

    /// Drops the renderer (the GPU is going).
    pub fn release(&mut self, cx: &mut App) {
        self.renderer = None;
        self.scene = None;
        self.hide(cx);
    }

    /// Draws the next frame at `place` when one is due. `video_id` seeds
    /// the 3D scenes.
    pub fn update(
        &mut self,
        tick: &Tick,
        place: Place,
        palette: &[[f32; 4]; 4],
        video_id: Option<&str>,
        window: &mut Window,
        cx: &mut App,
    ) {
        if !tick.due {
            return;
        }
        let config = config::get();
        let v = &config.visualizer;
        if let Some(kind) = v.style.scene() {
            self.update_scene(tick, place, kind, palette, video_id, window, cx);
            return;
        }
        let Some(region) = region(place, v.style, cx) else {
            return;
        };
        let region = whole_pixels(region, window.scale_factor());
        let scale = window.scale_factor() * render_scale(v.style);
        let size = (
            (f32::from(region.size.width) * scale)
                .round()
                .clamp(1., 2048.) as u32,
            (f32::from(region.size.height) * scale)
                .round()
                .clamp(1., 2048.) as u32,
        );
        let settings = BarSettings {
            count: v.bars as usize,
            sensitivity: v.sensitivity,
            smoothing: v.smoothing,
            decay: v.decay,
            low_hz: v.low_hz,
            high_hz: v.high_hz,
            linear: v.spacing == Spacing::Linear,
            peak_fall: v.peak_fall,
        };
        let dt = tick.dt.max(1. / 120.);
        if v.style.spectral() {
            self.bars.update(&tick.levels, &settings, dt);
        } else {
            self.listen(tick, v, size.0, dt);
        }
        self.travel += tick.dt * (0.35 + 1.5 * tick.level);
        let renderer = self.renderer.get_or_insert_with(|| {
            let started = std::time::Instant::now();
            let r = Visualizer::new(tick.gpu, size.0, size.1);
            super::timing::setup(started);
            r
        });
        if renderer.size() != size {
            renderer.resize(size.0, size.1);
        }
        let cover = cover_slot(place)
            .and_then(|slot| Slots::get(cx, slot))
            .map(|c| {
                let o = c.origin - region.origin;
                let r = [
                    f32::from(o.x) * scale,
                    f32::from(o.y) * scale,
                    f32::from(o.x + c.size.width) * scale,
                    f32::from(o.y + c.size.height) * scale,
                ];
                (r, f32::from(cover_radius(place, c.size.width)) * scale)
            });
        let treble = tick.levels[BANDS - 8..].iter().sum::<f32>() / 8.;
        let params = VisualizerParams {
            style: v.style.index(),
            seconds: tick.seconds,
            travel: self.travel,
            bass: tick.bass,
            kick: tick.kick,
            level: tick.level,
            treble,
            look: tick.look,
            opacity: v.opacity,
            glow: v.glow,
            thickness: v.thickness * scale,
            peaks: v.peaks,
            scale,
            cover,
            reach: f32::from(reach(place, cx)) * scale,
            // Bands across a scene stand clear of its edges.
            margin: match place {
                Place::NowPlaying => 0.,
                Place::Stage | Place::Full => size.0 as f32 * 0.07,
            },
            stops: stops(v.palette, &v.custom, palette, tick.look, cx),
            bars: &self.bars,
            scope: &self.scope,
        };
        match renderer.frame(&params) {
            Ok(Some(frame)) => {
                self.frames.push(frame, window);
                self.region = self.pending_region;
            }
            Ok(None) => {}
            Err(e) => log::warn!("visuals: visualiser frame: {e:#}"),
        }
        self.pending_region = Some((region, Corners::default()));
    }

    /// A 3D scene's next frame, filling Now Playing's panel or the scene.
    #[allow(clippy::too_many_arguments)]
    fn update_scene(
        &mut self,
        tick: &Tick,
        place: Place,
        kind: SceneKind,
        palette: &[[f32; 4]; 4],
        video_id: Option<&str>,
        window: &mut Window,
        cx: &mut App,
    ) {
        // Behind text (Now Playing, Stage) a scene draws at most 60 times
        // a second, every other frame on a 120 Hz display: dimmed and
        // moving slowly there, like the backdrop (which draws at half the
        // window's rate). The full window draws every frame.
        let now = Instant::now();
        let since = self.scene_at.map(|at| now.duration_since(at).as_secs_f32());
        if place != Place::Full && since.is_some_and(|s| s < 0.8 / BEHIND_TEXT_FPS) {
            return;
        }
        let Some((region, corners)) = scene_region(place, cx) else {
            return;
        };
        self.scene_at = Some(now);
        let dt = since.unwrap_or(tick.dt).clamp(1. / 240., 0.1);
        let region = whole_pixels(region, window.scale_factor());
        let (w, h) = (
            f32::from(region.size.width) * window.scale_factor(),
            f32::from(region.size.height) * window.scale_factor(),
        );
        let scenes = &config::get().scenes;
        // Scaled down evenly, never past the readback's 2048 pixels.
        let scale = (scene_scale(kind) * scenes.resolution)
            .min(1.)
            .min(2048. / w.max(h).max(1.));
        let size = (
            (w * scale).round().max(1.) as u32,
            (h * scale).round().max(1.) as u32,
        );
        self.pace.update(dt, &tick.levels, tick.bass, tick.level);
        let renderer = self.scene.get_or_insert_with(|| {
            let started = std::time::Instant::now();
            let r = Scene::new(tick.gpu, size.0, size.1);
            super::timing::setup(started);
            r
        });
        if renderer.size() != size {
            renderer.resize(size.0, size.1);
        }
        let params = SceneParams {
            kind,
            look: tick.look,
            visualiser: place == Place::Full,
            seconds: tick.seconds,
            bass: tick.bass,
            kick: tick.kick,
            level: tick.level,
            levels: &tick.levels,
            palette: *palette,
            seed: seed(video_id.unwrap_or_default()),
            pace: &self.pace,
            strength: scenes.strength,
            quality: scenes.detail,
            reaction: scenes.reaction,
        };
        match renderer.frame(&params) {
            Ok(Some(frame)) => {
                self.stats.record(scene_label(kind), frame.cost, size);
                self.frames.push(frame, window);
                self.region = self.pending_region;
            }
            Ok(None) => {}
            Err(e) => log::warn!("visuals: scene frame: {e:#}"),
        }
        self.pending_region = Some((region, corners));
    }

    /// Whether a 3D scene shows now, filling the backdrop's place.
    pub fn fills(&self) -> bool {
        config::get().visualizer.style.scene().is_some() && self.image().is_some()
    }

    /// Moves the scope to the newest samples, with a point every 2.5
    /// output pixels across `width`.
    fn listen(&mut self, tick: &Tick, v: &config::Visualizer, width: u32, dt: f32) {
        let rate = tick
            .tap
            .map_or(0, |tap| tap.recent(SPAN, &mut self.samples));
        if rate == 0 {
            self.samples.clear();
        }
        let points = (width as f32 / 2.5) as usize;
        self.scope.update(
            &self.samples,
            rate,
            v.channels.visuals(),
            v.sensitivity,
            points,
            dt,
        );
    }
}

/// The scale the style renders at, of device pixels: soft particles look
/// the same at less. The ring's frame edge runs through the scene, where a
/// scaled frame's last texels would show as a faint line, so it renders at
/// full size.
fn render_scale(style: Style) -> f32 {
    match style {
        Style::Particles => 0.5,
        _ => 1.,
    }
}

/// The most frames a second a scene draws behind text.
const BEHIND_TEXT_FPS: f32 = 60.;

/// The scale a 3D scene renders at, of device pixels: the XMB's thin lines
/// and sparkles want full size (and it costs little); the raymarched scenes
/// are soft and cost per pixel.
fn scene_scale(kind: SceneKind) -> f32 {
    match kind {
        SceneKind::Xmb => 1.,
        SceneKind::Ridges => 0.6,
        SceneKind::Aurora => 0.5,
    }
}

fn scene_label(kind: SceneKind) -> &'static str {
    match kind {
        SceneKind::Xmb => "scene xmb",
        SceneKind::Ridges => "scene ridges",
        SceneKind::Aurora => "scene aurora",
    }
}

/// Where a 3D scene draws: Now Playing's panel (with its rounded corners)
/// or the whole scene.
fn scene_region(place: Place, cx: &App) -> Option<(Bounds<Pixels>, Corners<Pixels>)> {
    match place {
        Place::NowPlaying => super::slots::panel(cx).map(|p| (p, Corners::all(theme::radius::LG))),
        Place::Stage | Place::Full => Slots::get(cx, Slot::Stage).map(|s| (s, Corners::default())),
    }
}

/// `b` on whole device pixels, so the frame's texels meet the screen's.
fn whole_pixels(b: Bounds<Pixels>, scale: f32) -> Bounds<Pixels> {
    let snap = |v: Pixels| px((f32::from(v) * scale).round() / scale);
    Bounds::from_corners(
        point(snap(b.left()), snap(b.top())),
        point(snap(b.right()), snap(b.bottom())),
    )
}

/// The cover the ring goes round.
fn cover_slot(place: Place) -> Option<Slot> {
    match place {
        Place::NowPlaying => Some(Slot::Cover),
        Place::Stage | Place::Full => Some(Slot::StageCover),
    }
}

/// The cover's corner radius, as the views round it.
fn cover_radius(place: Place, side: Pixels) -> Pixels {
    match place {
        Place::NowPlaying if side >= theme::size::HEADER_COVER => theme::radius::LG,
        Place::NowPlaying => theme::radius::MD,
        Place::Stage | Place::Full => super::stage_cover_radius(side),
    }
}

/// The ring's longest bar, in points.
fn reach(place: Place, cx: &App) -> Pixels {
    let side = cover_slot(place)
        .and_then(|slot| Slots::get(cx, slot))
        .map_or(px(200.), |c| c.size.width);
    match place {
        // Clear of the strip and the title below the cover.
        Place::NowPlaying => super::now_playing_ring_reach(side),
        Place::Stage => super::stage_ring_reach(side),
        Place::Full => super::ring_reach(side),
    }
}

/// Where the visualiser draws, in window coordinates.
fn region(place: Place, style: Style, cx: &App) -> Option<Bounds<Pixels>> {
    let cover = cover_slot(place).and_then(|slot| Slots::get(cx, slot));
    let around = |c: Bounds<Pixels>, by: Pixels| c.dilate(by);
    match place {
        Place::NowPlaying => match style {
            Style::Ring => {
                let page = Slots::get(cx, Slot::Page)?;
                cover.map(|c| around(c, reach(place, cx) + px(24.)).intersect(&page))
            }
            Style::Particles => {
                let page = Slots::get(cx, Slot::Page)?;
                let c = cover?;
                let grown = around(c, c.size.width * 0.3);
                Some(grown.intersect(&page))
            }
            // The scope's traces sit round the strip's middle.
            Style::Scope => Slots::get(cx, Slot::Spectrum).map(|s| s.dilate(px(8.))),
            _ => Slots::get(cx, Slot::Spectrum).map(super::visualizer_strip),
        },
        Place::Stage | Place::Full => {
            let body = Slots::get(cx, Slot::StageBody)?;
            let band = if place == Place::Full { 0.26 } else { 0.22 };
            // Stage keeps a band free under its cover and lyrics.
            let kept = super::stage_band(f32::from(body.size.height));
            match style {
                Style::Ring => cover.map(|c| around(c, reach(place, cx) + px(32.))),
                Style::Particles | Style::Xmb | Style::Ridges | Style::Aurora => {
                    Slots::get(cx, Slot::Stage)
                }
                Style::Bars | Style::Line | Style::Mirrored | Style::Scope => {
                    // Mirrored bars stand on a floor with their reflection
                    // under it: a taller band.
                    let band = if style == Style::Mirrored {
                        band * 1.15
                    } else {
                        band
                    };
                    let h = if place == Place::Stage && kept > px(0.) {
                        kept
                    } else {
                        body.size.height * band
                    };
                    Some(Bounds::new(
                        point(body.left(), body.bottom() - h),
                        size(body.size.width, h),
                    ))
                }
            }
        }
    }
}

/// The gradient's four stops (linear RGB), toned for the look: light and
/// vivid over the dark backdrop, deeper over the light one.
pub(super) fn stops(
    palette: Palette,
    custom: &[config::Swatch; 2],
    cover: &[[f32; 4]; 4],
    look: Look,
    cx: &App,
) -> [[f32; 3]; 4] {
    let c = theme::colors(cx);
    let rgb = |h: Hsla| {
        let c = h.to_rgb();
        [c.r, c.g, c.b]
    };
    let lab = |display: [f32; 3]| color::oklab(color::to_linear(display));
    let labs: [[f32; 3]; 4] = match palette {
        Palette::Cover => {
            let mut labs = cover.map(|c| lab([c[0], c[1], c[2]]));
            // Lows to highs by hue, so neighbours blend rather than jump.
            labs.sort_by(|a, b| a[2].atan2(a[1]).total_cmp(&b[2].atan2(b[1])));
            // A sepia or black-and-white cover gives a quiet grey with a
            // trace of its hue, not its muddy tint pushed to full colour.
            let colourful = colourfulness(&labs);
            let labs = labs.map(|l| tame(l, colourful));
            spread(labs.map(|l| vivid(l, look, 0.12 * colourful)), look)
        }
        Palette::Accent => {
            let base = lab(rgb(c.signal));
            let mut out = [base; 4];
            for (i, o) in out.iter_mut().enumerate() {
                *o = turn(base, i as f32 * 14.);
            }
            spread(out.map(|l| vivid(l, look, 0.12)), look)
        }
        Palette::Theme => [lab(rgb(c.text)); 4],
        Palette::Custom => {
            let (a, b) = (lab(custom[0].rgb()), lab(custom[1].rgb()));
            let mix = |t: f32| [0, 1, 2].map(|i| a[i] + (b[i] - a[i]) * t);
            [mix(0.), mix(1. / 3.), mix(2. / 3.), mix(1.)].map(|l| vivid(l, look, 0.))
        }
    };
    labs.map(|l| {
        let lin = color::oklab_to_linear(l);
        lin.map(|v| v.clamp(0., 1.))
    })
}

/// How colourful a palette is, 0 (grey) to 1: its strongest OKLab chroma
/// between 0.05 and 0.12.
pub(super) fn colourfulness(labs: &[[f32; 3]; 4]) -> f32 {
    let chroma = labs.iter().map(|l| l[1].hypot(l[2])).fold(0., f32::max);
    ((chroma - 0.05) / 0.07).clamp(0., 1.)
}

/// An OKLab colour's chroma scaled down for a palette that is hardly
/// colourful, to at most 0.03 for a grey one.
fn tame(lab: [f32; 3], colourful: f32) -> [f32; 3] {
    let c = lab[1].hypot(lab[2]);
    let most = 0.03 + colourful * 0.3;
    let scale = if c > most { most / c } else { 1. };
    [lab[0], lab[1] * scale, lab[2] * scale]
}

/// The stops from deeper to lighter along the spectrum, so the bars carry
/// a gradient even when the palette is one colour.
fn spread(labs: [[f32; 3]; 4], look: Look) -> [[f32; 3]; 4] {
    let (lo, hi) = match look {
        Look::Dark => (0.7, 0.9),
        Look::Light => (0.42, 0.6),
    };
    let mut out = labs;
    for (i, l) in out.iter_mut().enumerate() {
        let target = lo + (hi - lo) * (0.15 + 0.7 * i as f32 / 3.);
        l[0] = (l[0] + target) * 0.5;
    }
    out
}

/// An OKLab colour brought into the look's lightness range, with at least
/// `chroma` of colour (greys stay grey).
fn vivid(lab: [f32; 3], look: Look, chroma: f32) -> [f32; 3] {
    let (lo, hi) = match look {
        Look::Dark => (0.7, 0.9),
        Look::Light => (0.45, 0.6),
    };
    let c = lab[1].hypot(lab[2]);
    let scale = if c > 0.02 { c.max(chroma) / c } else { 1. };
    [lab[0].clamp(lo, hi), lab[1] * scale, lab[2] * scale]
}

/// `lab` with its hue turned by `degrees`.
fn turn(lab: [f32; 3], degrees: f32) -> [f32; 3] {
    let (s, c) = degrees.to_radians().sin_cos();
    [lab[0], lab[1] * c - lab[2] * s, lab[1] * s + lab[2] * c]
}
