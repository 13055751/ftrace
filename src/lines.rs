//! 线导出模式（--line-sheet）：每条笔画分配调色板预留色 + 序号标注。
//!
//! 输出两张图：
//!   - 线索引图 `STEM.lines.png`：每条线按预留色绘制 + bbox 中心点阵数字序号，
//!     用于在 .efx 脚本里按序号/名称逐条引用。
//!   - 合成预览图 `STEM.lines.preview.png`：所有线按预留色合成（无标注），
//!     用于整体预览。
//!
//! 调色板采用 golden-angle 色相分布（区分度大、任意相邻色差异明显），
//! 数字用内置 5×7 点阵字体（零依赖，无需字体库）。

use image::{Rgb, RgbImage};
use imageproc::drawing::draw_line_segment_mut;

use crate::fourier::ContourTransform;
use crate::render::RenderCfg;

/// HSV -> RGB（h∈[0,360)，s/v∈[0,1]）。
pub fn hsv_to_rgb(h: f64, s: f64, v: f64) -> [u8; 3] {
    let c = v * s;
    let hp = (h / 60.0).rem_euclid(6.0);
    let x = c * (1.0 - (hp.rem_euclid(2.0) - 1.0).abs());
    let (r, g, b) = match hp.floor() as i32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = v - c;
    [
        ((r + m) * 255.0).round().clamp(0.0, 255.0) as u8,
        ((g + m) * 255.0).round().clamp(0.0, 255.0) as u8,
        ((b + m) * 255.0).round().clamp(0.0, 255.0) as u8,
    ]
}

/// 区分度最大的调色板第 i 色（golden-angle 色相分布，饱和度/亮度固定）。
pub fn palette(i: usize) -> [u8; 3] {
    let hue = (i as f64 * 137.508).rem_euclid(360.0);
    hsv_to_rgb(hue, 0.72, 0.88)
}

/// 语义启发式名称：周长最大 = outer；细长 = arc；小碎片 = detail；其余 ring。
/// 仅作参考（脚本引用用稳定序号名 L{i} 或 --line-names 自定义）。
pub fn hint_of(tr: &ContourTransform, idx: usize) -> String {
    let [minx, miny, maxx, maxy] = tr.bbox;
    let w = maxx - minx;
    let h = maxy - miny;
    let aspect = if h > 0.0 { w / h } else { f64::INFINITY };
    let thin = aspect > 3.0 || aspect < 0.34;
    if idx == 0 {
        "outer".into()
    } else if tr.perimeter_px < 60.0 {
        format!("detail_{idx}")
    } else if thin {
        format!("arc_{idx}")
    } else {
        format!("ring_{idx}")
    }
}

/// 5×7 点阵数字（每行 5 位，LSB=左）。
const DIGITS: [[u8; 7]; 10] = [
    [0b01110, 0b10001, 0b10011, 0b10101, 0b11001, 0b10001, 0b01110], // 0
    [0b00100, 0b01100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110], // 1
    [0b01110, 0b10001, 0b00001, 0b00010, 0b00100, 0b01000, 0b11111], // 2
    [0b11111, 0b00010, 0b00100, 0b00010, 0b00001, 0b10001, 0b01110], // 3
    [0b00010, 0b00110, 0b01010, 0b10010, 0b11111, 0b00010, 0b00010], // 4
    [0b11111, 0b10000, 0b11110, 0b00001, 0b00001, 0b10001, 0b01110], // 5
    [0b00110, 0b01000, 0b10000, 0b11110, 0b10001, 0b10001, 0b01110], // 6
    [0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b01000, 0b01000], // 7
    [0b01110, 0b10001, 0b10001, 0b01110, 0b10001, 0b10001, 0b01110], // 8
    [0b01110, 0b10001, 0b10001, 0b01111, 0b00001, 0b00010, 0b01100], // 9
];

/// 在 (x,y)（数字左上角）绘制整数序号，每点放大 `scale` 倍。
pub fn draw_number(img: &mut RgbImage, x: i32, y: i32, num: usize, color: Rgb<u8>, scale: u32) {
    let s = scale.max(1);
    let digits: Vec<u8> = if num == 0 {
        vec![0]
    } else {
        let mut n = num;
        let mut v = Vec::new();
        while n > 0 {
            v.push((n % 10) as u8);
            n /= 10;
        }
        v.reverse();
        v
    };
    let (w, h) = img.dimensions();
    let mut dx = x;
    for &d in &digits {
        for (row, bits) in DIGITS[d as usize].iter().enumerate() {
            for col in 0..5 {
                if bits & (1 << (4 - col)) != 0 {
                    for sy in 0..s {
                        for sx in 0..s {
                            let px = dx + col as i32 * s as i32 + sx as i32;
                            let py = y + row as i32 * s as i32 + sy as i32;
                            if px >= 0 && py >= 0 && (px as u32) < w && (py as u32) < h {
                                img.put_pixel(px as u32, py as u32, color);
                            }
                        }
                    }
                }
            }
        }
        dx += (5 + 1) * s as i32; // 数字间空一列
    }
}

/// 画一条线的傅里叶重建（闭合折线，按周长自适应步数）。
fn draw_line(img: &mut RgbImage, cfg: &RenderCfg, tr: &ContourTransform, color: Rgb<u8>) {
    let perim_px = tr.perimeter_px * cfg.scale;
    let steps = (perim_px / 0.4).round().clamp(64.0, 720.0) as usize;
    let mut prev: Option<crate::geom::P2> = None;
    for i in 0..=steps {
        let t = i as f64 / steps as f64;
        let p = tr.evaluate(t);
        if let Some(q) = prev {
            let p0 = ((q.x * cfg.scale) as f32, (q.y * cfg.scale) as f32);
            let p1 = ((p.x * cfg.scale) as f32, (p.y * cfg.scale) as f32);
            draw_line_segment_mut(img, p0, p1, color);
        }
        prev = Some(p);
    }
}

/// 生成线索引图（标注序号）或合成预览图（无标注）。
/// `annotate=true` 时每条线在 bbox 中心上方标注序号。
pub fn draw_line_sheet(cfg: &RenderCfg, trs: &[ContourTransform], annotate: bool) -> RgbImage {
    let mut img = RgbImage::from_pixel(cfg.width, cfg.height, Rgb([255, 255, 255]));
    // 先画全部线（预留色），再标注序号，避免序号被线覆盖
    for tr in trs {
        draw_line(&mut img, cfg, tr, Rgb([tr.color[0], tr.color[1], tr.color[2]]));
    }
    if annotate {
        const LABEL_SCALE: u32 = 2;
        let (w, h) = img.dimensions();
        for (i, tr) in trs.iter().enumerate() {
            let cx = (((tr.bbox[0] + tr.bbox[2]) / 2.0) * cfg.scale) as i32;
            let cy = (((tr.bbox[1] + tr.bbox[3]) / 2.0) * cfg.scale) as i32;
            let label = i;
            let num_w = (label.to_string().len() as i32) * 6 * LABEL_SCALE as i32;
            let num_h = 7 * LABEL_SCALE as i32;
            let bx = (cx - num_w / 2).clamp(0, w as i32 - num_w);
            let by = (cy - num_h / 2).clamp(0, h as i32 - num_h);
            draw_number(&mut img, bx, by, label, Rgb([35, 35, 35]), LABEL_SCALE);
        }
    }
    img
}
