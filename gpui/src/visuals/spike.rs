//! The spike's view: backdrop, spectrum bars, waveform and a cost readout.

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use gpui_kit::component::{ActiveTheme, h_flex, v_flex};
use gpui_kit::*;

use super::gpu::{Backdrop, COVER_SIZE, FrameCost, FrameParams};
use super::spectrum::{BANDS, Bands, SpectrumTap};
use super::waveform::{self, Waveform};

/// What the app tells the spike each render.
#[derive(Clone, Default)]
pub struct Input {
    pub video_id: Option<String>,
    pub cover: Option<String>,
    pub playing: bool,
    pub position: f64,
    pub duration: f64,
}

pub struct VisualsSpike {
    pub input: Input,
    backdrop: Option<Result<Backdrop, String>>,
    /// The frame on screen, and the one before it (dropped from GPUI's atlas
    /// a frame later, so its atlas texture isn't freed and re-made).
    shown: Option<Arc<RenderImage>>,
    retired: VecDeque<Arc<RenderImage>>,
    cover: Option<String>,
    palette: [[f32; 4]; 4],
    tap: SpectrumTap,
    waveform: Arc<Mutex<Option<Waveform>>>,
    waveform_for: Option<String>,
    socket: PathBuf,
    /// Animation time: moves only while animating, so pause freezes it.
    clock: f32,
    last: Option<Instant>,
    stats: Stats,
    /// Notifies at `fps()` while animating (see `pace`).
    ticker: Option<Task<()>>,
}

#[derive(Default)]
struct Stats {
    frames: u32,
    interval: f32,
    cost: FrameCost,
    since: Option<Instant>,
    text: String,
}

impl VisualsSpike {
    pub fn new(socket: PathBuf) -> Self {
        Self {
            input: Input::default(),
            backdrop: None,
            shown: None,
            retired: VecDeque::new(),
            cover: None,
            palette: [[0.2, 0.2, 0.3, 1.0]; 4],
            tap: SpectrumTap::start(),
            waveform: Arc::default(),
            waveform_for: None,
            socket,
            clock: 0.0,
            last: None,
            stats: Stats::default(),
            ticker: None,
        }
    }

    fn backdrop(&mut self) -> Option<&mut Backdrop> {
        let backdrop = self.backdrop.get_or_insert_with(|| {
            let (w, h) = resolution();
            let made = Backdrop::new(w, h).map_err(|e| format!("{e:#}"));
            match &made {
                Ok(b) => log::info!("visuals: backdrop {w}x{h} on {}", b.adapter),
                Err(e) => log::warn!("visuals: no backdrop: {e}"),
            }
            made
        });
        backdrop.as_mut().ok()
    }

    /// Uploads the current cover once GPUI has loaded it.
    fn sync_cover(&mut self, window: &mut Window, cx: &mut App) {
        let Some(url) = self.input.cover.clone() else {
            return;
        };
        if self.cover.as_ref() == Some(&url) {
            return;
        }
        let resource = Resource::Uri(SharedUri::from(url.clone()));
        let Some(Ok(image)) = window.use_asset::<ImgResourceLoader>(&resource, cx) else {
            return;
        };
        let small = small_cover(&image);
        self.palette = palette(&small);
        if let Some(backdrop) = self.backdrop() {
            backdrop.set_cover(&small);
        }
        self.cover = Some(url);
    }

    fn sync_waveform(&mut self) {
        let Some(id) = self.input.video_id.clone() else {
            return;
        };
        if self.waveform_for.as_ref() == Some(&id) || !self.input.playing {
            return;
        }
        self.waveform_for = Some(id.clone());
        *self.waveform.lock().expect("waveform") = None;
        waveform::start(self.socket.clone(), id, self.waveform.clone());
    }

