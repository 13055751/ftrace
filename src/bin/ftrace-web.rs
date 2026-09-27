//! ftrace-web —— ftrace 库的本地 Web 服务端。
//!
//! 端点：
//!   - `GET  /`                返回前端单页（assets/index.html 内嵌编译）
//!   - `POST /api/transform`   接收 multipart（image 必填 + 若干可选参数），
//!                             调用 `ftrace::transform` 后返回 JSON 结果
//!   - `GET  /files/<path...>` 读取 work/ 目录内的产物文件（防目录穿越）

use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};

use axum::extract::{DefaultBodyLimit, Multipart, Path as AxPath, State};
use axum::http::{header, StatusCode};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Serialize;

use ftrace::lineart::LineArtMode;
use ftrace::Options;

// ---------------------------------------------------------------------------
// 响应类型
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct HarmonicRow {
    k: i32,
    real: f64,
    imag: f64,
    mag: f64,
    /// 相位（弧度）
    phase: f64,
}

#[derive(Serialize)]
struct ContourRow {
    id: usize,
    perimeter_px: f64,
    area_px: f64,
    n_samples: usize,
    n_harmonics: usize,
    /// 笔画主色（12 类基本色代表色 RGB）与类别名。
    color: [u8; 3],
    color_name: String,
    /// 前 10 个谐波的 [k, mag]（题目要求的紧凑形式）
    top: Vec<[f64; 2]>,
    /// 前 10 个谐波的完整信息（含相位，供前端表格展示）
    harmonics: Vec<HarmonicRow>,
}

#[derive(Serialize, Default)]
struct FilesInfo {
    lineart: String,
    lineart_color: Option<String>,
    preview: String,
    gif: Option<String>,
    svg: String,
    coeffs: String,
}

#[derive(Serialize, Default)]
struct TransformResponse {
    ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    width: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    height: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    mode: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    samples: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    harmonics: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    contours: Option<Vec<ContourRow>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    files: Option<FilesInfo>,
}

#[derive(Clone)]
struct AppState {
    work_dir: PathBuf,
}

// ---------------------------------------------------------------------------
// 入口
// ---------------------------------------------------------------------------

#[tokio::main]
async fn main() {
    let work_dir = std::path::absolute("work").unwrap_or_else(|_| PathBuf::from("work"));
    std::fs::create_dir_all(&work_dir).expect("无法创建 work 目录");

    let port = std::env::var("PORT")
        .ok()
        .and_then(|p| p.trim().parse::<u16>().ok())
        .unwrap_or(17800);

    let state = AppState { work_dir };

    let app = Router::new()
        .route("/", get(index))
        .route("/api/transform", post(transform))
        .route("/files/{*path}", get(serve_file))
        .layer(DefaultBodyLimit::max(256 * 1024 * 1024))
        .with_state(state);

    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    println!("ftrace-web 已启动: http://{addr}");
    println!("工作目录(产物): {}", std::path::absolute("work").unwrap_or_default().display());

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .unwrap_or_else(|e| panic!("无法监听 {addr}: {e}"));
    axum::serve(listener, app).await.unwrap();
}

async fn index() -> Html<&'static str> {
    Html(include_str!("../../web/index.html"))
}

// ---------------------------------------------------------------------------
// POST /api/transform
// ---------------------------------------------------------------------------

