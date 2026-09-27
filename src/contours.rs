//! Ordered contour extraction from a binary line-art image + arc-length
//! uniform resampling.
//!
//! Border following is Moore-neighbor tracing with Jacob's stopping
//! criterion, applied to the 1-px boundary ring of the ink. The technique is
//! a classic from the contour-tracing literature and is the same approach
//! used by open-source projects referenced here:
//!   - img2svg (Apache-2.0): marching-squares contour extraction
//!   - seiza-imgproc (Apache-2.0): Suzuki-Abe border following
//!   - contour_tracing crate (EUPL-1.2, **not** used as a dependency -
//!     license incompatible with our MIT/Apache distribution, hence the
//!     inline implementation).

use image::GrayImage;
use crate::geom::{P2, arc_length, signed_area};

/// 外轮廓追踪(闭合掩膜轮廓) —— 委托给 seiza-imgproc 的 Suzuki-Abe 实现
/// (OpenCV `findContours(RETR_EXTERNAL)` 的纯 Rust 移植, Apache-2.0,
/// github.com/theatrus/seiza)。对厚斑/自触曲线都能正确给出每组件一个外轮廓,
/// 且返回拐点序列(CHAIN_APPROX_SIMPLE), 正好适合弧长均匀重采样。
pub fn trace_contours(art: &GrayImage) -> Vec<Vec<P2>> {
    let (w, h) = art.dimensions();
    let raw = art.as_raw();
    let contours = seiza_imgproc::contours::find_external_contours(raw, w as usize, h as usize);
    if std::env::var("FT_TRACE_STATS").is_ok() {
        eprintln!(
            "[ftrace-trace] contours={} img={}x{}",
            contours.len(),
            w,
            h
        );
    }
    contours
        .into_iter()
        .map(|c| c.into_iter().map(|(x, y)| P2::new(x as f64, y as f64)).collect())
        .collect()
}

#[derive(Clone, Copy, Debug)]
pub struct ContourFilter {
    /// Minimum absolute shoelace area (px) for a contour to be kept.
    pub min_area: f64,
    /// Keep at most this many contours (largest perimeter first).
    pub max_contours: usize,
}

#[derive(Clone, Copy, Debug)]
pub struct ChainFilter {
    /// Minimum chain length in px.
    pub min_len: f64,
    /// Keep at most this many chains (longest first).
    pub max_chains: usize,
}

/// 笔画链追踪：把二值线稿的细笔画提取为有序开放路径(链)。
///
/// 对 Canny 这类"细笔画"线稿, 正确的做法是沿着笔画中心线走, 而不是追踪
/// 边界环(那会把笔画变成包围区域的怪环、产生重叠乱线)。分支点(度>=3)
/// 作为断点切开, 每条臂独立成链; 闭合环(全度2)整圈提取为一条闭环链。
pub fn trace_stroke_chains(art: &GrayImage) -> Vec<Vec<P2>> {
    let (w, h) = art.dimensions();
    let pw = w + 2;
    let ph = h + 2;
    let mut ink = vec![false; (pw * ph) as usize];
    for y in 0..h {
        for x in 0..w {
            if art.get_pixel(x, y).0[0] >= 128 {
                ink[(y as usize + 1) * pw as usize + x as usize + 1] = true;
            }
        }
    }
    const DX: [i64; 8] = [0, 1, 1, 1, 0, -1, -1, -1];
    const DY: [i64; 8] = [-1, -1, 0, 1, 1, 1, 0, -1];
    let idx = |x: i64, y: i64| -> usize { (y as usize) * pw as usize + x as usize };
    let on = |x: i64, y: i64| -> bool {
        x >= 0 && y >= 0 && x < pw as i64 && y < ph as i64 && ink[idx(x, y)]
    };
    let deg = |x: i64, y: i64| -> usize {
        (0..8).filter(|&d| on(x + DX[d], y + DY[d])).count()
    };

    let mut visited = vec![false; (pw * ph) as usize];
    let mut chains: Vec<Vec<P2>> = Vec::new();
    let mut junctions: Vec<(i64, i64)> = Vec::new();
    for y in 0..ph {
        for x in 0..pw {
            let s = (x as i64, y as i64);
            let si = idx(s.0, s.1);
            if !ink[si] || visited[si] {
                continue;
            }
            if deg(s.0, s.1) >= 3 {
                // 分支点: 作为断点, 各臂随后各自成链
                visited[si] = true;
                junctions.push(s);
                continue;
            }
            let chain = walk_chain(&ink, &mut visited, pw as i64, ph as i64, s, &DX, &DY);
            let pts: Vec<P2> = chain
                .iter()
                .map(|&(cx, cy)| P2::new((cx - 1) as f64, (cy - 1) as f64))
                .collect();
            if pts.len() >= 4 {
                chains.push(pts);
            }
        }
    }
    // 交叉点接续 + 端点缝合: 把被交叉点/噪声断开的笔画片段重新拼成完整笔画,
    // 避免"长笔画在交叉处被切成短段、短段又被长度过滤丢光"以及"笔画断断续续"
    let before = chains.len();
    reconnect_chains(&mut chains, &junctions);
    stitch_chains(&mut chains);
    if std::env::var("FT_RECONNECT_STATS").is_ok() {
        eprintln!(
            "[ftrace-reconnect] junctions={} chains {} -> {} (merged {})",
            junctions.len(),
            before,
            chains.len(),
            before.saturating_sub(chains.len())
        );
    }
    chains
}

