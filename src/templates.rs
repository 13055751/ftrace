//! 法阵模板库（--template）：参数化几何直接生成法阵线条，无需输入图片。
//!
//! 每个模板返回若干条闭合折线（像素坐标，画布 1024×1024、中心 (512,512)），
//! 走与图片线稿相同的下游管线（重采样 → 傅里叶 → coeffs.json / SVG / 预览 / GIF），
//! 天然兼容 --line-sheet（线索引图 + 预留色 + 稳定名称），一条命令进 Eldoria。
//!
//! 模板：
//!   - `hexagram`  六芒星：外环 + 上下两三角 + 内环
//!   - `starfield` 星辰阵：外环 + 五角星 + 内环 + 五角端点小星
//!   - `rune_ring` 符文环：外环 + 内环 + 对角辐条 ×4 + 四方符文 ×4
//!   - `element`   元素徽记：外环 + 内环 + 四元素徽记（火三角/水波/土方/空气弧）
//!
//! 名称稳定可预测（供 .efx `line <图形> <线名>` 直接引用），语义名见各模板。

use std::f64::consts::TAU;

use crate::geom::P2;

/// 一张参数化生成的法阵模板。
pub struct Template {
    /// 模板名（hexagram/starfield/rune_ring/element）。
    pub name: String,
    pub width: u32,
    pub height: u32,
    /// 每条线条的折线（像素坐标）。
    pub paths: Vec<Vec<P2>>,
    /// 每条线的稳定语义名（供 .efx 引用；--line-names 优先覆盖）。
    pub path_names: Vec<String>,
}

/// 生成模板；未知模板返回 None。
pub fn generate(name: &str) -> Option<Template> {
    let name = name.trim().to_ascii_lowercase();
    match name.as_str() {
        "hexagram" | "六芒星" => Some(hexagram()),
        "starfield" | "星辰阵" => Some(starfield()),
        "rune_ring" | "符文环" => Some(rune_ring()),
        "element" | "元素徽记" | "elemental" => Some(element()),
        _ => None,
    }
}

/// 全部可用模板名（含别名去重，用于 CLI 提示 / tab 补全）。
pub fn all_names() -> Vec<&'static str> {
    vec!["hexagram", "starfield", "rune_ring", "element"]
}

const W: u32 = 1024;
const H: u32 = 1024;
const CX: f64 = 512.0;
const CY: f64 = 512.0;
/// 主半径（像素）。
const R: f64 = 420.0;

fn finish(name: &str, mut paths: Vec<Vec<P2>>, path_names: Vec<String>) -> Template {
    // 过滤空路径（防御）
    paths.retain(|p| p.len() >= 2);
    Template {
        name: name.to_string(),
        width: W,
        height: H,
        paths,
        path_names,
    }
}

// ---------- 基础几何 ----------

/// 圆环折线（顺时针，首尾不闭合——闭合性由 resample_loop 保证）。
fn circle(r: f64, segments: usize) -> Vec<P2> {
    (0..segments)
        .map(|i| {
            let a = i as f64 * TAU / segments as f64;
            P2::new(CX + r * a.cos(), CY + r * a.sin())
        })
        .collect()
}

/// 正多边形折线（rot_deg 为起始顶点角度，度；闭合）。
fn polygon(r: f64, n: usize, rot_deg: f64) -> Vec<P2> {
    let mut pts: Vec<P2> = (0..n)
        .map(|i| {
            let a = (rot_deg + i as f64 * 360.0 / n as f64).to_radians();
            P2::new(CX + r * a.cos(), CY + r * a.sin())
        })
        .collect();
    pts.push(pts[0]);
    pts
}

/// 五角星（五外顶点按 +2 跳跃连接，单笔闭合路径）。
fn pentagram(r: f64) -> Vec<P2> {
    let outer: Vec<P2> = (0..5)
        .map(|i| {
            let a = (-90.0 + i as f64 * 72.0).to_radians();
            P2::new(CX + r * a.cos(), CY + r * a.sin())
        })
        .collect();
    let mut pts = Vec::with_capacity(6);
    for i in 0..5 {
        pts.push(outer[(i * 2) % 5]);
    }
    pts.push(outer[0]);
    pts
}

/// 五角星外顶点（供点缀小圆定位）。
fn pentagram_outer(r: f64) -> Vec<P2> {
    (0..5)
        .map(|i| {
            let a = (-90.0 + i as f64 * 72.0).to_radians();
            P2::new(CX + r * a.cos(), CY + r * a.sin())
        })
        .collect()
}

/// 圆心在任意位置的圆。
fn circle_at(cx: f64, cy: f64, r: f64, segments: usize) -> Vec<P2> {
    (0..segments)
        .map(|i| {
            let a = i as f64 * TAU / segments as f64;
            P2::new(cx + r * a.cos(), cy + r * a.sin())
        })
        .collect()
}

// ---------- 各模板 ----------

/// 六芒星：外环 + 上三角 + 下三角 + 内环。
fn hexagram() -> Template {
    let paths = vec![
        circle(R, 128),
        polygon(R, 3, -90.0),  // 上三角（顶点朝上）
        polygon(R, 3, 90.0),   // 下三角（顶点朝下）
        circle(R * 0.45, 96),
    ];
    let names = vec!["outer_ring".to_string(), "star_up".to_string(), "star_down".to_string(), "inner_ring".to_string()];
    finish("hexagram", paths, names)
}