async fn transform(State(state): State<AppState>, mut multipart: Multipart) -> Json<TransformResponse> {
    let mut image: Option<(Vec<u8>, Option<String>)> = None; // (bytes, original filename)
    let mut params: HashMap<String, String> = HashMap::new();

    while let Ok(Some(field)) = multipart.next_field().await {
        let Some(name) = field.name().map(str::to_string) else { continue };
        if name == "image" {
            let fname = field.file_name().map(str::to_string);
            if let Ok(bytes) = field.bytes().await {
                image = Some((bytes.to_vec(), fname));
            }
        } else if let Ok(text) = field.text().await {
            params.insert(name, text);
        }
    }

    let id = uuid::Uuid::new_v4();
    let dir = state.work_dir.join(id.to_string());
    if let Err(e) = std::fs::create_dir_all(&dir) {
        return err(&format!("创建工作目录失败: {e}"));
    }

    // 图片模式：保存上传文件；模板模式（--template）：无需输入文件，用伪路径承载元信息
    let input_path = if let Some((data, fname)) = image {
        if data.is_empty() {
            return Json(TransformResponse {
                ok: false,
                error: Some("上传的图片为空".into()),
                ..Default::default()
            });
        }
        let ext = fname
            .as_deref()
            .and_then(|f| Path::new(f).extension().and_then(|e| e.to_str()))
            .map(|e| e.to_ascii_lowercase())
            .filter(|e| !e.is_empty() && e.len() <= 5 && e.chars().all(|c| c.is_ascii_alphanumeric()))
            .unwrap_or_else(|| "png".to_string());
        let p = dir.join(format!("input.{ext}"));
        if let Err(e) = std::fs::write(&p, &data) {
            return err(&format!("保存上传文件失败: {e}"));
        }
        p
    } else if params.get("template").is_some_and(|s| !s.trim().is_empty()) {
        dir.join("template")
    } else {
        return Json(TransformResponse {
            ok: false,
            error: Some("缺少 image 文件字段或 template 参数".into()),
            ..Default::default()
        });
    };

    let opts = build_options(&params);
    let stem = dir.join("out");

    let inp = input_path.clone();
    let job = tokio::task::spawn_blocking(move || ftrace::transform(&inp, Some(&stem), &opts)).await;

    let out = match job {
        Ok(Ok(o)) => o,
        Ok(Err(e)) => return err(&e),
        Err(e) => return err(&format!("转换任务异常: {e}")),
    };

    let base = format!("/files/{id}/");
    let gif = out.gif_path.as_ref().map(|_| format!("{base}out.draw.gif"));

    let contours: Vec<ContourRow> = out
        .report
        .contours
        .iter()
        .map(|c| {
            let hs: Vec<HarmonicRow> = c
                .harmonics
                .iter()
                .take(10)
                .map(|h| HarmonicRow {
                    k: h.k,
                    real: h.real,
                    imag: h.imag,
                    mag: h.mag(),
                    phase: h.imag.atan2(h.real),
                })
                .collect();
            let top: Vec<[f64; 2]> = hs.iter().map(|h| [h.k as f64, h.mag]).collect();
            ContourRow {
                id: c.id,
                perimeter_px: c.perimeter_px,
                area_px: c.area_px,
                n_samples: c.n_samples,
                n_harmonics: c.n_harmonics,
                color: c.color,
                color_name: c.color_name.clone(),
                top,
                harmonics: hs,
            }
        })
        .collect();

    Json(TransformResponse {
        ok: true,
        width: Some(out.width),
        height: Some(out.height),
        mode: Some(out.mode),
        samples: Some(out.samples),
        harmonics: Some(out.harmonics),
        contours: Some(contours),
        files: Some(FilesInfo {
            lineart: format!("{base}out.lineart.png"),
            lineart_color: if params.contains_key("no_color") {
                None
            } else {
                Some(format!("{base}out.lineart_color.png"))
            },
            preview: format!("{base}out.preview.png"),
            gif,
            svg: format!("{base}out.epicycles.svg"),
            coeffs: format!("{base}out.coeffs.json"),
        }),
        ..Default::default()
    })
}

fn err(msg: &str) -> Json<TransformResponse> {
    Json(TransformResponse {
        ok: false,
        error: Some(msg.to_string()),
        ..Default::default()
    })
}

// ---------------------------------------------------------------------------
// GET /files/<path...>
// ---------------------------------------------------------------------------

async fn serve_file(State(state): State<AppState>, AxPath(rel): AxPath<String>) -> Response {
    let work = std::path::absolute(&state.work_dir).unwrap_or_else(|_| state.work_dir.clone());
    let full = match work.join(&rel).canonicalize() {
        Ok(f) => f,
        Err(_) => return plain(StatusCode::NOT_FOUND, "not found"),
    };
    // 防目录穿越：规范化后必须仍位于 work 目录内，且必须是文件。
    if !full.starts_with(&work) || !full.is_file() {
        return plain(StatusCode::FORBIDDEN, "forbidden");
    }
    match tokio::fs::read(&full).await {
        Ok(data) => (
            [(header::CONTENT_TYPE, content_type_for(&full))],
            data,
        )
            .into_response(),
        Err(_) => plain(StatusCode::NOT_FOUND, "not found"),
    }
}

fn plain(status: StatusCode, text: &str) -> Response {
    (status, text.to_string()).into_response()
}

fn content_type_for(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .as_deref()
    {
        Some("png") => "image/png",
        Some("svg") => "image/svg+xml",
        Some("gif") => "image/gif",
        Some("json") => "application/json",
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("webp") => "image/webp",
        Some("bmp") => "image/bmp",
        _ => "application/octet-stream",
    }
}

// ---------------------------------------------------------------------------
// 参数解析（非法值一律回退默认；对范围敏感的值做夹取）
// ---------------------------------------------------------------------------

