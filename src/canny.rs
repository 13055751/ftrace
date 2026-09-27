//! Canny edge detection in pure Rust.
//!
//! Algorithm follows the classic Canny pipeline (Gaussian blur -> Sobel
//! gradient -> non-maximum suppression -> double threshold + hysteresis),
//! matching the open-source references this project builds on:
//!   - OpenCV `modules/imgproc/src/canny.cpp`
//!   - seiza-imgproc (Apache-2.0, github.com/theatrus/seiza) port of that routine
//!   - img2svg (Apache-2.0, github.com/yingkitw/img2svg), which applies Sobel edges
//! This implementation is written from the algorithm description, not copied.

use image::{GrayImage, ImageBuffer, Luma};

/// Canny parameters. Thresholds are fractions (0..=1) of the maximum
/// gradient magnitude found in the image, which is more robust than fixed
/// absolute values for arbitrary content.
#[derive(Clone, Copy, Debug)]
pub struct CannyParams {
    /// Gaussian blur standard deviation applied before gradients.
    pub sigma: f32,
    /// Low hysteresis threshold (fraction of max gradient magnitude).
    pub low: f32,
    /// High hysteresis threshold (fraction of max gradient magnitude).
    pub high: f32,
}

impl Default for CannyParams {
    fn default() -> Self {
        CannyParams {
            sigma: 1.2,
            low: 0.08,
            high: 0.22,
        }
    }
}

/// Returns a binary edge image (255 = edge pixel, 0 = otherwise).
pub fn canny(img: &GrayImage, p: &CannyParams) -> GrayImage {
    let (w, h) = img.dimensions();
    let n = (w * h) as usize;
    let blurred = imageproc::filter::gaussian_blur_f32(img, p.sigma);

    let px = |x: u32, y: u32| -> f32 { blurred.get_pixel(x, y).0[0] as f32 };

    // Sobel gradients + magnitude.
    const SOBEL_X: [f32; 9] = [-1.0, 0.0, 1.0, -2.0, 0.0, 2.0, -1.0, 0.0, 1.0];
    const SOBEL_Y: [f32; 9] = [-1.0, -2.0, -1.0, 0.0, 0.0, 0.0, 1.0, 2.0, 1.0];
    let mut gx = vec![0f32; n];
    let mut gy = vec![0f32; n];
    let mut mag = vec![0f32; n];
    let mut max_mag = 0f32;
    for y in 1..h.saturating_sub(1) {
        for x in 1..w.saturating_sub(1) {
            let mut sxf = 0f32;
            let mut syf = 0f32;
            let mut k = 0usize;
            for dy in -1i32..=1 {
                for dx in -1i32..=1 {
                    let v = px((x as i32 + dx) as u32, (y as i32 + dy) as u32);
                    sxf += v * SOBEL_X[k];
                    syf += v * SOBEL_Y[k];
                    k += 1;
                }
            }
            let m = (sxf * sxf + syf * syf).sqrt();
            let i = (y * w + x) as usize;
            gx[i] = sxf;
            gy[i] = syf;
            mag[i] = m;
            if m > max_mag {
                max_mag = m;
            }
        }
    }
    if max_mag <= 0.0 {
        return GrayImage::new(w, h);
    }

    // Non-maximum suppression against (quantized) gradient direction.
    let mut nms = vec![0f32; n];
    for y in 1..h.saturating_sub(1) {
        for x in 1..w.saturating_sub(1) {
            let i = (y * w + x) as usize;
            let m = mag[i];
            if m <= 0.0 {
                continue;
            }
            let gxv = gx[i];
            let gyv = gy[i];
            let (n1, n2) = if gxv.abs() > 3.0 * gyv.abs() {
                (mag[i - 1], mag[i + 1])
            } else if gyv.abs() > 3.0 * gxv.abs() {
                (mag[i - w as usize], mag[i + w as usize])
            } else if (gxv > 0.0) == (gyv > 0.0) {
                (mag[i - w as usize - 1], mag[i + w as usize + 1])
            } else {
                (mag[i - w as usize + 1], mag[i + w as usize - 1])
            };
            if m >= n1 && m >= n2 {
                nms[i] = m;
            }
        }
    }

    // Double threshold + hysteresis (8-connectivity flood from strong seeds).
    let strong_t = p.high * max_mag;
    let weak_t = p.low * max_mag;
    let mut out = GrayImage::new(w, h);
    let mut strong = vec![false; n];
    let mut stack: Vec<(u32, u32)> = Vec::new();
    for y in 0..h {
        for x in 0..w {
            let i = (y * w + x) as usize;
            if nms[i] >= strong_t {
                strong[i] = true;
                stack.push((x, y));
            }
        }
    }
    while let Some((x, y)) = stack.pop() {
        out.put_pixel(x, y, Luma([255u8]));
        for dy in -1i32..=1 {
            for dx in -1i32..=1 {
                if dx == 0 && dy == 0 {
                    continue;
                }
                let nx = x as i32 + dx;
                let ny = y as i32 + dy;
                if nx < 0 || ny < 0 || nx >= w as i32 || ny >= h as i32 {
                    continue;
                }
                let nxi = nx as u32;
                let nyi = ny as u32;
                let idx = (nyi * w + nxi) as usize;
                if !strong[idx] && nms[idx] >= weak_t {
                    strong[idx] = true;
                    stack.push((nxi, nyi));
                }
            }
        }
    }
    out
}

/// Small helper used when debugging: builds a tiny blurred grayscale image.
#[allow(dead_code)]
fn _kept_for_docs() -> ImageBuffer<Luma<f32>, Vec<f32>> {
    unimplemented!()
}