    /// Renders the next backdrop frame if it should move.
    fn advance(&mut self, animate: bool, bands: &Bands, window: &mut Window) {
        let now = Instant::now();
        if !animate && self.shown.is_some() {
            self.last = None;
            return;
        }
        // Other redraws (the position clock, events) don't add frames.
        let since = self.last.map_or(1.0, |l| (now - l).as_secs_f32());
        if fps() > 0 && since < 0.8 / fps() as f32 {
            return;
        }
        let dt = if self.last.is_some() {
            since.min(0.1)
        } else {
            0.0
        };
        self.last = Some(now);
        self.clock += dt;
        let bass = bands.levels[..4].iter().sum::<f32>() / 4.0;
        let level = bands.levels.iter().sum::<f32>() / BANDS as f32;
        let params = FrameParams {
            seconds: self.clock,
            bass,
            level,
            palette: self.palette,
        };
        let Some(backdrop) = self.backdrop() else {
            return;
        };
        match backdrop.frame(&params) {
            Ok(Some((image, cost))) => {
                if let Some(old) = self.shown.replace(image) {
                    self.retired.push_back(old);
                }
                while self.retired.len() > 1 {
                    if let Some(old) = self.retired.pop_front() {
                        let _ = window.drop_image(old);
                    }
                }
                self.stats.record(dt, cost);
            }
            Ok(None) => {}
            Err(e) => log::warn!("visuals: frame: {e:#}"),
        }
        // Until a first frame shows; then `pace` drives the frames.
        if self.shown.is_none() || (animate && fps() == 0) {
            window.request_animation_frame();
        }
    }

    /// While animating, a timer notifies at `fps()`: GPUI would otherwise
    /// draw at the display's rate (120 Hz here) through
    /// `request_animation_frame`. Dropping the task stops it.
    fn pace(&mut self, animate: bool, cx: &mut Context<Self>) {
        if !animate || fps() == 0 {
            self.ticker = None;
            return;
        }
        if self.ticker.is_some() {
            return;
        }
        let period = Duration::from_secs_f32(1.0 / fps() as f32);
        self.ticker = Some(cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(period).await;
                if this.update(cx, |_, cx| cx.notify()).is_err() {
                    break;
                }
            }
        }));
    }
}

impl Stats {
    fn record(&mut self, dt: f32, cost: FrameCost) {
        let since = *self.since.get_or_insert_with(Instant::now);
        self.frames += 1;
        self.interval += dt;
        self.cost.submit += cost.submit;
        self.cost.wait += cost.wait;
        self.cost.copy += cost.copy;
        if since.elapsed() < Duration::from_secs(2) {
            return;
        }
        let n = self.frames as f32;
        let (w, h) = resolution();
        self.text = format!(
            "{w}x{h}: {:.0} fps, frame {:.1} ms; submit {:.2} ms, wait {:.2} ms, copy {:.2} ms",
            n / since.elapsed().as_secs_f32(),
            self.interval / n * 1000.0,
            self.cost.submit / n,
            self.cost.wait / n,
            self.cost.copy / n,
        );
        log::info!("visuals: {}", self.text);
        *self = Self {
            text: std::mem::take(&mut self.text),
            ..Self::default()
        };
    }
}

impl Render for VisualsSpike {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.sync_cover(window, cx);
        self.sync_waveform();
        let mut bands = self.tap.bands.lock().expect("bands").clone();
        // No samples for a moment (paused, stopped): let the bars fall.
        if bands
            .at
            .is_none_or(|at| at.elapsed() > Duration::from_millis(150))
        {
            bands.levels = [0.0; BANDS];
        }
        let animate = self.input.playing && !cx.reduce_motion();
        self.advance(animate, &bands, window);
        self.pace(animate, cx);

        let theme = cx.theme();
        let shown = self.shown.clone();
        let waveform = self
            .waveform
            .lock()
            .expect("waveform")
            .clone()
            .filter(|w| Some(&w.video_id) == self.input.video_id.as_ref());
        let progress = (self.input.position / self.input.duration.max(1.0)) as f32;
        let (played, rest) = (theme.primary, theme.muted_foreground.opacity(0.5));
        let backdrop_note = match &self.backdrop {
            Some(Err(e)) => format!("no backdrop: {e}"),
            Some(Ok(b)) => b.adapter.clone(),
            None => String::new(),
        };
        let node = format!("tap linked to {} mpv stream(s)", bands.linked);
        div()
            .relative()
            .flex_1()
            .min_h_0()
            .w_full()
            .overflow_hidden()
            .child(
                canvas(
                    |_, _, _| (),
                    move |bounds, (), window, _| {
                        if let Some(image) = shown {
                            paint_cover(bounds, image, window);
                        }
                    },
                )
                .absolute()
                .size_full(),
            )
            .child(
                v_flex()
                    .absolute()
                    .inset_0()
                    .p_6()
                    .gap_3()
                    .justify_end()
                    .text_color(theme.foreground)
                    .text_xs()
                    .child(format!("{backdrop_note} · {node} · {}", self.stats.text))
                    .children(waveform.as_ref().map(|w| div().child(w.how.clone())))
                    .child(spectrum_bars(&bands.levels, played))
                    .child(waveform_strip(waveform, progress, played, rest)),
            )
    }
}

