//! ftrace 核心库：图片 → 黑白线稿 → 傅里叶（本轮/离心轮）变换系数与动画。
//!
//! 本库同时被两个二进制使用：
//!   - `ftrace`（CLI，src/main.rs）
//!   - `ftrace-web`（本地 Web 服务，src/bin/ftrace-web.rs）
//!
//! 入口：`Options` + [`transform`]，一次性产出线稿 PNG / 系数 JSON / 本轮 SVG /
//! 预览 PNG / 绘制动画 GIF，并返回各产物路径与逐轮廓统计。

pub mod canny;
pub mod color;
pub mod contours;
pub mod fourier;
pub mod geom;
pub mod gifout;
pub mod jsonout;
pub mod lineart;
pub mod lines;
pub mod render;
pub mod svgout;
pub mod templates;

use std::path::{Path, PathBuf};

use image::GenericImageView;
use rayon::prelude::*;
use render::RenderCfg;

/// 处理参数（CLI 与服务端共用；字段语义见 CLI --help）。
#[derive(Clone, Debug)]
pub struct Options {
    pub mode: lineart::LineArtMode,
    pub blur: f32,
    pub canny_low: f32,
    pub canny_high: f32,
    pub samples: usize,
    pub harmonics: usize,
    pub max_contours: usize,
    pub min_area: f64,
    pub frames: u32,
    pub invert: bool,
    pub no_gif: bool,
    pub max_dim: u32,
    pub process_dim: u32,
    pub no_circles: bool,
    pub no_underlay: bool,
    /// 关闭彩色输出（默认开启：线稿线条按原图区域主色染色）。
    pub no_color: bool,
    // ---- 播放参数（导出到 coeffs.json 的 playback 块，供 Eldoria 等下游渲染）----
    /// 逐轮廓起始时间错开：第 i 条笔画 start = i * stagger（tick）
    pub start_stagger: u32,
    /// 显式逐轮廓起始时间列表（优先于 stagger；不足的用最后一项补齐）
    pub starts: Vec<u32>,
    /// 一条笔画画完一圈需要的 tick
    pub ticks_per_cycle: u32,
    /// 图形最长边映射到多少格（直径）
    pub scale: f64,
    /// 绑定目标：player / origin / world
    pub bind: String,
    /// 高度轴=时间轴
    pub height_axis: bool,
    /// 高度轴每周期上升格数
    pub height_per_cycle: f64,
    /// 循环绘制
    pub loop_play: bool,
    /// 图形中心 X 平移（格，相对绑定锚点）
    pub offset_x: f64,
    /// 图形中心 Y 平移（格）
    pub offset_y: f64,
    /// 图形中心 Z 平移（格）
    pub offset_z: f64,
    /// 绕 Y 轴旋转（度，顺时针）
    pub rotation_deg: f64,
    /// 线导出模式：每条线分配调色板预留色 + 生成线索引图(区分色+序号)与合成预览图
    pub line_sheet: bool,
    /// 自定义逐线名称列表（覆盖默认 L{i}；长度不足用默认补齐）
    pub line_names: Vec<String>,
    /// 法阵模板（--template）：参数化几何直接生成线条，无需输入图片。
    /// 取值 hexagram(六芒星)/starfield(星辰阵)/rune_ring(符文环)/element(元素徽记)。
    pub template: Option<String>,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            mode: lineart::LineArtMode::Auto,
            blur: 1.2,
            canny_low: 0.08,
            canny_high: 0.22,
            samples: 1024,
            harmonics: 256,
            max_contours: 12,
            min_area: 30.0,
            frames: 96,
            invert: false,
            no_gif: false,
            max_dim: 1600,
            process_dim: 2048,
            no_circles: false,
            no_underlay: false,
            no_color: false,
            start_stagger: 0,
            starts: Vec::new(),
            ticks_per_cycle: 20,
            scale: 8.0,
            bind: "player".to_string(),
            height_axis: false,
            height_per_cycle: 1.0,
            loop_play: false,
            offset_x: 0.0,
            offset_y: 0.0,
            offset_z: 0.0,
            rotation_deg: 0.0,
            line_sheet: false,
            line_names: Vec::new(),
            template: None,
        }
    }
}