/// 端点缝合：任意两条链的端点距离 ≤ 阈值且走向连续（转弯 < 45°）时，
/// 把它们桥接成一条更长的链（桥接段直接连线补上缝隙）。
/// 贪心选最优对、迭代多轮，直到没有可缝合的对。
fn stitch_chains(chains: &mut Vec<Vec<P2>>) {
    let dir_into = |c: &[P2], end: usize| -> Option<(f64, f64)> {
        // end: 0=起点方向, 1=终点方向; 返回"进入端点"的单位方向(指向链外)
        if c.len() < 3 {
            return None;
        }
        let (a, b) = if end == 0 {
            (c[1], c[0])
        } else {
            (c[c.len() - 2], c[c.len() - 1])
        };
        let (dx, dy) = (b.x - a.x, b.y - a.y);
        let l = (dx * dx + dy * dy).sqrt();
        if l < 1e-9 {
            None
        } else {
            Some((dx / l, dy / l))
        }
    };
    loop {
        let mut best: Option<(usize, usize, usize, usize, f64)> = None; // (i, k, ae, be, cost)
        let n = chains.len();
        for i in 0..n {
            if chains[i].len() < 3 {
                continue;
            }
            for k in 0..n {
                if i == k || chains[k].len() < 3 {
                    continue;
                }
                for (ae, be) in [(1usize, 0usize), (0usize, 1usize)] {
                    let e_a = if ae == 0 {
                        *chains[i].first().unwrap()
                    } else {
                        *chains[i].last().unwrap()
                    };
                    let e_b = if be == 0 {
                        *chains[k].first().unwrap()
                    } else {
                        *chains[k].last().unwrap()
                    };
                    let d = (e_a.x - e_b.x).hypot(e_a.y - e_b.y);
                    if !(1.0..=8.0).contains(&d) {
                        continue;
                    }
                    let (Some(da), Some(db)) = (dir_into(&chains[i], ae), dir_into(&chains[k], be)) else {
                        continue;
                    };
                    // 缝隙方向: 从 A 端点指向 B 端点
                    let (gx, gy) = ((e_b.x - e_a.x) / d, (e_b.y - e_a.y) / d);
                    let ang_a = gx * da.0 + gy * da.1; // A 端方向与缝隙方向夹角余弦
                    let ang_b = gx * db.0 + gy * db.1; // B 端方向与缝隙方向夹角余弦(期望≈-1)
                    if ang_a < 0.707 || ang_b > -0.707 {
                        continue; // 转弯 >45° 或 B 方向不朝 A, 不算同一笔画
                    }
                    let cost = d + (1.0 - ang_a) * 8.0 + (1.0 + ang_b) * 8.0;
                    if best.map_or(true, |(_, _, _, _, c)| cost < c) {
                        best = Some((i, k, ae, be, cost));
                    }
                }
            }
        }
        let Some((i, k, ae, be, _)) = best else { break };
        let a = chains[i].clone();
        let b = chains[k].clone();
        let a_or: Vec<P2> = if ae == 0 { a.iter().rev().cloned().collect() } else { a };
        let b_or: Vec<P2> = if be == 1 { b.iter().rev().cloned().collect() } else { b };
        let mut merged = a_or;
        merged.extend(b_or);
        chains[i] = merged;
        chains[k] = Vec::new();
    }
    chains.retain(|c| !c.is_empty() && c.len() >= 4);
}

