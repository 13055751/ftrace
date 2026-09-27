//! 基本色分类（HSV 阈值法，与 tools/ftrace_color.py 的 12 类保持一致）。
//!
//! 用途：给每条傅里叶笔画按原图区域主色染色 —— 线稿线条自带颜色，
//! 方便法阵绘制时函数取色。

use image::RgbImage;
use crate::geom::P2;

/// 返回 (类别中文名, 代表色 RGB)。
pub fn classify(rgb: [u8; 3]) -> (&'static str, [u8; 3]) {
    let (r, g, b) = (rgb[0] as f64, rgb[1] as f64, rgb[2] as f64);
    let mx = r.max(g).max(b);
    let mn = r.min(g).min(b);
    let v = mx;
    let d = mx - mn;
    let s = if mx == 0.0 { 0.0 } else { d / mx * 255.0 };
    let h = if d == 0.0 {
        0.0
    } else if mx == r {
        (60.0 * ((g - b) / d)).rem_euclid(360.0)
    } else if mx == g {
        60.0 * ((b - r) / d + 2.0)
    } else {
        60.0 * ((r - g) / d + 4.0)
    };
    // (name, rep_rgb, condition)
    // 注意顺序：棕(深色暖色)先于橙/黄判断，否则深橙会被误判为橙
    let cats: [(&str, [u8; 3], bool); 12] = [
        ("黑", [30, 30, 35], v < 34.0),
        ("白", [245, 245, 245], s < 20.0 && v > 225.0),
        ("灰", [150, 150, 150], s < 20.0),
        ("红", [214, 45, 55], s >= 20.0 && (h < 12.0 || h >= 348.0)),
        ("棕", [128, 84, 48], s >= 20.0 && v < 170.0 && (12.0..68.0).contains(&h)),
        ("橙", [232, 126, 38], s >= 20.0 && (12.0..35.0).contains(&h)),
        ("黄", [232, 198, 48], s >= 20.0 && (35.0..68.0).contains(&h) && v >= 170.0),
        ("绿", [64, 168, 76], s >= 20.0 && (68.0..158.0).contains(&h)),
        ("青", [52, 188, 188], s >= 20.0 && (158.0..192.0).contains(&h)),
        ("蓝", [52, 106, 226], s >= 20.0 && (192.0..262.0).contains(&h)),
        ("紫", [146, 88, 216], s >= 20.0 && (262.0..305.0).contains(&h)),
        ("粉", [232, 126, 178], s >= 20.0 && (305.0..348.0).contains(&h)),
    ];
    for (name, rep, cond) in cats {
        if cond {
            return (name, rep);
        }
    }
    ("灰", [150, 150, 150])
}

/// 从原图采样一条笔画周围(3×3 邻域, 封闭轮廓另采内部一圈)的主色类别,
/// 返回 (代表色 RGB, 类别名)。优先非白/灰/黑的彩色类别, 否则取深色。
pub fn dominant_color(path: &[P2], rgb: &RgbImage, closed: bool) -> ([u8; 3], String) {
    let (w, h) = rgb.dimensions();
    let mut counts: Vec<(&'static str, usize)> = Vec::new();
    let mut bump = |name: &'static str| {
        if let Some(e) = counts.iter_mut().find(|(n, _)| *n == name) {
            e.1 += 1;
        } else {
            counts.push((name, 1));
        }
    };
    let step = (path.len() / 160).max(1);
    for i in (0..path.len()).step_by(step) {
        let (x, y) = (path[i].x, path[i].y);
        for dx in -1i32..=1 {
            for dy in -1i32..=1 {
                let xx = (x as i32 + dx).max(0);
                let yy = (y as i32 + dy).max(0);
                if (xx as u32) < w && (yy as u32) < h {
                    let p = rgb.get_pixel(xx as u32, yy as u32);
                    bump(classify([p.0[0], p.0[1], p.0[2]]).0);
                }
            }
        }
    }
    if closed && path.len() > 8 {
        let (mut sx, mut sy) = (0.0f64, 0.0f64);
        for p in path {
            sx += p.x;
            sy += p.y;
        }
        let (cx, cy) = (sx / path.len() as f64, sy / path.len() as f64);
        let rad = (w.min(h) as f64) * 0.012;
        for k in 0..24 {
            let ang = k as f64 / 24.0 * std::f64::consts::TAU;
            let xx = (cx + rad * ang.cos()).round() as i32;
            let yy = (cy + rad * ang.sin()).round() as i32;
            if xx >= 0 && yy >= 0 && (xx as u32) < w && (yy as u32) < h {
                let p = rgb.get_pixel(xx as u32, yy as u32);
                bump(classify([p.0[0], p.0[1], p.0[2]]).0);
            }
        }
    }
    counts.sort_by(|a, b| b.1.cmp(&a.1));
    let first_color = counts.iter().find(|(n, _)| !matches!(*n, "白" | "灰" | "黑"));
    match first_color {
        Some((name, _)) => (classify_rep(name), name.to_string()),
        None => {
            if counts.iter().any(|(n, _)| *n == "黑") {
                ([40, 40, 45], "黑".to_string())
            } else {
                ([90, 90, 90], "灰".to_string())
            }
        }
    }
}

fn classify_rep(name: &str) -> [u8; 3] {
    match name {
        "黑" => [30, 30, 35],
        "白" => [245, 245, 245],
        "灰" => [150, 150, 150],
        "红" => [214, 45, 55],
        "橙" => [232, 126, 38],
        "黄" => [232, 198, 48],
        "绿" => [64, 168, 76],
        "青" => [52, 188, 188],
        "蓝" => [52, 106, 226],
        "紫" => [146, 88, 216],
        "粉" => [232, 126, 178],
        _ => [128, 84, 48],
    }
}