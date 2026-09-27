//! ftrace CLI — 图片 → 黑白线稿 → 傅里叶(本轮)系数与动画
//!
//! 核心管线在 lib.rs（`ftrace::transform`），本文件只负责参数解析与结果打印。

use std::error::Error;
use std::path::PathBuf;

use clap::{Parser, ValueEnum};

#[derive(Parser)]
#[command(name = "ftrace", version, about = "图片 → 黑白线稿 → 傅里叶(本轮)系数与动画")]
struct Cli {
    /// 输入图片 (PNG/JPG/GIF/WebP/BMP/TIFF 等)；使用 --template 时可省略
    #[arg(value_name = "INPUT")]
    input: Option<PathBuf>,
    /// 输出文件前缀 (默认取输入文件名)
    #[arg(short, long, value_name = "STEM")]
    output: Option<PathBuf>,
    /// 线稿模式: auto=有 alpha 走 mask 否则 edge / edge=Canny / mask=掩膜轮廓
    #[arg(long, value_enum, default_value_t = ModeArg::Auto)]
    mode: ModeArg,
    /// 高斯模糊 σ (edge 模式, 越大线条越粗越连贯)
    #[arg(long, default_value_t = 1.2)]
    blur: f32,
    /// Canny 低阈值 (占最大梯度幅值比例)
    #[arg(long, default_value_t = 0.08)]
    canny_low: f32,
    /// Canny 高阈值 (占最大梯度幅值比例)
    #[arg(long, default_value_t = 0.22)]
    canny_high: f32,
    /// 每个轮廓弧长均匀采样点数 (建议 2 的幂)
    #[arg(long, default_value_t = 1024)]
    samples: usize,
    /// 每个轮廓保留的最大谐波数
    #[arg(long, default_value_t = 256)]
    harmonics: usize,
    /// 最多保留的轮廓/笔画链数
    #[arg(long, default_value_t = 12)]
    max_contours: usize,
    /// 轮廓最小面积/笔画最小长度 (px, 滤噪点)
    #[arg(long, default_value_t = 30.0)]
    min_area: f64,
    /// 动画帧数
    #[arg(long, default_value_t = 96)]
    frames: u32,
    /// mask 模式反转前景/背景极性 (浅色主体深色背景)
    #[arg(long)]
    invert: bool,
    /// 不生成 GIF 动画
    #[arg(long)]
    no_gif: bool,
    /// 渲染画布最长边上限 (px)
    #[arg(long, default_value_t = 1600)]
    max_dim: u32,
    /// 线稿处理阶段的最长边上限 (px): 超大的图先降采样再提取, 控制内存与耗时
    #[arg(long, default_value_t = 2048)]
    process_dim: u32,
    /// 动画/SVG 不绘制本轮圆盘(只画轨迹线)
    #[arg(long)]
    no_circles: bool,
    /// 预览图不叠加原始线稿底纹
    #[arg(long)]
    no_underlay: bool,
    /// 关闭彩色输出（默认开：线稿/还原线条按原图区域主色染色）
    #[arg(long)]
    no_color: bool,
    // ---- 播放参数（写进 coeffs.json 的 playback 块，供 Eldoria 法阵渲染）----
    /// 逐笔画起始时间错开：第 i 条 = i × N tick（0=同时开始）
    #[arg(long, default_value_t = 0)]
    start_stagger: u32,
    /// 显式逐笔画起始时间列表，逗号分隔（如 "0,5,10"；优先于 --start-stagger）
    #[arg(long, value_name = "LIST")]
    starts: Option<String>,
    /// 一条笔画画完一圈所需 tick
    #[arg(long, default_value_t = 20)]
    ticks_per_cycle: u32,
    /// 图形最长边映射格数（直径，Minecraft 方块）
    #[arg(long, default_value_t = 8.0)]
    scale: f64,
    /// 绑定目标：player(跟随玩家, 默认) / origin(施法点) / world(绝对坐标)
    #[arg(long, default_value = "player")]
    bind: String,
    /// 开启高度轴（可看作时间轴：笔画随绘制进度在 Y 轴上升）
    #[arg(long)]
    height_axis: bool,
    /// 高度轴每周期上升格数
    #[arg(long, default_value_t = 1.0)]
    height_per_cycle: f64,
    /// 循环绘制
    #[arg(long)]
    loop_play: bool,
    /// 图形中心 X 平移（格，相对绑定锚点；法阵可画在玩家面前/上方）
    #[arg(long, default_value_t = 0.0)]
    offset_x: f64,
    /// 图形中心 Y 平移（格）
    #[arg(long, default_value_t = 0.0)]
    offset_y: f64,
    /// 图形中心 Z 平移（格）
    #[arg(long, default_value_t = 0.0)]
    offset_z: f64,
    /// 绕 Y 轴旋转（度，顺时针）
    #[arg(long, default_value_t = 0.0)]
    rotation_deg: f64,
    /// 线导出模式：每条线分配调色板预留色 + 生成线索引图(区分色+序号)与合成预览图
    #[arg(long)]
    line_sheet: bool,
    /// 自定义逐线名称列表，逗号分隔（如 "outer,inner,star"；默认 L0,L1,...）
    #[arg(long, value_name = "LIST")]
    line_names: Option<String>,
    /// 法阵模板：不读取图片，参数化几何直接生成法阵线条
    /// (hexagram 六芒星 / starfield 星辰阵 / rune_ring 符文环 / element 元素徽记)
    #[arg(long, value_name = "NAME")]
    template: Option<String>,
}