/// 在分支点处把"穿过同一交叉点的断链"重新接续成完整笔画。
///
/// 规则：同一 junction 上两条链的端点 8-邻接，且它们进入 junction 的方向
/// 大致相反（点积 < -0.7，夹角 > ~135°），视为同一条笔画的左右两段，
/// 经 junction 拼成一条。贪心两两配对，可迭代多轮（一条笔画可能穿过多个交叉点）。
fn reconnect_chains(chains: &mut Vec<Vec<P2>>, junctions: &[(i64, i64)]) {
    let near = |a: P2, b: (i64, i64)| -> bool {
        (a.x - b.0 as f64).abs() <= 1.0 && (a.y - b.1 as f64).abs() <= 1.0
    };
    let dir_into = |c: &[P2], which: usize| -> Option<(f64, f64)> {
        // which: 0=起点方向, 1=终点方向; 返回"进入端点"的单位方向
        if c.len() < 2 {
            return None;
        }
        let (d0, d1) = if which == 0 {
            (c[1], c[0])
        } else {
            (c[c.len() - 2], c[c.len() - 1])
        };
        let (dx, dy) = (d0.x - d1.x, d0.y - d1.y);
        let l = (dx * dx + dy * dy).sqrt();
        if l < 1e-9 {
            None
        } else {
            Some((dx / l, dy / l))
        }
    };
    // 每轮尝试合并; 一条笔画可能穿过多个交叉点, 迭代直到没有合并发生
    loop {
        let mut merged = false;
        // 对每个 junction, 收集邻接的链端点
        for &j in junctions {
            // (chain_index, which_end, direction_into_endpoint)
            let mut cands: Vec<(usize, usize, (f64, f64))> = Vec::new();
            for (ci, c) in chains.iter().enumerate() {
                if c.is_empty() {
                    continue;
                }
                if near(*c.first().unwrap(), j) {
                    if let Some(d) = dir_into(c, 0) {
                        cands.push((ci, 0, d));
                    }
                }
                if near(*c.last().unwrap(), j) {
                    if let Some(d) = dir_into(c, 1) {
                        cands.push((ci, 1, d));
                    }
                }
            }
            if cands.len() < 2 {
                continue;
            }
            // 贪心: 每次找夹角最大的相反配对
            let mut best: Option<(usize, usize)> = None;
            let mut best_dot = -0.7f64;
            for i in 0..cands.len() {
                for k in (i + 1)..cands.len() {
                    let (ai, _, da) = cands[i];
                    let (bi, _, db) = cands[k];
                    if ai == bi {
                        continue; // 同一链的两个端点都在此 junction(自环), 不合并
                    }
                    let dot = da.0 * db.0 + da.1 * db.1;
                    if dot < best_dot {
                        best_dot = dot;
                        best = Some((i, k));
                    }
                }
            }
            if let Some((i, k)) = best {
                let (ai, ae, _) = cands[i];
                let (bi, be, _) = cands[k];
                if chains[ai].len() + chains[bi].len() > 4 {
                    // 合并: 链A(按原序) + junction + 链B(翻转使邻接端贴近 junction)
                    let jp = P2::new(j.0 as f64, j.1 as f64);
                    let mut a = chains[ai].clone();
                    let b = &chains[bi];
                    let b_ordered: Vec<P2> = if be == 0 { b.clone() } else { b.iter().rev().cloned().collect() };
                    a.push(jp);
                    a.extend(b_ordered);
                    // 用合并后的替换两条原链(先占位空, 尾部统一清理)
                    chains[ai] = a;
                    chains[bi] = Vec::new();
                    merged = true;
                }
            }
        }
        if !merged {
            break;
        }
    }
    chains.retain(|c| !c.is_empty() && c.len() >= 4);
}

fn chain_neighbors(
    c: (i64, i64),
    prev: Option<(i64, i64)>,
    visited: &[bool],
    ink: &[bool],
    pw: i64,
    ph: i64,
    dx: &[i64; 8],
    dy: &[i64; 8],
) -> Vec<(i64, i64)> {
    let idx = |x: i64, y: i64| -> usize { (y as usize) * pw as usize + x as usize };
    let on = |x: i64, y: i64| -> bool {
        x >= 0 && y >= 0 && x < pw && y < ph && ink[idx(x, y)]
    };
    let mut nbs: Vec<(i64, i64)> = (0..8)
        .filter(|&d| on(c.0 + dx[d], c.1 + dy[d]))
        .map(|d| (c.0 + dx[d], c.1 + dy[d]))
        .collect();
    nbs.retain(|&n| Some(n) != prev && !visited[idx(n.0, n.1)]);
    nbs
}

