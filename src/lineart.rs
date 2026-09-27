//! Image -> black & white line art.
//!
//! Three strategies, inspired by open-source projects:
//!   - `Edge`: Canny edges (OpenCV / seiza-imgproc style, implemented in
//!     `canny.rs`), best for photos and detailed illustrations.
//!   - `Line`: direct binarization of an *already line-art* input (thin
//!     strokes). NO Canny — Canny would detect both sides of each thin
//!     stroke and double-draw it. Best for existing line art / sketches.
//!   - `Mask`: foreground/alpha mask -> outline ring (boundary of the mask),
//!     best for solid shapes, logos and transparent PNGs (flat-color images,
//!     the img2svg "quantize -> contour" spirit).
//!
//! Internal binary convention: 255 = ink (stroke), 0 = paper.

use image::{DynamicImage, GrayImage, Luma};
use crate::canny::{canny, CannyParams};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LineArtMode {
    Auto,
    Edge,
    Line,
    Mask,
}

pub struct LineArtCfg {
    pub mode: LineArtMode,
    pub blur: f32,
    pub canny_low: f32,
    pub canny_high: f32,
    /// Invert the polarity of the foreground selection in mask/line mode
    /// (dark ink on light paper vs. light subject on dark background).
    pub invert: bool,
}

impl Default for LineArtCfg {
    fn default() -> Self {
        LineArtCfg {
            mode: LineArtMode::Auto,
            blur: 1.2,
            canny_low: 0.08,
            canny_high: 0.22,
            invert: false,
        }
    }
}

pub fn has_alpha(img: &DynamicImage) -> bool {
    img.color().has_alpha()
}

/// 输出两套二进制：细线稿(展示用) + 轮廓追踪输入。
///
/// edge/line 模式两者相同(细笔画 -> 笔画链追踪)；
/// mask 模式 art=掩膜外轮廓环(线稿观感), trace_input=填充掩膜本身
/// (直接追踪填充掩膜能得到干净的外形轮廓(+内孔)环)。
pub struct LineArtResult {
    pub art: GrayImage,
    pub trace_input: GrayImage,
    /// true = edge/line 模式(细笔画 -> 笔画链追踪);
    /// false = mask 模式(填充掩膜 -> 外轮廓环追踪)
    pub is_edge: bool,
}

/// Produce the binary line-art image (255 = ink).
pub fn to_lineart(img: &DynamicImage, cfg: &LineArtCfg) -> LineArtResult {
    match cfg.mode {
        LineArtMode::Line => {
            // 已有线稿：直接二值化取笔画，不跑 Canny（Canny 会把细线双描）。
            // 二值化后笔画常是 2px 粗（JPEG 抗锯齿+阈值）——先 Zhang-Suen 细化
            // 到 1px 中心线，再闭运算桥接残缺口子，链追踪才能拿到完整笔画。
            let gray = img.to_luma8();
            let mut mask = auto_foreground(&gray, cfg.invert);
            mask = thin(&mask);
            mask = close_gaps(&mask);
            LineArtResult {
                art: mask.clone(),
                trace_input: mask,
                is_edge: true,
            }
        }
        _ => {
            let use_edge = match cfg.mode {
                LineArtMode::Edge => true,
                LineArtMode::Mask => false,
                LineArtMode::Auto => !has_alpha(img),
                LineArtMode::Line => unreachable!(),
            };
            if use_edge {
                let gray = img.to_luma8();
                let mut e = canny(
                    &gray,
                    &CannyParams {
                        sigma: cfg.blur,
                        low: cfg.canny_low,
                        high: cfg.canny_high,
                    },
                );
                if cfg.invert {
                    invert(&mut e);
                }
                LineArtResult { art: e.clone(), trace_input: e, is_edge: true }
            } else {
                let mask: GrayImage = if has_alpha(img) {
                    let rgba = img.to_rgba8();
                    let (w, h) = rgba.dimensions();
                    let mut m = GrayImage::new(w, h);
                    for (x, y, p) in rgba.enumerate_pixels() {
                        if p.0[3] >= 128 {
                            m.put_pixel(x, y, Luma([255u8]));
                        }
                    }
                    m
                } else {
                    let gray = img.to_luma8();
                    auto_foreground(&gray, cfg.invert)
                };
                let art = outline_ring(&mask);
                LineArtResult { art, trace_input: mask, is_edge: false }
            }
        }
    }
}

/// Zhang-Suen 细化：把 2px+ 粗的笔画骨架化为 1px 中心线。
/// 经典算法（经典图像处理文献；实现为常规公开算法）。
pub fn thin(mask: &GrayImage) -> GrayImage {
    let (w, h) = mask.dimensions();
    let mut cur = mask.clone();
    let mut changed = true;
    while changed {
        changed = false;
        for phase in 0..2 {
            let mut rem: Vec<(u32, u32)> = Vec::new();
            for y in 1..h.saturating_sub(1) {
                for x in 1..w.saturating_sub(1) {
                    if cur.get_pixel(x, y).0[0] < 128 {
                        continue;
                    }
                    let p = |dx: i32, dy: i32| -> u8 {
                        if cur
                            .get_pixel((x as i32 + dx) as u32, (y as i32 + dy) as u32)
                            .0[0]
                            >= 128
                        {
                            1
                        } else {
                            0
                        }
                    };
                    let p2 = p(0, -1);
                    let p3 = p(1, -1);
                    let p4 = p(1, 0);
                    let p5 = p(1, 1);
                    let p6 = p(0, 1);
                    let p7 = p(-1, 1);
                    let p8 = p(-1, 0);
                    let p9 = p(-1, -1);
                    let b = p2 + p3 + p4 + p5 + p6 + p7 + p8 + p9;
                    if !(2..=6).contains(&b) {
                        continue;
                    }
                    let seq = [p2, p3, p4, p5, p6, p7, p8, p9, p2];
                    let a = seq.windows(2).filter(|w| w[0] == 0 && w[1] == 1).count();
                    if a != 1 {
                        continue;
                    }
                    let cond = if phase == 0 {
                        p2 * p4 * p6 == 0 && p4 * p6 * p8 == 0
                    } else {
                        p2 * p4 * p8 == 0 && p2 * p6 * p8 == 0
                    };
                    if cond {
                        rem.push((x, y));
                    }
                }
            }
            for &(x, y) in &rem {
                cur.put_pixel(x, y, Luma([0u8]));
            }
            changed |= !rem.is_empty();
        }
    }
    cur
}

