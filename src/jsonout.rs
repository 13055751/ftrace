//! JSON export of the Fourier coefficients (serde_json, MIT/Apache-2.0).
//!
//! Coefficient JSON layout follows the spirit of fourier-svg-rs and
//! fluffy-eureka exports: per-contour signed-frequency harmonic lists, plus
//! metadata (centroid / bbox / area / perimeter / sample count) so a
//! downstream renderer can reproduce the animation exactly.

use std::fs::File;
use std::io::BufWriter;
use std::path::Path;

use serde::Serialize;

use crate::fourier::ContourTransform;

#[derive(Serialize, Clone, Debug)]
pub struct ParamSnapshot {
    pub mode: String,
    pub blur_sigma: f32,
    pub canny_low: f32,
    pub canny_high: f32,
    pub samples: usize,
    pub harmonics: usize,
    pub max_contours: usize,
    pub min_area_px: f64,
    pub frames: u32,
    pub invert: bool,
}

/// 播放参数块（供 Minecraft 插件等下游渲染器直接消费；Eldoria 法阵用）。
#[derive(Serialize, Clone, Debug)]
pub struct PlaybackSnapshot {
    /// 绑定目标：player(默认，跟随玩家) / origin(施法点) / world(绝对坐标)
    pub bind: String,
    /// 图形最长边映射到多少格（直径，单位：Minecraft 方块）
    pub scale: f64,
    /// 一条笔画画完一圈需要多少 tick
    pub ticks_per_cycle: u32,
    /// 高度轴（可看作时间轴）：开启后笔画随绘制进度在 Y 轴上升
    pub height_axis: bool,
    /// 开启高度轴时，一个周期上升多少格
    pub height_per_cycle: f64,
    /// 是否循环（每周期重复绘制）
    pub loop_play: bool,
    /// 图形中心 X 平移（格，相对绑定锚点；法阵可画在玩家面前/上方）
    pub offset_x: f64,
    /// 图形中心 Y 平移（格）
    pub offset_y: f64,
    /// 图形中心 Z 平移（格）
    pub offset_z: f64,
    /// 绕 Y 轴旋转（度，顺时针）
    pub rotation_deg: f64,
}

#[derive(Serialize, Clone, Debug)]
pub struct RunReport {
    pub format_version: u32,
    pub tool: String,
    pub source: String,
    pub width: u32,
    pub height: u32,
    pub params: ParamSnapshot,
    /// 播放参数（v2 新增；下游可直接读取，无需再解析脚本）
    pub playback: PlaybackSnapshot,
    pub contour_count: usize,
    pub contours: Vec<ContourTransform>,
}

pub fn write_json(path: &Path, report: &RunReport) -> std::io::Result<()> {
    let file = File::create(path)?;
    let writer = BufWriter::new(file);
    serde_json::to_writer_pretty(writer, report)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
    Ok(())
}