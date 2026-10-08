//! The frames an effect shows: the one on screen, and the one before it,
//! dropped from GPUI's atlas a frame later so its atlas texture isn't freed
//! and made again every frame.

use std::collections::VecDeque;
use std::sync::Arc;
use std::time::{Duration, Instant};

use encore_visuals::FrameCost;
use gpui_kit::*;

#[derive(Default)]
pub struct Frames {
    shown: Option<Arc<RenderImage>>,
    retired: VecDeque<Arc<RenderImage>>,
    invalid_reported: bool,
    drop_reported: bool,
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
        let Some(pixels) = pixels(frame) else {
            if !std::mem::replace(&mut self.invalid_reported, true) {
                log::warn!(
                    "visuals: invalid frame dimensions or BGRA length; keeping previous image"
                );
            }
            return;
        };
        let image = Arc::new(RenderImage::new([image::Frame::new(pixels)]));
        if let Some(old) = self.shown.replace(image) {
            self.retired.push_back(old);
        }
        while self.retired.len() > 1 {
            if let Some(old) = self.retired.pop_front() {
                // GPUI 0.3.8 currently always returns Ok here. If its atlas
                // implementation becomes fallible, report that separately
                // from cancellation of an entity/window update.
                if let Err(error) = window.drop_image(old)
                    && !std::mem::replace(&mut self.drop_reported, true)
                {
                    log::warn!("visuals: retiring atlas image: {error:#}");
                }
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

/// Public renderer frames can be constructed by any caller. Reject empty,
/// overflowing, short and overlong buffers before replacing a valid image.
fn pixels(frame: encore_visuals::Frame) -> Option<image::RgbaImage> {
    if frame.width == 0 || frame.height == 0 {
        return None;
    }
    let expected = usize::try_from(frame.width)
        .ok()?
        .checked_mul(usize::try_from(frame.height).ok()?)?
        .checked_mul(4)?;
    if expected != frame.bgra.len() {
        return None;
    }
    image::RgbaImage::from_raw(frame.width, frame.height, frame.bgra)
}

/// Atlas insertion/painting errors are real failures, not weak-entity
/// cancellation during teardown. Report each layer's first failure only.
#[derive(Clone, Copy, Debug)]
pub(super) enum PaintLayer {
    Bar,
    Backdrop,
    Visualizer,
    Dissolve,
    Flight,
}

impl PaintLayer {
    pub fn report(self, result: anyhow::Result<()>) {
        static REPORTED: [std::sync::Once; 5] = [const { std::sync::Once::new() }; 5];
        if let Err(error) = result {
            REPORTED[self as usize].call_once(|| {
                log::warn!("visuals: {self:?} paint failed: {error:#}");
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::pixels;
    use encore_visuals::FrameCost;

    fn frame(width: u32, height: u32, bytes: usize) -> encore_visuals::Frame {
        encore_visuals::Frame {
            width,
            height,
            bgra: vec![0; bytes],
            cost: FrameCost::default(),
        }
    }

    // The GPUI glob import includes a test attribute; use the built-in one
    // explicitly for this pure frame-validation test.
    #[::core::prelude::v1::test]
    fn rejects_invalid_frames_and_keeps_valid_bgra_bytes() {
        for (width, height, bytes) in [
            (0, 1, 0),
            (1, 0, 0),
            (2, 2, 15),
            (2, 2, 17),
            (u32::MAX, u32::MAX, 0),
        ] {
            assert!(pixels(frame(width, height, bytes)).is_none());
        }
        let mut valid = frame(2, 1, 8);
        valid.bgra = vec![1, 2, 3, 4, 5, 6, 7, 8];
        let Some(image) = pixels(valid) else {
            panic!("valid frame rejected")
        };
        assert_eq!(image.dimensions(), (2, 1));
        assert_eq!(image.as_raw(), &[1, 2, 3, 4, 5, 6, 7, 8]);
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