/// 从起点沿 8-连通笔画双向行走, 优先走"最直"的延续方向, 直到端点或闭环。
fn walk_chain(
    ink: &[bool],
    visited: &mut [bool],
    pw: i64,
    ph: i64,
    start: (i64, i64),
    dx: &[i64; 8],
    dy: &[i64; 8],
) -> Vec<(i64, i64)> {
    let idx = |x: i64, y: i64| -> usize { (y as usize) * pw as usize + x as usize };
    let straightest = |c: (i64, i64), nbs: &[(i64, i64)], dir_in: (i64, i64)| -> (i64, i64) {
        nbs.iter()
            .cloned()
            .min_by(|&a, &b| {
                let ta = turn_cost(dir_in, (a.0 - c.0, a.1 - c.1));
                let tb = turn_cost(dir_in, (b.0 - c.0, b.1 - c.1));
                ta.partial_cmp(&tb).unwrap_or(std::cmp::Ordering::Equal)
            })
            .unwrap()
    };

    // 正向: 起点 -> 尽头(或闭环回来)
    let mut fwd: Vec<(i64, i64)> = Vec::new();
    let mut c = start;
    let mut prev: Option<(i64, i64)> = None;
    let mut closed = false;
    loop {
        fwd.push(c);
        visited[idx(c.0, c.1)] = true;
        let dir_in = match prev {
            Some(p) => (c.0 - p.0, c.1 - p.1),
            None => (0, 0),
        };
        let mut nbs = chain_neighbors(c, prev, visited, ink, pw, ph, dx, dy);
        if nbs.is_empty() {
            break;
        }
        let next = if prev.is_none() {
            nbs[0]
        } else {
            straightest(c, &nbs, dir_in)
        };
        prev = Some(c);
        c = next;
        if c == start && fwd.len() > 1 {
            closed = true;
            break;
        }
    }
    if closed {
        return fwd;
    }
    // 反向: 起点 -> 另一端
    let mut bwd: Vec<(i64, i64)> = Vec::new();
    let mut c = start;
    let mut prev: Option<(i64, i64)> = None;
    loop {
        bwd.push(c);
        visited[idx(c.0, c.1)] = true;
        let dir_in = match prev {
            Some(p) => (c.0 - p.0, c.1 - p.1),
            None => (0, 0),
        };
        let mut nbs = chain_neighbors(c, prev, visited, ink, pw, ph, dx, dy);
        if nbs.is_empty() {
            break;
        }
        let next = if prev.is_none() {
            nbs[0]
        } else {
            straightest(c, &nbs, dir_in)
        };
        prev = Some(c);
        c = next;
    }
    bwd.reverse();
    let mut chain = bwd;
    chain.pop(); // 去掉重复的 start(在 fwd[0])
    chain.extend(fwd);
    chain
}

#[inline]
fn turn_cost(dir_in: (i64, i64), dir_out: (i64, i64)) -> f64 {
    if dir_in == (0, 0) {
        return 0.0;
    }
    let li = ((dir_in.0 * dir_in.0 + dir_in.1 * dir_in.1) as f64).sqrt();
    let lo = ((dir_out.0 * dir_out.0 + dir_out.1 * dir_out.1) as f64).sqrt();
    let dot = (dir_in.0 * dir_out.0 + dir_in.1 * dir_out.1) as f64 / (li * lo);
    -dot
}

/// 过滤笔画链: 按长度降序, 丢弃过短链, 去重(近似的长度+质心), 限制条数。
pub fn filter_chains(mut chains: Vec<Vec<P2>>, f: ChainFilter) -> Vec<Vec<P2>> {
    let per = |c: &[P2]| -> f64 {
        c.windows(2)
            .map(|w| ((w[1].x - w[0].x).powi(2) + (w[1].y - w[0].y).powi(2)).sqrt())
            .sum::<f64>()
    };
    chains.sort_by(|a, b| per(b).partial_cmp(&per(a)).unwrap_or(std::cmp::Ordering::Equal));
    let mut kept: Vec<Vec<P2>> = Vec::new();
    for c in chains {
        if per(&c) < f.min_len {
            continue;
        }
        let cl = per(&c);
        let (cx, cy) = centroid(&c);
        let mut dup = false;
        for k in &kept {
            let kl = per(k);
            if (kl - cl).abs() / kl.max(1.0) < 0.04 {
                let (kx, ky) = centroid(k);
                if (kx - cx).hypot(ky - cy) < 12.0 {
                    dup = true;
                    break;
                }
            }
        }
        if !dup {
            kept.push(c);
        }
        if kept.len() >= f.max_chains {
            break;
        }
    }
    kept
}

