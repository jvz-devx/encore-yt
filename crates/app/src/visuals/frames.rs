//! The frames an effect shows: the one on screen, and the one before it,
//! dropped from GPUI's atlas a frame later so its atlas texture isn't freed
//! and made again every frame.

use std::collections::VecDeque;
use std::sync::Arc;
use std::time::{Duration, Instant};

use gpui_kit::*;
use encore_visuals::FrameCost;

#[derive(Default)]
pub struct Frames {
    shown: Option<Arc<RenderImage>>,
    retired: VecDeque<Arc<RenderImage>>,
}

impl Frames {
    /// The frame on screen.
    pub fn image(&self) -> Option<Arc<RenderImage>> {
        self.shown.clone()
    }

    /// Shows `frame` from now on.
    pub fn push(&mut self, frame: encore_visuals::Frame, window: &mut Window) {
        if super::effects::skip("upload") && self.shown.is_some() {
            return;
        }
        let pixels = image::RgbaImage::from_raw(frame.width, frame.height, frame.bgra)
            .expect("a frame's size matches its bytes");
        let image = Arc::new(RenderImage::new([image::Frame::new(pixels)]));
        if let Some(old) = self.shown.replace(image) {
            self.retired.push_back(old);
        }
        while self.retired.len() > 1 {
            if let Some(old) = self.retired.pop_front() {
                let _ = window.drop_image(old);
            }
        }
    }

    /// Drops every frame from the atlas.
    pub fn clear(&mut self, cx: &mut App) {
        for image in self.shown.take().into_iter().chain(self.retired.drain(..)) {
            cx.drop_image(image, None);
        }
    }

    /// Forgets the frames without touching the atlas (a new window has its
    /// own).
    pub fn forget(&mut self) {
        self.shown = None;
        self.retired.clear();
    }
}

/// An effect's frame costs, logged every five seconds while it animates.
#[derive(Default)]
pub struct Stats {
    frames: u32,
    cost: FrameCost,
    since: Option<Instant>,
}

impl Stats {
    pub fn record(&mut self, label: &str, cost: FrameCost, size: (u32, u32)) {
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
            "visuals: {label} {}x{}: {:.0} fps; submit {:.2} ms, wait {:.2} ms, copy {:.2} ms",
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