/// 一次变换的全部产物路径与统计信息。
#[derive(Clone, Debug)]
pub struct TransformOutput {
    pub input: PathBuf,
    /// 输出前缀（各产物 = STEM + 后缀）。
    pub stem: PathBuf,
    pub width: u32,
    pub height: u32,
    pub mode: String,
    pub samples: usize,
    pub harmonics: usize,
    pub lineart_path: PathBuf,
    pub lineart_color_path: Option<PathBuf>,
    pub json_path: PathBuf,
    pub svg_path: PathBuf,
    pub preview_path: PathBuf,
    pub gif_path: Option<PathBuf>,
    /// 线导出模式：线索引图（预留色 + 序号标注）。
    pub lines_path: Option<PathBuf>,
    /// 线导出模式：合成预览图（预留色，无标注）。
    pub lines_preview_path: Option<PathBuf>,
    pub report: jsonout::RunReport,
}

/// 对一张图片执行完整管线并写出全部产物。
///
/// `stem` 为输出前缀（None 时取输入文件名去扩展名）；所有产物写入 `stem` 所在目录。
pub fn transform(input: &Path, stem: Option<&Path>, opts: &Options) -> Result<TransformOutput, String> {
    let mut t0 = std::time::Instant::now();
    let mut tick = |label: &str| {
        if std::env::var("FT_TIMING").is_ok() {
            eprintln!("[ftrace-time] {:<22} {:>8.2}s", label, t0.elapsed().as_secs_f64());
            t0 = std::time::Instant::now();
        }
    };

    // ---- 源解析：法阵模板（--template 参数化几何）或图片管线 ----
    let mut art_opt: Option<image::GrayImage> = None;
    let mut img_rgb: Option<image::RgbImage> = None;
    let mut template_names: Vec<String> = Vec::new();
    let mut alpha_mode = false;
    let (paths, is_chain, w, h) = if let Some(tpl) = opts.template.as_deref() {
        let t = templates::generate(tpl).ok_or_else(|| {
            format!("未知法阵模板: {tpl}（可选: {}）", templates::all_names().join("/"))
        })?;
        template_names = t.path_names.clone();
        tick("template generate");
        (t.paths, false, t.width, t.height)
    } else {
        let mut img = image::open(input).map_err(|e| format!("无法打开图片 {}: {e}", input.display()))?;
        // 超大图先降采样到 process_dim，控制内存与耗时（坐标随之缩放）
        let (ow, oh) = img.dimensions();
        let longest = ow.max(oh);
        if opts.process_dim > 0 && longest > opts.process_dim {
            let nw = ((ow as f64 * opts.process_dim as f64 / longest as f64).round() as u32).max(1);
            let nh = ((oh as f64 * opts.process_dim as f64 / longest as f64).round() as u32).max(1);
            let resized = image::imageops::resize(&img, nw, nh, image::imageops::FilterType::Lanczos3);
            img = image::DynamicImage::ImageRgba8(resized);
            eprintln!("[ftrace] 输入 {}×{} 过大, 已降采样到 {}×{} 再处理", ow, oh, nw, nh);
        }
        let (w, h) = img.dimensions();
        if w == 0 || h == 0 {
            return Err("图片尺寸异常".into());
        }

        let lcfg = lineart::LineArtCfg {
            mode: opts.mode,
            blur: opts.blur,
            canny_low: opts.canny_low,
            canny_high: opts.canny_high,
            invert: opts.invert,
        };
        let artres = lineart::to_lineart(&img, &lcfg);
        art_opt = Some(artres.art.clone());
        tick("open+downscale+lineart");
        let ink = artres.art.pixels().filter(|p| p.0[0] >= 128).count();
        if ink == 0 {
            return Err("线稿为空: 没有提取到任何线条".into());
        }

        // edge 模式: 细笔画 -> 笔画链追踪; mask 模式: 填充掩膜 -> 外轮廓环追踪
        let (mut paths, is_chain): (Vec<Vec<geom::P2>>, bool) = if artres.is_edge {
            (contours::trace_stroke_chains(&artres.trace_input), true)
        } else {
            (contours::trace_contours(&artres.trace_input), false)
        };
        tick(if is_chain { "trace chains" } else { "trace contours" });
        let n_raw = paths.len();
        if is_chain {
            paths = contours::filter_chains(
                paths,
                contours::ChainFilter {
                    min_len: opts.min_area.max(8.0),
                    max_chains: opts.max_contours,
                },
            );
        } else {
            paths = contours::filter_contours(
                paths,
                contours::ContourFilter {
                    min_area: opts.min_area,
                    max_contours: opts.max_contours,
                },
            );
        }
        tick(&format!("filter (raw={n_raw})"));
        if paths.is_empty() {
            return Err("未找到足够大的有效轮廓/笔画, 试试 --mode edge/mask 或调低 --min-area".into());
        }

        img_rgb = Some(img.to_rgb8());
        alpha_mode = lineart::has_alpha(&img) && matches!(opts.mode, lineart::LineArtMode::Auto);
        (paths, is_chain, w, h)
    };

    let samples = opts.samples.max(16);
    let n_h = opts.harmonics.min(samples);
    // 原图 RGB 缓冲（用于每条笔画的主色分类取色；模板模式无图，用调色板预留色）
    let mut trs: Vec<fourier::ContourTransform> = paths
        .iter()
        .enumerate()
        .map(|(i, lp)| {
            // 链模式: 先 Chaikin 平滑去除像素级锯齿, 再用"去程+回程"闭合
            // 开放笔画(避免横穿画面的直线跳段); 轮廓模式: 本身闭合
            let pts = if is_chain {
                let sm = contours::chaikin_smooth(lp, 2);
                let mut closed: Vec<geom::P2> = sm.clone();
                closed.extend(sm.iter().rev());
                contours::resample_loop(&closed, samples)
            } else {
                contours::resample_loop(lp, samples)
            };
            let (col, col_name) = if let Some(rgb) = &img_rgb {
                color::dominant_color(lp, rgb, !is_chain)
            } else {
                (lines::palette(i), format!("palette{i}"))
            };
            let mut tr = fourier::forward_fft(&pts, n_h, i, lp.clone(), col, col_name);
            // 逐笔画起始时间：显式列表优先，其次按 stagger 递增
            tr.start = if !opts.starts.is_empty() {
                let idx = i.min(opts.starts.len() - 1);
                opts.starts[idx]
            } else {
                i as u32 * opts.start_stagger
            };
            tr
        })
        .collect();
    // 线导出模式 / 模板模式：分配调色板预留色 + 简化名称 + 语义参考名（供 .efx 按名引用）
    if opts.line_sheet || opts.template.is_some() {
        for (i, tr) in trs.iter_mut().enumerate() {
            tr.color = lines::palette(i);
            tr.color_name = format!("palette{i}");
            tr.name = opts
                .line_names
                .get(i)
                .cloned()
                .or_else(|| template_names.get(i).cloned())
                .unwrap_or_else(|| format!("L{i}"));
            tr.hint = lines::hint_of(tr, i);
        }
    }
    tick("resample+fft");

    let stem: PathBuf = match stem {
        Some(s) => s.to_path_buf(),
        None => {
            let mut p = input.to_path_buf();
            p.set_extension("");
            p
        }
    };

    let rcfg = RenderCfg::from_dims(w, h, opts.frames, Some(opts.max_dim), !opts.no_circles, !opts.no_color);

    // 1. 线稿 PNG（白纸黑线；模板模式无源图，直接画黑线白底）
    let lineart_path = with_ext(&stem, "lineart.png");
    if let Some(art) = &art_opt {
        image::save_buffer(
            &lineart_path,
            lineart::display(art).as_raw(),
            w,
            h,
            image::ColorType::L8,
        )
        .map_err(|e| e.to_string())?;
    } else {
        templates::draw_black_lineart(&paths)
            .save(&lineart_path)
            .map_err(|e| e.to_string())?;
    }

    // 1+. 彩色线稿 PNG（线条自带原图区域主色，供函数取色/法阵绘制）
    //     --no-color 时不生成（彩色输出彻底关闭）
    let lineart_color_path = if opts.no_color {
        None
    } else {
        let p = with_ext(&stem, "lineart_color.png");
        let mut lc = render::new_canvas(&rcfg);
        render::draw_colored_lineart(&mut lc, &rcfg, &trs);
        lc.save(&p).map_err(|e| e.to_string())?;
        Some(p)
    };

    // 2. 系数 JSON
    let resolved_mode = if let Some(tpl) = &opts.template {
        format!("template({tpl})")
    } else if alpha_mode {
        "mask(alpha)".to_string()
    } else {
        match opts.mode {
            lineart::LineArtMode::Auto => "auto".into(),
            lineart::LineArtMode::Edge => "edge".into(),
            lineart::LineArtMode::Line => "line".into(),
            lineart::LineArtMode::Mask => "mask".into(),
        }
    };
    let report = jsonout::RunReport {
        format_version: 2,
        tool: "ftrace".into(),
        source: input.display().to_string(),
        width: w,
        height: h,
        params: jsonout::ParamSnapshot {
            mode: resolved_mode.clone(),
            blur_sigma: opts.blur,
            canny_low: opts.canny_low,
            canny_high: opts.canny_high,
            samples,
            harmonics: n_h,
            max_contours: opts.max_contours,
            min_area_px: opts.min_area,
            frames: opts.frames,
            invert: opts.invert,
        },
        playback: jsonout::PlaybackSnapshot {
            bind: opts.bind.clone(),
            scale: opts.scale,
            ticks_per_cycle: opts.ticks_per_cycle.max(1),
            height_axis: opts.height_axis,
            height_per_cycle: opts.height_per_cycle,
            loop_play: opts.loop_play,
            offset_x: opts.offset_x,
            offset_y: opts.offset_y,
            offset_z: opts.offset_z,
            rotation_deg: opts.rotation_deg,
        },
        contour_count: trs.len(),
        contours: trs.clone(),
    };
    let json_path = with_ext(&stem, "coeffs.json");
    jsonout::write_json(&json_path, &report).map_err(|e| e.to_string())?;

    // 3. SVG（本轮圆盘 + 还原轨迹）
    let svg = svgout::to_svg(&rcfg, &trs, &input.display().to_string());
    let svg_path = with_ext(&stem, "epicycles.svg");
    std::fs::write(&svg_path, svg).map_err(|e| e.to_string())?;
    tick("json+svg");

    // 4. 预览 PNG: 先铺线稿底纹, 再叠红色还原轨迹 (轨迹在上层, 不被底纹覆盖)
    let mut preview = render::new_canvas(&rcfg);
    if let Some(art) = &art_opt {
        if !opts.no_underlay {
            render::with_underlay(&mut preview, art, &rcfg);
        }
    }
    render::draw_reconstructions(&mut preview, &rcfg, &trs);
    let preview_path = with_ext(&stem, "preview.png");
    preview.save(&preview_path).map_err(|e| e.to_string())?;
    tick("preview");

    // 5. 动画 GIF（帧并行渲染）
    let gif_path = with_ext(&stem, "draw.gif");
    let gif_path = if opts.no_gif {
        None
    } else {
        let nf = (opts.frames.max(2) - 1) as f64;
        let frame_count = opts.frames.max(2);
        let frames: Vec<image::RgbImage> = (0..frame_count)
            .into_par_iter()
            .map(|f| render::render_frame(&rcfg, &trs, f as f64 / nf))
            .collect();
        gifout::write_gif(&gif_path, &frames, 5).map_err(|e| e.to_string())?;
        tick("gif");
        Some(gif_path)
    };

    // 6. 线导出模式：线索引图（预留色+序号） + 合成预览图（预留色）
    let (lines_path, lines_preview_path) = if opts.line_sheet {
        let p1 = with_ext(&stem, "lines.png");
        lines::draw_line_sheet(&rcfg, &trs, true)
            .save(&p1)
            .map_err(|e| e.to_string())?;
        let p2 = with_ext(&stem, "lines.preview.png");
        lines::draw_line_sheet(&rcfg, &trs, false)
            .save(&p2)
            .map_err(|e| e.to_string())?;
        tick("lines");
        (Some(p1), Some(p2))
    } else {
        (None, None)
    };

    Ok(TransformOutput {
        input: input.to_path_buf(),
        stem,
        width: w,
        height: h,
        mode: resolved_mode,
        samples,
        harmonics: n_h,
        lineart_path,
        lineart_color_path,
        json_path,
        svg_path,
        preview_path,
        gif_path,
        lines_path,
        lines_preview_path,
        report,
    })
}

fn with_ext(stem: &Path, ext: &str) -> PathBuf {
    let mut s = stem.as_os_str().to_os_string();
    s.push(".");
    s.push(ext);
    PathBuf::from(s)
}