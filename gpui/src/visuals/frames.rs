//! The frames an effect shows: the one on screen, and the one before it,
//! dropped from GPUI's atlas a frame later so its atlas texture isn't freed
//! and made again every frame.

use std::collections::VecDeque;
use std::sync::Arc;

use gpui_kit::*;

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
    pub fn push(&mut self, frame: ytfast_visuals::Frame, window: &mut Window) {
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
