//! The cover as the backdrop uses it: a small, pre-blurred RGBA square and
//! a palette of four colours.

/// The cover is reduced to this many pixels a side before upload: the
/// backdrop shows it blurred anyway.
pub const COVER_SIZE: u32 = 64;

/// A cover ready for [`crate::Renderer::set_cover`].
#[derive(Clone)]
pub struct Cover {
    /// `COVER_SIZE`² RGBA pixels, box-blurred.
    pub rgba: Vec<u8>,
    /// Four colours (RGBA, 0..1, display space): each quadrant's average,
    /// pushed away from grey so dull covers still colour the backdrop.
    pub palette: [[f32; 4]; 4],
}

impl Cover {
    /// From decoded pixels in GPUI's order (BGRA, as `RenderImage` keeps
    /// them), any size.
    pub fn from_bgra(width: u32, height: u32, bgra: &[u8]) -> Self {
        let small = resample(width, height, bgra);
        let palette = palette(&small);
        let rgba = blur(&blur(&small));
        Self { rgba, palette }
    }

    /// No cover: a dim, neutral square and palette.
    pub fn blank() -> Self {
        let n = (COVER_SIZE * COVER_SIZE) as usize;
        Self {
            rgba: [40, 40, 48, 255].repeat(n),
            palette: [[0.16, 0.16, 0.2, 1.0]; 4],
        }
    }
}

/// Box-filters `bgra` down (or up) to `COVER_SIZE`², as RGBA.
fn resample(width: u32, height: u32, bgra: &[u8]) -> Vec<u8> {
    let n = COVER_SIZE as usize;
    let (w, h) = (width.max(1) as usize, height.max(1) as usize);
    let mut out = vec![0u8; n * n * 4];
    if bgra.len() < w * h * 4 {
        return Cover::blank().rgba;
    }
    for oy in 0..n {
        let (y0, y1) = span(oy, n, h);
        for ox in 0..n {
            let (x0, x1) = span(ox, n, w);
            let mut sum = [0u32; 3];
            for y in y0..y1 {
                for x in x0..x1 {
                    let i = (y * w + x) * 4;
                    sum[0] += u32::from(bgra[i + 2]);
                    sum[1] += u32::from(bgra[i + 1]);
                    sum[2] += u32::from(bgra[i]);
                }
            }
            let count = ((y1 - y0) * (x1 - x0)) as u32;
            let o = (oy * n + ox) * 4;
            for c in 0..3 {
                out[o + c] = (sum[c] / count) as u8;
            }
            out[o + 3] = 255;
        }
    }
    out
}

/// The source range that output pixel `i` of `n` covers, at least one wide.
fn span(i: usize, n: usize, len: usize) -> (usize, usize) {
    let start = i * len / n;
    let end = ((i + 1) * len / n).max(start + 1).min(len);
    (start.min(len - 1), end)
}

/// A 5×5 box blur (edges clamped).
fn blur(rgba: &[u8]) -> Vec<u8> {
    let n = COVER_SIZE as i32;
    let at = |x: i32, y: i32| ((y.clamp(0, n - 1) * n + x.clamp(0, n - 1)) * 4) as usize;
    let mut out = rgba.to_vec();
    for y in 0..n {
        for x in 0..n {
            let mut sum = [0u32; 3];
            for dy in -2..=2 {
                for dx in -2..=2 {
                    let i = at(x + dx, y + dy);
                    for c in 0..3 {
                        sum[c] += u32::from(rgba[i + c]);
                    }
                }
            }
            let o = at(x, y);
            for c in 0..3 {
                out[o + c] = (sum[c] / 25) as u8;
            }
        }
    }
    out
}

/// Quadrant averages with their saturation lifted.
fn palette(rgba: &[u8]) -> [[f32; 4]; 4] {
    let n = COVER_SIZE as usize;
    let half = n / 2;
    let mut out = [[0.0f32; 4]; 4];
    for (q, color) in out.iter_mut().enumerate() {
        let (qx, qy) = ((q % 2) * half, (q / 2) * half);
        let mut sum = [0.0f32; 3];
        for y in qy..qy + half {
            for x in qx..qx + half {
                let i = (y * n + x) * 4;
                for c in 0..3 {
                    sum[c] += f32::from(rgba[i + c]) / 255.0;
                }
            }
        }
        let count = (half * half) as f32;
        let avg = sum.map(|s| s / count);
        let grey = (avg[0] + avg[1] + avg[2]) / 3.0;
        let lifted = avg.map(|c| (grey + (c - grey) * 1.4).clamp(0.0, 1.0));
        *color = [lifted[0], lifted[1], lifted[2], 1.0];
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A cover with a red left half and a blue right half keeps red in the
    /// left quadrants of the palette and blue in the right ones.
    #[test]
    fn palette_follows_the_cover() {
        let (w, h) = (100u32, 80u32);
        let mut bgra = Vec::new();
        for _ in 0..h {
            for x in 0..w {
                bgra.extend(if x < w / 2 {
                    [0, 0, 220, 255]
                } else {
                    [220, 0, 0, 255]
                });
            }
        }
        let cover = Cover::from_bgra(w, h, &bgra);
        assert_eq!(cover.rgba.len(), (COVER_SIZE * COVER_SIZE * 4) as usize);
        for q in [0, 2] {
            assert!(cover.palette[q][0] > 0.6 && cover.palette[q][2] < 0.2);
        }
        for q in [1, 3] {
            assert!(cover.palette[q][2] > 0.6 && cover.palette[q][0] < 0.2);
        }
    }
}
