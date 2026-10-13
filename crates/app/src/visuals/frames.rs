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
    /// The frame on screen, including its one-texel sampling border.
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

/// Add a one-texel border that repeats the edge colours, including alpha.
/// GPUI filters atlas tiles linearly: an upscaled edge can otherwise sample
/// a neighbouring tile, which changes as animated frames enter the atlas.
/// Paint these images with `image_bounds` or `cover_bounds` to hide the border.
/// Reject invalid buffers before replacing a valid image.
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
    let width = frame.width.checked_add(2)?;
    let height = frame.height.checked_add(2)?;
    let stride = usize::try_from(width).ok()?.checked_mul(4)?;
    let length = stride.checked_mul(usize::try_from(height).ok()?)?;
    let source_stride = usize::try_from(frame.width).ok()?.checked_mul(4)?;
    let mut bytes = vec![0; length];
    for (y, row) in bytes.chunks_exact_mut(stride).enumerate() {
        let source_y = y.saturating_sub(1).min(frame.height as usize - 1);
        let start = source_y * source_stride;
        let source = &frame.bgra[start..start + source_stride];
        row[..4].copy_from_slice(&source[..4]);
        row[4..stride - 4].copy_from_slice(source);
        row[stride - 4..].copy_from_slice(&source[source_stride - 4..]);
    }
    image::RgbaImage::from_raw(width, height, bytes)
}

/// Place the original frame in `bounds`, with its atlas border outside it.
/// Pass `bounds` separately to `paint_image` for clipping and corner radii.
pub fn image_bounds(bounds: Bounds<Pixels>, image: &RenderImage) -> Bounds<Pixels> {
    let size = image.size(0);
    let texel = gpui_kit::size(
        bounds.size.width / (size.width.0 - 2).max(1) as f32,
        bounds.size.height / (size.height.0 - 2).max(1) as f32,
    );
    Bounds::new(
        bounds.origin - point(texel.width, texel.height),
        gpui_kit::size(
            bounds.size.width + texel.width * 2.,
            bounds.size.height + texel.height * 2.,
        ),
    )
}

/// Cover-fit the original frame, preserving its aspect ratio independently
/// of the border (important for narrow strips and non-square frames).
pub fn cover_bounds(bounds: Bounds<Pixels>, image: &RenderImage) -> Bounds<Pixels> {
    let size = image.size(0);
    let (w, h) = (
        (size.width.0 - 2).max(1) as f32,
        (size.height.0 - 2).max(1) as f32,
    );
    let scale = (f32::from(bounds.size.width) / w).max(f32::from(bounds.size.height) / h);
    let fitted = gpui_kit::size(px(w * scale), px(h * scale));
    image_bounds(
        Bounds::new(
            bounds.center() - point(fitted.width / 2., fitted.height / 2.),
            fitted,
        ),
        image,
    )
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
    use super::{cover_bounds, image_bounds, pixels};
    use encore_visuals::FrameCost;
    use gpui_kit::{Bounds, RenderImage, point, px, size};

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
        assert_eq!(image.dimensions(), (4, 3));
        for row in image.as_raw().as_chunks::<16>().0 {
            assert_eq!(row, &[1, 2, 3, 4, 1, 2, 3, 4, 5, 6, 7, 8, 5, 6, 7, 8]);
        }
    }

    #[::core::prelude::v1::test]
    fn border_repeats_all_edges_and_corners_without_changing_alpha() {
        for (width, height) in [(1, 1), (1, 5), (5, 1), (3, 5), (4, 2)] {
            let mut source = frame(width, height, (width * height * 4) as usize);
            for (i, pixel) in source.bgra.as_chunks_mut::<4>().0.iter_mut().enumerate() {
                pixel.copy_from_slice(&[i as u8, 23, 47, (i * 7) as u8]);
            }
            let original = source.bgra.clone();
            let padded = pixels(source).unwrap_or_else(|| panic!("valid frame rejected"));
            for (x, y, pixel) in padded.enumerate_pixels() {
                let sx = x.saturating_sub(1).min(width - 1);
                let sy = y.saturating_sub(1).min(height - 1);
                let start = ((sy * width + sx) * 4) as usize;
                assert_eq!(pixel.0, original[start..start + 4]);
            }
        }
    }

    #[::core::prelude::v1::test]
    fn painting_crops_only_the_border_at_fractional_and_odd_sizes() {
        let padded = pixels(frame(3, 5, 60)).unwrap_or_else(|| panic!("valid frame rejected"));
        let image = RenderImage::new([image::Frame::new(padded)]);
        for (w, h) in [(3., 5.), (901., 703.), (720.8, 562.4)] {
            let content = Bounds::new(point(px(13.25), px(27.5)), size(px(w), px(h)));
            let outer = image_bounds(content, &image);
            // Match GPUI's atlas-subtile calculation: the clipped rectangle
            // must address the original 3x5 texels, starting at border (1, 1).
            let offset = content.origin - outer.origin;
            assert_eq!((offset.x / outer.size.width * 5.).round(), 1.);
            assert_eq!((offset.y / outer.size.height * 7.).round(), 1.);
            assert_eq!((content.size.width / outer.size.width * 5.).round(), 3.);
            assert_eq!((content.size.height / outer.size.height * 7.).round(), 5.);
        }
    }

    #[::core::prelude::v1::test]
    fn cover_fit_uses_the_original_aspect_ratio() {
        let padded = pixels(frame(3, 5, 60)).unwrap_or_else(|| panic!("valid frame rejected"));
        let image = RenderImage::new([image::Frame::new(padded)]);
        let panel = Bounds::new(point(px(10.), px(20.)), size(px(90.), px(90.)));
        let fitted = cover_bounds(panel, &image);
        // Original frame fits as 90x150; one border texel is 30 points.
        assert_eq!(
            fitted,
            Bounds::new(point(px(-20.), px(-40.)), size(px(150.), px(210.)))
        );
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