/// 星辰阵：外环 + 五角星 + 内环 + 五角端点小星。
fn starfield() -> Template {
    let mut paths = vec![
        circle(R, 128),
        pentagram(R * 0.95),
        circle(R * 0.42, 96),
    ];
    let mut names: Vec<String> = vec!["outer_ring".into(), "pentagram".into(), "inner_ring".into()];
    for (i, p) in pentagram_outer(R * 0.95).into_iter().enumerate() {
        paths.push(circle_at(p.x, p.y, 24.0, 24));
        names.push(format!("dot_{i}"));
    }
    finish("starfield", paths, names)
}

/// 符文环：外环 + 内环 + 对角辐条 ×4 + 四方符文 ×4（径向竖杠 + 双刻痕）。
fn rune_ring() -> Template {
    let mut paths = vec![circle(R, 128), circle(R * 0.55, 96)];
    let mut names: Vec<String> = vec!["outer_ring".into(), "inner_ring".into()];
    for a in [45.0_f64, 135.0, 225.0, 315.0] {
        let rad = a.to_radians();
        let p0 = P2::new(CX + R * 0.55 * rad.cos(), CY + R * 0.55 * rad.sin());
        let p1 = P2::new(CX + R * rad.cos(), CY + R * rad.sin());
        paths.push(vec![p0, p1]);
        names.push(format!("spoke_{}", a as i32));
    }
    let mid = (R * 0.55 + R) / 2.0;
    let half = (R - R * 0.55) / 2.0 * 0.72;
    for a in [-90.0_f64, 0.0, 90.0, 180.0] {
        let rad = a.to_radians();
        let (rx, ry) = (rad.cos(), rad.sin());
        let (tx, ty) = (-rad.sin(), rad.cos());
        let (gx, gy) = (CX + mid * rx, CY + mid * ry);
        let p0 = P2::new(gx - rx * half, gy - ry * half);
        let p1 = P2::new(gx + rx * half, gy + ry * half);
        let t1 = P2::new(gx - rx * half * 0.85 + tx * 12.0, gy - ry * half * 0.85 + ty * 12.0);
        let t2 = P2::new(gx - rx * half * 0.85 - tx * 12.0, gy - ry * half * 0.85 - ty * 12.0);
        paths.push(vec![t1, p0, p1, t2]);
        names.push(format!("rune_{}", a as i32));
    }
    finish("rune_ring", paths, names)
}

/// 元素徽记：外环 + 内环 + 四元素徽记。
fn element() -> Template {
    let d = R * 0.62; // 徽记中心距圆心
    let s = R * 0.34; // 徽记尺寸
    let mut paths = vec![circle(R, 128), circle(R * 0.35, 96)];
    let mut names: Vec<String> = vec!["outer_ring".into(), "inner_ring".into()];
    // 火（北）：朝外的三角
    let tri: Vec<P2> = (0..3)
        .map(|i| {
            let a = (180.0 + i as f64 * 120.0).to_radians(); // 顶点朝上(-90)，底边朝下
            P2::new(CX + s * a.cos(), CY - d + s * a.sin())
        })
        .collect();
    paths.push({ let mut t = tri.clone(); t.push(t[0]); t });
    names.push("fire".into());
    // 水（东）：水平正弦波
    let wave: Vec<P2> = (0..12)
        .map(|i| {
            let x = -s + 2.0 * s * i as f64 / 11.0;
            P2::new(CX + d + x, CY + 7.0 * (x / s * std::f64::consts::PI).sin())
        })
        .collect();
    paths.push(wave);
    names.push("water".into());
    // 土（南）：方块
    paths.push(polygon_at(CX, CY + d, s, 4, 0.0));
    names.push("earth".into());
    // 气（西）：240° 弓形弧（朝外开口）
    let arc: Vec<P2> = (0..40)
        .map(|i| {
            let a = (60.0_f64 - 240.0 * i as f64 / 39.0).to_radians(); // 60°..-180°
            P2::new(CX - d + s * a.cos(), CY + s * a.sin())
        })
        .collect();
    paths.push(arc);
    names.push("air".into());
    finish("element", paths, names)
}

/// 圆心在任意位置的正多边形。
fn polygon_at(cx: f64, cy: f64, r: f64, n: usize, rot_deg: f64) -> Vec<P2> {
    let mut pts: Vec<P2> = (0..n)
        .map(|i| {
            let a = (rot_deg + i as f64 * 360.0 / n as f64).to_radians();
            P2::new(cx + r * a.cos(), cy + r * a.sin())
        })
        .collect();
    pts.push(pts[0]);
    pts
}

// ---------- 画布渲染（模板的线稿 PNG，黑线白底） ----------

use image::Rgb;
use image::RgbImage;
use imageproc::drawing::draw_line_segment_mut;

/// 把模板折线画成黑线白底 PNG（与图片模式的 lineart.png 对齐）。
pub fn draw_black_lineart(paths: &[Vec<P2>]) -> RgbImage {
    let mut img = RgbImage::from_pixel(W, H, Rgb([255, 255, 255]));
    for pts in paths {
        if pts.len() < 2 {
            continue;
        }
        for pair in pts.windows(2) {
            draw_line_segment_mut(
                &mut img,
                (pair[0].x as f32, pair[0].y as f32),
                (pair[1].x as f32, pair[1].y as f32),
                Rgb([0, 0, 0]),
            );
        }
    }
    img
}