/// Fills `bounds` with the frame, cropped to keep its aspect ratio.
fn paint_cover(bounds: Bounds<Pixels>, image: Arc<RenderImage>, window: &mut Window) {
    let size = image.size(0);
    let (iw, ih) = (size.width.0 as f32, size.height.0 as f32);
    let (bw, bh) = (f32::from(bounds.size.width), f32::from(bounds.size.height));
    let scale = (bw / iw).max(bh / ih);
    let fitted = gpui_kit::size(px(iw * scale), px(ih * scale));
    let origin = bounds.center() - point(fitted.width / 2., fitted.height / 2.);
    let image_bounds = Bounds::new(origin, fitted);
    let _ = window.paint_image(bounds, image_bounds, Corners::default(), image, 0, false);
}

fn spectrum_bars(levels: &[f32; BANDS], color: Hsla) -> impl IntoElement {
    h_flex()
        .h(px(120.))
        .w_full()
        .items_end()
        .gap(px(3.))
        .children(levels.iter().map(move |l| {
            div()
                .flex_1()
                .h(relative(l.max(0.02)))
                .rounded_t(px(2.))
                .bg(color.opacity(0.85))
        }))
}

fn waveform_strip(
    waveform: Option<Waveform>,
    progress: f32,
    played: Hsla,
    rest: Hsla,
) -> impl IntoElement {
    canvas(
        |_, _, _| (),
        move |bounds, (), window, _| {
            let Some(waveform) = waveform else {
                return;
            };
            let n = waveform.peaks.len().max(1) as f32;
            let step = bounds.size.width / n;
            for (i, peak) in waveform.peaks.iter().enumerate() {
                let x = bounds.origin.x + step * i as f32;
                let h = bounds.size.height * peak.max(0.04);
                let y = bounds.origin.y + (bounds.size.height - h) / 2.;
                let color = if (i as f32) / n < progress {
                    played
                } else {
                    rest
                };
                let bar = Bounds::new(point(x, y), gpui_kit::size(step * 0.7, h));
                window.paint_quad(fill(bar, color));
            }
        },
    )
    .h(px(48.))
    .w_full()
}

/// Backdrop frames per second from `YTFAST_GPUI_VISUALS_FPS` (60 by
/// default; 0 draws on every display frame).
fn fps() -> u32 {
    std::env::var("YTFAST_GPUI_VISUALS_FPS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(60)
}

/// The render size from `YTFAST_GPUI_VISUALS_RES` ("640x360" by default).
fn resolution() -> (u32, u32) {
    std::env::var("YTFAST_GPUI_VISUALS_RES")
        .ok()
        .and_then(|v| {
            let (w, h) = v.split_once('x')?;
            Some((w.parse().ok()?, h.parse().ok()?))
        })
        .unwrap_or((640, 360))
}

/// The cover at `COVER_SIZE`², RGBA.
fn small_cover(image: &RenderImage) -> Vec<u8> {
    let size = image.size(0);
    let (w, h) = (size.width.0 as u32, size.height.0 as u32);
    let mut bytes = image.as_bytes(0).unwrap_or_default().to_vec();
    // GPUI keeps BGRA; swap to RGBA.
    for px in bytes.as_chunks_mut::<4>().0 {
        px.swap(0, 2);
    }
    let Some(full) = image::RgbaImage::from_raw(w, h, bytes) else {
        return vec![0; (COVER_SIZE * COVER_SIZE * 4) as usize];
    };
    image::imageops::resize(
        &full,
        COVER_SIZE,
        COVER_SIZE,
        image::imageops::FilterType::Triangle,
    )
    .into_raw()
}

/// Four colours: the average of each quadrant of the small cover.
fn palette(rgba: &[u8]) -> [[f32; 4]; 4] {
    let half = COVER_SIZE / 2;
    let mut out = [[0.0f32; 4]; 4];
    for (q, color) in out.iter_mut().enumerate() {
        let (qx, qy) = ((q as u32 % 2) * half, (q as u32 / 2) * half);
        let mut sum = [0.0f32; 3];
        for y in qy..qy + half {
            for x in qx..qx + half {
                let i = ((y * COVER_SIZE + x) * 4) as usize;
                for c in 0..3 {
                    sum[c] += f32::from(rgba[i + c]) / 255.0;
                }
            }
        }
        let n = (half * half) as f32;
        *color = [sum[0] / n, sum[1] / n, sum[2] / n, 1.0];
    }
    out
}
