//! Complex Discrete Fourier Transform of contour points (epicycle theory).
//!
//! A closed contour sampled as complex points z[n] = x[n] + i*y[n] is
//! decomposed into rotating circles:
//!     z(t) = sum_k c_k * exp(2*pi*i*k*t),  t in [0,1)
//! Each harmonic c_k is one epicycle with radius |c_k|, frequency k, and
//! initial phase arg(c_k). Technique/format follows well-known open-source
//! implementations:
//!   - The Coding Train #130 "Drawing with Fourier Transform and Epicycles"
//!     (DFT of the path, sort harmonics by magnitude, chain the circles)
//!   - fluffy-eureka / circles-sketch (contour -> complex DFT, harmonic lists)
//!   - fourier-svg-rs (JSON/GIF/HTML exports of harmonic coefficients)

use num_complex::Complex64;
use rustfft::{FftPlanner, num_complex::Complex};
use serde::{Deserialize, Serialize};

use crate::geom::{BBox, arc_length, signed_area, P2};

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Harmonic {
    /// Signed frequency index (negative = counter-rotating circle).
    pub k: i32,
    /// Real part of the complex coefficient (x-amplitude).
    pub real: f64,
    /// Imaginary part of the complex coefficient (y-amplitude).
    pub imag: f64,
}

impl Harmonic {
    #[inline]
    pub fn mag(&self) -> f64 {
        (self.real * self.real + self.imag * self.imag).sqrt()
    }
}

/// One Fourier transform: the decomposition of a single closed contour.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ContourTransform {
    pub id: usize,
    /// Number of uniformly resampled input points.
    pub n_samples: usize,
    /// Number of harmonics retained.
    pub n_harmonics: usize,
    /// Centroid subtracted before the DFT (image pixel coords).
    pub centroid: [f64; 2],
    /// [min_x, min_y, max_x, max_y] of the source contour.
    pub bbox: [f64; 4],
    pub area_px: f64,
    pub perimeter_px: f64,
    /// Harmonics sorted by magnitude, largest first.
    pub harmonics: Vec<Harmonic>,
    /// 笔画/轮廓主色（原图区域颜色分类后的代表色 RGB）。
    pub color: [u8; 3],
    /// 颜色类别中文名（红/橙/黄/绿/青/蓝/紫/粉/棕/黑/白/灰）。
    pub color_name: String,
    /// 原始笔画路径（未闭合/未重采样，渲染彩色线稿用；不序列化）。
    #[serde(skip)]
    pub raw_path: Vec<P2>,
    /// 该笔画的绘制起始时间（tick，可自定义；默认 0=立即开始）。
    pub start: u32,
    /// 简化名称（线索引/脚本引用用；默认 "L{id}"，可用 --line-names 覆盖）。
    pub name: String,
    /// 自动语义名（启发式参考：outer/ring/arc/detail；仅参考，不保证准确）。
    pub hint: String,
}

impl ContourTransform {
    /// Evaluate the truncated Fourier series at t in [0,1) -> image coords.
    pub fn evaluate(&self, t: f64) -> P2 {
        let (qr, qi) = sum_all(&self.harmonics, t);
        P2::new(self.centroid[0] + qr, self.centroid[1] + qi)
    }

    /// Cumulative harmonic tips (epicycle chain) for drawing: chain[0] is the
    /// centroid, chain[i] the partial sum after i circles.
    pub fn evaluate_chain(&self, t: f64) -> Vec<P2> {
        let mut chain = Vec::with_capacity(self.harmonics.len() + 1);
        chain.push(P2::new(self.centroid[0], self.centroid[1]));
        let two_pi = std::f64::consts::TAU;
        let mut qr = 0.0;
        let mut qi = 0.0;
        for h in &self.harmonics {
            let th = two_pi * h.k as f64 * t;
            let (s, c) = th.sin_cos();
            qr += h.real * c - h.imag * s;
            qi += h.real * s + h.imag * c;
            chain.push(P2::new(self.centroid[0] + qr, self.centroid[1] + qi));
        }
        chain
    }
}

#[inline]
fn sum_all(harmonics: &[Harmonic], t: f64) -> (f64, f64) {
    let two_pi = std::f64::consts::TAU;
    let mut qr = 0.0;
    let mut qi = 0.0;
    for h in harmonics {
        let th = two_pi * h.k as f64 * t;
        let (s, c) = th.sin_cos();
        qr += h.real * c - h.imag * s;
        qi += h.real * s + h.imag * c;
    }
    (qr, qi)
}

/// Compute the normalized DFT of `points`, keep the `n_harmonics` largest
/// harmonics (sorted by magnitude), and package everything for output.
///
/// `raw_path`/`color`/`color_name` 来自原图区域颜色分类（见 lib.rs 组装处）。
pub fn forward_fft(
    points: &[P2],
    n_harmonics: usize,
    id: usize,
    raw_path: Vec<P2>,
    color: [u8; 3],
    color_name: String,
) -> ContourTransform {
    let n = points.len();
    assert!(n >= 2, "need at least 2 points for a DFT");
    let mut cx = 0.0;
    let mut cy = 0.0;
    for p in points {
        cx += p.x;
        cy += p.y;
    }
    cx /= n as f64;
    cy /= n as f64;

    let mut data: Vec<Complex<f64>> = points
        .iter()
        .map(|p| Complex64::new(p.x - cx, p.y - cy))
        .collect();
    let mut planner = FftPlanner::<f64>::new();
    let fft = planner.plan_fft_forward(n);
    fft.process(&mut data);

    // data[k] = sum_n z[n] * exp(-2*pi*i*k*n/N)  (unnormalized), k = 0..N-1.
    // Map to signed frequencies and normalize by 1/N.
    let mut harmonics: Vec<Harmonic> = (0..n)
        .map(|k| {
            let c = data[k] / n as f64;
            let signed = if k as i64 <= n as i64 / 2 {
                k as i32
            } else {
                k as i32 - n as i32
            };
            Harmonic {
                k: signed,
                real: c.re,
                imag: c.im,
            }
        })
        .collect();
    harmonics.sort_by(|a, b| b.mag().partial_cmp(&a.mag()).unwrap_or(std::cmp::Ordering::Equal));
    let keep = n_harmonics.min(n);
    harmonics.truncate(keep);

    let bbox = BBox::from_points(points);
    ContourTransform {
        id,
        n_samples: n,
        n_harmonics: harmonics.len(),
        centroid: [cx, cy],
        bbox: [bbox.min_x, bbox.min_y, bbox.max_x, bbox.max_y],
        area_px: signed_area(points).abs(),
        perimeter_px: arc_length(points),
        harmonics,
        color,
        color_name,
        raw_path,
        start: 0,
        name: format!("L{id}"),
        hint: String::new(),
    }
}