/// Sort loops by perimeter (desc), drop tiny/degenerate ones, cap the count,
/// and deduplicate near-identical loops (the same digital boundary ring can
/// be split into several overlapping loops at self-touching corner pixels).
pub fn filter_contours(mut loops: Vec<Vec<P2>>, f: ContourFilter) -> Vec<Vec<P2>> {
    loops.sort_by(|a, b| {
        arc_length(b)
            .partial_cmp(&arc_length(a))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let mut kept: Vec<Vec<P2>> = Vec::new();
    for l in loops {
        if l.len() < 8 || signed_area(&l).abs() < f.min_area {
            continue;
        }
        // Duplicate? Compare perimeter and centroid against already-kept loops.
        let per = arc_length(&l);
        let (cx, cy) = centroid(&l);
        let mut dup = false;
        for k in &kept {
            let kper = arc_length(k);
            if (kper - per).abs() / kper.max(1.0) < 0.04 {
                let (kx, ky) = centroid(k);
                if (kx - cx).hypot(ky - cy) < 12.0 {
                    dup = true;
                    break;
                }
            }
        }
        if !dup {
            kept.push(l);
        }
        if kept.len() >= f.max_contours {
            break;
        }
    }
    kept
}

fn centroid(l: &[P2]) -> (f64, f64) {
    let n = l.len().max(1) as f64;
    let (mut x, mut y) = (0.0, 0.0);
    for p in l {
        x += p.x;
        y += p.y;
    }
    (x / n, y / n)
}

/// Chaikin 角切平滑（开放折线，不闭合）：迭代把每个线段两端各按 1/4、3/4
/// 切角，去除像素级锯齿，让 DFT 还原的线条更平滑干净。
pub fn chaikin_smooth(pts: &[P2], iterations: usize) -> Vec<P2> {
    let mut cur = pts.to_vec();
    for _ in 0..iterations {
        if cur.len() < 3 {
            break;
        }
        let n = cur.len();
        let mut next = Vec::with_capacity(n * 2);
        for i in 0..n - 1 {
            let p0 = cur[i];
            let p1 = cur[i + 1];
            next.push(P2::new(0.75 * p0.x + 0.25 * p1.x, 0.75 * p0.y + 0.25 * p1.y));
            next.push(P2::new(0.25 * p0.x + 0.75 * p1.x, 0.25 * p0.y + 0.75 * p1.y));
        }
        next.push(*cur.last().unwrap());
        cur = next;
    }
    cur
}

/// Resample a closed loop to exactly `n` points uniformly spaced by arc
/// length (linear interpolation along the polygon edges). This is essential
/// for a clean DFT: raw pixel sampling has uneven density.
pub fn resample_loop(loop_: &[P2], n: usize) -> Vec<P2> {
    let m = loop_.len();
    if m == 0 {
        return Vec::new();
    }
    if m == 1 {
        return vec![loop_[0]; n];
    }
    let mut seg = Vec::with_capacity(m);
    let mut cum = Vec::with_capacity(m + 1);
    cum.push(0.0);
    for i in 0..m {
        let j = (i + 1) % m;
        let d = ((loop_[j].x - loop_[i].x).powi(2) + (loop_[j].y - loop_[i].y).powi(2)).sqrt();
        seg.push(d);
        cum.push(cum[i] + d);
    }
    let total = cum[m];
    if total <= f64::EPSILON {
        return vec![loop_[0]; n];
    }
    let step = total / n as f64;
    let mut out = Vec::with_capacity(n);
    for k in 0..n {
        let t = k as f64 * step;
        // Upper bound: first index with cum > t; segment = ub-1.
        let ub = cum.partition_point(|&c| c <= t);
        let i = ub.saturating_sub(1).min(m - 1);
        let s = t - cum[i];
        let l = if seg[i] > 1e-12 { seg[i] } else { 1e-12 };
        let frac = (s / l).clamp(0.0, 1.0);
        let a = loop_[i];
        let b = loop_[(i + 1) % m];
        out.push(P2::new(a.x + (b.x - a.x) * frac, a.y + (b.y - a.y) * frac));
    }
    out
}