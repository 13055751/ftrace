//! Raster rendering: epicycle chains + progressive trace, PNG preview frames.
//!
//! Drawing primitives come from imageproc (MIT), a well-known open-source
//! image-processing crate; the epicycle chain visualization follows The
//! Coding Train #130 / fluffy-eureka style (cumulative circle chain, sorted
//! harmonics, growing trace line).

use image::{Rgb, RgbImage};
use imageproc::drawing::{draw_hollow_circle_mut, draw_line_segment_mut};

use crate::fourier::ContourTransform;
use crate::geom::P2;

pub const TRACE: Rgb<u8> = Rgb([210, 40, 40]);
pub const CIRCLE: Rgb<u8> = Rgb([120, 130, 165]);
pub const ARM: Rgb<u8> = Rgb([200, 205, 220]);

pub struct RenderCfg {
    pub width: u32,
    pub height: u32,
    /// Animation frame count (also the trace granularity).
    pub frames: u32,
    /// Scale factor used when downscaling the canvas (image coords -> px).
    pub scale: f64,
    /// Whether to draw the epicycle "machinery" (circles + arms).
    pub show_circles: bool,
    /// Draw circle discs only for the first `max_circles` harmonics (per
    /// contour per frame) — visually equivalent (small harmonics are
    /// sub-pixel) but much cheaper to render.
    pub max_circles: usize,
    /// Draw the chain arm only for the first `max_arms` harmonics; the tail
    /// harmonics are sub-pixel and invisible.
    pub max_arms: usize,
    /// 彩色模式：还原轨迹按每条笔画/轮廓的原图区域主色绘制（默认开）。
    pub colored: bool,
}

impl RenderCfg {
    /// Build a config from source image dims, optionally capping the longest
    /// edge to `max_dim` pixels.
    pub fn from_dims(
        w: u32,
        h: u32,
        frames: u32,
        max_dim: Option<u32>,
        show_circles: bool,
        colored: bool,
    ) -> Self {
        let longest = w.max(h) as f64;
        let scale = match max_dim {
            Some(m) if m > 0 && longest > m as f64 => m as f64 / longest,
            _ => 1.0,
        };
        let cw = ((w as f64) * scale).round().max(1.0) as u32;
        let ch = ((h as f64) * scale).round().max(1.0) as u32;
        RenderCfg {
            width: cw,
            height: ch,
            frames: frames.max(2),
            scale,
            show_circles,
            max_circles: 96,
            max_arms: 160,
            colored,
        }
    }
}

#[inline]
fn sc(cfg: &RenderCfg, p: P2) -> (f32, f32) {
    ((p.x * cfg.scale) as f32, (p.y * cfg.scale) as f32)
}

pub fn new_canvas(cfg: &RenderCfg) -> RgbImage {
    RgbImage::from_pixel(cfg.width, cfg.height, Rgb([255, 255, 255]))
}

/// Draw one contour's epicycle chain (circles + arms) at time t in [0,1).
pub fn draw_chain(img: &mut RgbImage, cfg: &RenderCfg, tr: &ContourTransform, t: f64) {
    if !cfg.show_circles {
        return;
    }
    let chain = tr.evaluate_chain(t);
    for (i, pair) in chain.windows(2).enumerate() {
        let a = pair[0];
        let b = pair[1];
        let (ax, ay) = sc(cfg, a);
        let (bx, by) = sc(cfg, b);
        let r = tr.harmonics[i].mag() * cfg.scale;
        if i < cfg.max_circles && r >= 1.0 {
            draw_hollow_circle_mut(img, (ax.round() as i32, ay.round() as i32), r.round() as i32, CIRCLE);
        }
        if i < cfg.max_arms {
            draw_line_segment_mut(img, (ax, ay), (bx, by), ARM);
        }
    }
}

/// 笔画的主色（彩色模式用），否则回退经典红色。
#[inline]
fn tr_color(cfg: &RenderCfg, tr: &ContourTransform) -> Rgb<u8> {
    if cfg.colored {
        Rgb([tr.color[0], tr.color[1], tr.color[2]])
    } else {
        TRACE
    }
}