fn parse_param<T: std::str::FromStr + Copy>(
    params: &HashMap<String, String>,
    key: &str,
    default: T,
) -> T {
    params
        .get(key)
        .and_then(|v| v.trim().parse::<T>().ok())
        .unwrap_or(default)
}

fn parse_bool(params: &HashMap<String, String>, key: &str, default: bool) -> bool {
    params
        .get(key)
        .map(|v| matches!(v.trim().to_ascii_lowercase().as_str(), "1" | "true" | "on" | "yes"))
        .unwrap_or(default)
}

fn clamp<T: PartialOrd>(v: T, lo: T, hi: T) -> T {
    if v < lo {
        lo
    } else if v > hi {
        hi
    } else {
        v
    }
}

fn build_options(params: &HashMap<String, String>) -> Options {
    let d = Options::default();
    let mode = match params
        .get("mode")
        .map(|s| s.trim().to_ascii_lowercase())
        .as_deref()
    {
        Some("edge") => LineArtMode::Edge,
        Some("line") => LineArtMode::Line,
        Some("mask") => LineArtMode::Mask,
        _ => LineArtMode::Auto,
    };
    Options {
        mode,
        blur: clamp(parse_param(params, "blur", d.blur), 0.0_f32, 50.0),
        canny_low: clamp(parse_param(params, "canny_low", d.canny_low), 0.0_f32, 1.0),
        canny_high: clamp(parse_param(params, "canny_high", d.canny_high), 0.0_f32, 1.0),
        samples: clamp(parse_param(params, "samples", d.samples), 16_usize, 16_384),
        harmonics: clamp(parse_param(params, "harmonics", d.harmonics), 1_usize, 16_384),
        max_contours: clamp(parse_param(params, "max_contours", d.max_contours), 1_usize, 500),
        min_area: clamp(parse_param(params, "min_area", d.min_area), 0.0_f64, 1.0e7),
        frames: clamp(parse_param(params, "frames", d.frames), 2_u32, 1_200),
        invert: parse_bool(params, "invert", d.invert),
        no_gif: parse_bool(params, "no_gif", d.no_gif),
        max_dim: clamp(parse_param(params, "max_dim", d.max_dim), 64_u32, 8_192),
        process_dim: clamp(parse_param(params, "process_dim", d.process_dim), 64_u32, 8_192),
        no_circles: parse_bool(params, "no_circles", d.no_circles),
        no_underlay: parse_bool(params, "no_underlay", d.no_underlay),
        no_color: parse_bool(params, "no_color", d.no_color),
        // ---- 播放参数（下游 Eldoria 法阵渲染用）----
        start_stagger: clamp(parse_param(params, "start_stagger", d.start_stagger), 0_u32, 1_200),
        starts: params
            .get("starts")
            .map(|s| {
                s.split(',')
                    .filter_map(|x| x.trim().parse::<u32>().ok())
                    .collect::<Vec<u32>>()
            })
            .unwrap_or_default(),
        ticks_per_cycle: clamp(parse_param(params, "ticks_per_cycle", d.ticks_per_cycle), 1_u32, 2_400),
        scale: clamp(parse_param(params, "scale", d.scale), 0.1_f64, 128.0),
        bind: params
            .get("bind")
            .map(|s| s.trim().to_ascii_lowercase())
            .filter(|s| matches!(s.as_str(), "player" | "origin" | "world"))
            .unwrap_or_else(|| d.bind.clone()),
        height_axis: parse_bool(params, "height_axis", d.height_axis),
        height_per_cycle: clamp(parse_param(params, "height_per_cycle", d.height_per_cycle), -64.0_f64, 64.0),
        loop_play: parse_bool(params, "loop_play", d.loop_play),
        offset_x: clamp(parse_param(params, "offset_x", d.offset_x), -64.0_f64, 64.0),
        offset_y: clamp(parse_param(params, "offset_y", d.offset_y), -64.0_f64, 64.0),
        offset_z: clamp(parse_param(params, "offset_z", d.offset_z), -64.0_f64, 64.0),
        rotation_deg: clamp(parse_param(params, "rotation_deg", d.rotation_deg), -360.0_f64, 360.0),
        line_sheet: parse_bool(params, "line_sheet", d.line_sheet),
        line_names: params
            .get("line_names")
            .map(|s| {
                s.split(',')
                    .map(|x| x.trim().to_string())
                    .filter(|x| !x.is_empty())
                    .collect::<Vec<String>>()
            })
            .unwrap_or_default(),
        template: params
            .get("template")
            .map(|s| s.trim().to_ascii_lowercase())
            .filter(|s| !s.is_empty())
            .clone(),
    }
}
