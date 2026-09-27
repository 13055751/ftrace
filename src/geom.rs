//! Minimal 2D geometry helpers for contour processing.
//!
//! Reference/technique source: classic computational-geometry texts; the
//! contour/Fourier pipeline follows open-source projects:
//!   - img2svg (Apache-2.0, github.com/yingkitw/img2svg): Sobel + marching squares
//!   - seiza-imgproc (Apache-2.0, github.com/theatrus/seiza): OpenCV-port Canny/contours
//!   - fluffy-eureka / circles-sketch: contour -> complex DFT -> epicycles
//!   - Coding Train #130 (MIT-ish teaching material): DFT-of-path + epicycle chain

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct P2 {
    pub x: f64,
    pub y: f64,
}

impl P2 {
    pub fn new(x: f64, y: f64) -> Self {
        P2 { x, y }
    }
}

/// Axis-aligned bounding box.
#[derive(Clone, Copy, Debug, Default)]
pub struct BBox {
    pub min_x: f64,
    pub min_y: f64,
    pub max_x: f64,
    pub max_y: f64,
}

impl BBox {
    pub fn from_points(pts: &[P2]) -> BBox {
        let mut b = BBox {
            min_x: f64::INFINITY,
            min_y: f64::INFINITY,
            max_x: f64::NEG_INFINITY,
            max_y: f64::NEG_INFINITY,
        };
        for p in pts {
            b.min_x = b.min_x.min(p.x);
            b.min_y = b.min_y.min(p.y);
            b.max_x = b.max_x.max(p.x);
            b.max_y = b.max_y.max(p.y);
        }
        b
    }
}

/// Shoelace (signed) area of a simple closed polygon. Positive => counter-clockwise.
pub fn signed_area(pts: &[P2]) -> f64 {
    let n = pts.len();
    if n < 3 {
        return 0.0;
    }
    let mut s = 0.0;
    for i in 0..n {
        let j = (i + 1) % n;
        s += pts[i].x * pts[j].y - pts[j].x * pts[i].y;
    }
    s * 0.5
}

pub fn arc_length(pts: &[P2]) -> f64 {
    let n = pts.len();
    if n == 0 {
        return 0.0;
    }
    let mut s = 0.0;
    for i in 0..n {
        let j = (i + 1) % n;
        let dx = pts[j].x - pts[i].x;
        let dy = pts[j].y - pts[i].y;
        s += (dx * dx + dy * dy).sqrt();
    }
    s
}