#[derive(Clone, Copy, PartialEq, Eq, ValueEnum)]
enum ModeArg {
    Auto,
    Edge,
    Line,
    Mask,
}

impl From<ModeArg> for ftrace::lineart::LineArtMode {
    fn from(m: ModeArg) -> Self {
        match m {
            ModeArg::Auto => ftrace::lineart::LineArtMode::Auto,
            ModeArg::Edge => ftrace::lineart::LineArtMode::Edge,
            ModeArg::Line => ftrace::lineart::LineArtMode::Line,
            ModeArg::Mask => ftrace::lineart::LineArtMode::Mask,
        }
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    let cli = Cli::parse();
    if cli.input.is_none() && cli.template.is_none() {
        return Err("缺少输入：请提供 <INPUT> 图片，或使用 --template <法阵模板>。".into());
    }
    if cli.input.is_some() && cli.template.is_some() {
        return Err("输入冲突：<INPUT> 图片与 --template 只能二选一。".into());
    }
    // 模板模式无真实输入文件；用伪路径承载元信息（transform 不会读取它）
    let input = cli
        .input
        .clone()
        .unwrap_or_else(|| PathBuf::from(format!("template://{}", cli.template.as_deref().unwrap_or("unknown"))));
    let opts = ftrace::Options {
        mode: cli.mode.into(),
        blur: cli.blur,
        canny_low: cli.canny_low,
        canny_high: cli.canny_high,
        samples: cli.samples,
        harmonics: cli.harmonics,
        max_contours: cli.max_contours,
        min_area: cli.min_area,
        frames: cli.frames,
        invert: cli.invert,
        no_gif: cli.no_gif,
        max_dim: cli.max_dim,
        process_dim: cli.process_dim,
        no_circles: cli.no_circles,
        no_underlay: cli.no_underlay,
        no_color: cli.no_color,
        start_stagger: cli.start_stagger,
        starts: cli
            .starts
            .as_deref()
            .map(|s| {
                s.split(',')
                    .filter_map(|x| x.trim().parse::<u32>().ok())
                    .collect::<Vec<u32>>()
            })
            .unwrap_or_default(),
        ticks_per_cycle: cli.ticks_per_cycle,
        scale: cli.scale,
        bind: cli.bind.clone(),
        height_axis: cli.height_axis,
        height_per_cycle: cli.height_per_cycle,
        loop_play: cli.loop_play,
        offset_x: cli.offset_x,
        offset_y: cli.offset_y,
        offset_z: cli.offset_z,
        rotation_deg: cli.rotation_deg,
        line_sheet: cli.line_sheet,
        line_names: cli
            .line_names
            .as_deref()
            .map(|s| {
                s.split(',')
                    .map(|x| x.trim().to_string())
                    .filter(|x| !x.is_empty())
                    .collect::<Vec<String>>()
            })
            .unwrap_or_default(),
        template: cli.template.clone(),
    };
    let out = ftrace::transform(&input, cli.output.as_deref(), &opts)?;

    println!("✅ {} ({}×{})", out.input.display(), out.width, out.height);
    println!("   模式: {}  | 采样 {} 点/轮廓  | 谐波 ≤{}", out.mode, out.samples, out.harmonics);
    println!("   线稿: {}", out.lineart_path.display());
    if let Some(c) = &out.lineart_color_path {
        println!("   彩色线稿: {}", c.display());
    }
    println!("   轮廓数: {}", out.report.contours.len());
    for tr in &out.report.contours {
        println!(
            "   #{} 周长 {:.0}px 面积 {:.0}px² 采样 {} 谐波 {} | top3: {}",
            tr.id,
            tr.perimeter_px,
            tr.area_px,
            tr.n_samples,
            tr.n_harmonics,
            top3(&tr.harmonics)
        );
    }
    println!("   系数: {}", out.json_path.display());
    if let Some(p) = &out.lines_path {
        println!("   线索引图: {}", p.display());
        if let Some(pp) = &out.lines_preview_path {
            println!("   合成预览: {}", pp.display());
        }
    }
    println!("   SVG : {}", out.svg_path.display());
    println!("   预览: {}", out.preview_path.display());
    if let Some(g) = &out.gif_path {
        println!("   动画: {}", g.display());
    }
    Ok(())
}

fn top3(hs: &[ftrace::fourier::Harmonic]) -> String {
    hs.iter()
        .take(3)
        .map(|h| format!("k={} |c|={:.1}", h.k, h.mag()))
        .collect::<Vec<_>>()
        .join(", ")
}