/// Draw the Fourier reconstruction as a closed polyline. Step count adapts
/// to the contour perimeter so segments stay above ~0.5px (sub-pixel
/// segments would otherwise collapse into dotted lines on small contours).
pub fn draw_reconstruction(img: &mut RgbImage, cfg: &RenderCfg, tr: &ContourTransform, color: Rgb<u8>) {
    let perim_px = tr.perimeter_px * cfg.scale;
    let steps = (perim_px / 0.4).round().clamp(64.0, 720.0) as usize;
    let mut prev: Option<P2> = None;
    for i in 0..=steps {
        let t = i as f64 / steps as f64;
        let p = tr.evaluate(t);
        if let Some(q) = prev {
            draw_line_segment_mut(img, sc(cfg, q), sc(cfg, p), color);
        }
        prev = Some(p);
    }
}

/// 彩色线稿：白底上按每条笔画的原图区域主色绘制其原始路径（宽 2px）。
/// 输出即"线条自带颜色"的线稿，供函数取色/法阵绘制参考。
pub fn draw_colored_lineart(img: &mut RgbImage, cfg: &RenderCfg, trs: &[ContourTransform]) {
    const OFFSETS: [(i32, i32); 4] = [(0, 0), (1, 0), (0, 1), (1, 1)];
    for tr in trs {
        let color = Rgb([tr.color[0], tr.color[1], tr.color[2]]);
        let path = &tr.raw_path;
        for &(ox, oy) in &OFFSETS {
            let mut prev: Option<P2> = None;
            for &p in path {
                if let Some(q) = prev {
                    let p0 = sc(cfg, q);
                    let p1 = sc(cfg, p);
                    draw_line_segment_mut(
                        img,
                        (p0.0 + ox as f32, p0.1 + oy as f32),
                        (p1.0 + ox as f32, p1.1 + oy as f32),
                        color,
                    );
                }
                prev = Some(p);
            }
        }
    }
}

/// Render one animation frame: full epicycle chain + trace grown up to
/// `frame_frac` (0..=1).
pub fn render_frame(cfg: &RenderCfg, trs: &[ContourTransform], frame_frac: f64) -> RgbImage {
    let mut img = new_canvas(cfg);
    for tr in trs {
        draw_chain(&mut img, cfg, tr, frame_frac);
        let nf = cfg.frames.saturating_sub(1) as f64;
        let upto = ((frame_frac * nf).round() as usize).min(cfg.frames as usize - 1);
        let mut prev: Option<P2> = None;
        for i in 0..=upto {
            let t = i as f64 / nf.max(1.0);
            let p = tr.evaluate(t);
            if let Some(q) = prev {
                draw_line_segment_mut(&mut img, sc(cfg, q), sc(cfg, p), tr_color(cfg, tr));
            }
            prev = Some(p);
        }
    }
    img
}

/// Draw all contours' final reconstructions onto an existing canvas.
pub fn draw_reconstructions(img: &mut RgbImage, cfg: &RenderCfg, trs: &[ContourTransform]) {
    for tr in trs {
        draw_reconstruction(img, cfg, tr, tr_color(cfg, tr));
    }
}

/// Fade the line art under the preview so reconstruction can be compared
/// against the source outline. Ink becomes a clearly visible light gray.
pub fn with_underlay(img: &mut RgbImage, art: &image::GrayImage, cfg: &RenderCfg) {
    let (w, h) = art.dimensions();
    for y in 0..h {
        for x in 0..w {
            if art.get_pixel(x, y).0[0] < 128 {
                continue;
            }
            let px = ((x as f64) * cfg.scale).round() as i32;
            let py = ((y as f64) * cfg.scale).round() as i32;
            if px >= 0 && py >= 0 && (px as u32) < cfg.width && (py as u32) < cfg.height {
                img.put_pixel(px as u32, py as u32, Rgb([205, 205, 205]));
            }
        }
    }
}