/// 3×3 形态学闭运算（先膨胀后腐蚀）：桥接细线稿上的 1px 断口、
/// 去掉 1px 毛刺，让笔画保持完整（seiza-imgproc, Apache-2.0）。
pub fn close_gaps(binary: &GrayImage) -> GrayImage {
    let (w, h) = binary.dimensions();
    let se = seiza_imgproc::morphology::StructuringElement::new(
        seiza_imgproc::morphology::KernelShape::Rect,
        3,
    );
    let border = seiza_imgproc::morphology::MorphBorder::Ignore;
    let dilated = seiza_imgproc::morphology::dilate(binary.as_raw(), w as usize, h as usize, &se, border);
    let closed = seiza_imgproc::morphology::erode(&dilated, w as usize, h as usize, &se, border);
    GrayImage::from_raw(w, h, closed).unwrap_or_else(|| binary.clone())
}

/// Otsu 二值化 + 自动极性选择，返回 255=前景(墨水)。
///
/// 自动极性：背景通常占据图像边框 -> 边框主色视为背景, 前景取其反色；
/// 边框两色混杂时(主体顶到边框)退回"墨水是少数派"规则；--invert 强制亮色为前景。
pub fn auto_foreground(gray: &GrayImage, invert: bool) -> GrayImage {
    let (w, h) = gray.dimensions();
    let level = imageproc::contrast::otsu_level(gray);
    let bin = imageproc::contrast::threshold(gray, level, imageproc::contrast::ThresholdType::Binary); // 255 = 亮类
    let bin_inv = invert_img(&bin); // 255 = 暗类
    let area = |im: &GrayImage| -> usize { im.pixels().filter(|p| p.0[0] >= 128).count() };
    let (fg_bin, fg_inv) = (area(&bin), area(&bin_inv));
    if std::env::var("FT_DEBUG_MASK").is_ok() {
        eprintln!(
            "[ftrace-debug] otsu_level={} fg_bin={} fg_inv={} img={}x{}",
            level, fg_bin, fg_inv, w, h
        );
    }
    let mut border_light = 0usize;
    let mut border_n = 0usize;
    if w > 2 && h > 2 {
        for x in 0..w {
            for &y in &[0u32, h - 1] {
                border_n += 1;
                if gray.get_pixel(x, y).0[0] > level {
                    border_light += 1;
                }
            }
        }
        for y in 1..h - 1 {
            for &x in &[0u32, w - 1] {
                border_n += 1;
                if gray.get_pixel(x, y).0[0] > level {
                    border_light += 1;
                }
            }
        }
    }
    let light_ratio = if border_n > 0 {
        border_light as f64 / border_n as f64
    } else {
        0.5
    };
    if invert {
        bin // 用户声明: 亮色主体
    } else if light_ratio >= 0.65 {
        bin_inv // 边框亮 -> 背景亮 -> 前景暗
    } else if light_ratio <= 0.35 {
        bin // 边框暗 -> 背景暗 -> 前景亮
    } else if fg_bin < fg_inv {
        bin // 边框混杂 -> 退回少数派
    } else {
        bin_inv
    }
}

/// 1-px outline ring of a filled mask (line-art look for solid shapes).
pub fn outline_ring(mask: &GrayImage) -> GrayImage {
    let (w, h) = mask.dimensions();
    let ink = |x: i64, y: i64| -> bool {
        x >= 0
            && y >= 0
            && (x as u32) < w
            && (y as u32) < h
            && mask.get_pixel(x as u32, y as u32).0[0] >= 128
    };
    let mut out = GrayImage::new(w, h);
    for y in 0..h {
        for x in 0..w {
            if mask.get_pixel(x, y).0[0] < 128 {
                continue;
            }
            let mut boundary = false;
            for dy in -1i64..=1 {
                for dx in -1i64..=1 {
                    if dx == 0 && dy == 0 {
                        continue;
                    }
                    if !ink(x as i64 + dx, y as i64 + dy) {
                        boundary = true;
                    }
                }
            }
            if boundary {
                out.put_pixel(x, y, Luma([255u8]));
            }
        }
    }
    out
}

fn invert(img: &mut GrayImage) {
    for p in img.pixels_mut() {
        p.0[0] = 255 - p.0[0];
    }
}

fn invert_img(img: &GrayImage) -> GrayImage {
    let mut out = img.clone();
    invert(&mut out);
    out
}

/// Convert the internal binary (255 = ink) to a display image with ink on
/// white paper (classic "line art" look).
pub fn display(img: &GrayImage) -> GrayImage {
    let mut out = img.clone();
    for p in out.pixels_mut() {
        p.0[0] = 255 - p.0[0];
    }
    out
}