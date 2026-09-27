# ftrace — 图片 → 黑白线稿 → 傅里叶（本轮/离心轮）变换

纯 Rust 命令行工具：把任意图片转成黑白线稿，从线稿提取闭环轮廓，对每个轮廓做一次复
**离散傅里叶变换（DFT）**，输出系数 JSON、本轮圆盘 SVG、还原预览 PNG，以及"一笔一画"
绘制动画 GIF。一张图可包含**一个或多个**独立轮廓，因此对应**一个或多个**傅里叶变换。

本机（Linux **aarch64/arm64**）编译，产物为可执行二进制，无 Python 运行时依赖。

## 用法

```bash
./target/release/ftrace <图片路径> [选项]
```

选项（`ftrace --help` 查看全部）：

| 选项 | 默认 | 说明 |
|---|---|---|
| `-o, --output STEM` | 取输入名 | 输出文件前缀（生成 `STEM_lineart.png` 等） |
| `--mode auto\|edge\|line\|mask` | auto | 线稿模式：auto=有 alpha 走 mask、否则 edge；edge=Canny 边缘（照片/插画）；**line=已有线稿直接二值化→Zhang-Suen 细化到 1px 中心线→链追踪（不跑 Canny，避免细线双描）**；mask=前景/alpha 掩膜轮廓（实体图形/logo） |
| `--blur σ` | 1.2 | Canny 前高斯模糊σ（越大线条越粗越连贯） |
| `--canny-low / --canny-high` | 0.08 / 0.22 | Canny 迟滞阈值（占最大梯度幅值比例） |
| `--samples N` | 1024 | 每轮廓弧长均匀采样点数（2 的幂最优） |
| `--harmonics N` | 256 | 每轮廓保留的最大谐波数（越多还原越准、文件越大） |
| `--max-contours K` | 12 | 最多保留的轮廓/笔画链数（按周长从大到小） |
| `--min-area px` | 30 | 轮廓最小面积/笔画最小长度（滤噪点） |
| `--frames N` | 96 | 动画帧数 |
| `--invert` | off | mask 模式反转前景/背景极性（浅色主体深色背景） |
| `--max-dim px` | 1600 | 渲染画布最长边上限 |
| `--process-dim px` | 2048 | 线稿处理阶段最长边上限（超大图先降采样，省内存提速） |
| `--no-circles` | off | 动画/SVG 只画轨迹、不画本轮圆盘 |
| `--no-gif` | off | 不生成 GIF |
| `--no-underlay` | off | 预览图不叠加线稿底纹 |

**法阵模板库（无需输入图片）**：`--template <名称>` 用参数化几何直接生成法阵线条
（`hexagram 六芒星 / starfield 星辰阵 / rune_ring 符文环 / element 元素徽记`），
每条线带稳定语义名（`outer_ring`/`star_up`/`pentagram`/`fire`…），可 `--line-names` 覆盖：

```bash
# 六芒星法阵：4 条线（外环+上下三角+内环），配线索引图 + 逐笔错开起始
./target/release/ftrace --template hexagram --line-sheet --start-stagger 5 \
  --harmonics 128 --scale 8 -o /tmp/hex
# 直接进 Eldoria：把产物 coeffs.json 改名为 hexagram.json 放入 <技能>/fx/，
# main.json 里 "fx": "hexagram.json" 即可
```

调试（环境变量，可选）：`FT_TIMING=1`（分阶段耗时）、`FT_TRACE_STATS=1`（追踪统计）、
`FT_DEBUG_MASK=1`（掩膜极性统计）、`FT_DEBUG_LOOP=1`（轮廓头部坐标）。

## 网页端（ftrace-web）

本地 Web 服务 + 网页前端：浏览器上传图片、调参数，实时看线稿 / 傅里叶系数 / 本轮动画。

```bash
cargo run --release --bin ftrace-web     # 或 ./target/release/ftrace-web
# 打开 http://127.0.0.1:17800  （PORT / WORK_DIR 环境变量可改端口与产物目录）
```

端点：

| 方法 | 路径 | 说明 |
|---|---|---|
| GET | `/` | 前端单页（深色主题：拖拽上传、全部参数控件、缩略预览、GIF 动画、系数表含相位°、下载按钮） |
| POST | `/api/transform` | multipart：`image` 文件 + 可选参数（mode/blur/canny_low/canny_high/samples/harmonics/max_contours/min_area/frames/invert/no_gif/max_dim/no_color 等，非法值自动回退默认并 clamp 范围）→ JSON（逐轮廓统计 + 颜色 + 产物 URL） |
| GET | `/files/{path..}` | 下载产物（规范化路径 + 前缀校验防目录穿越） |

网页端与 CLI 共用同一套 `ftrace::transform` 管线，产物一致。**喂已有线稿时请在网页端选 `line` 模式并把 max_contours 调大**——edge 模式会对细线稿跑 Canny 造成"双描"（每条线描成两条平行线），mask/auto 则会把线稿误判成"实体填充"描出整块轮廓。网页端"彩色线条"复选框默认勾选（取消即 `no_color`）。

### 输出文件

| 文件 | 内容 |
|---|---|
| `STEM_lineart.png` | 黑白线稿（白纸黑线） |
| `STEM_lineart_color.png` | **彩色线稿：线条自带原图区域主色**（辅助法阵绘制/函数取色） |
| `STEM_coeffs.json` | 每个轮廓一个傅里叶变换：`harmonics[].k/real/imag` + **`color`/`color_name`（笔画主色分类）** + centroid/bbox/面积/周长 |
| `STEM_epicycles.svg` | 本轮圆盘链（t=0）+ 还原轨迹矢量图 |
| `STEM_preview.png` | 还原末帧 + 线稿底纹（默认按笔画主色着色；`--no-color` 回退红色） |
| `STEM_draw.gif` | "一笔一画"绘制动画（默认彩色轨迹） |

### 颜色识别

每条笔画/轮廓会按**原图区域主色**分类到 12 类基本色（红/橙/黄/绿/青/蓝/紫/粉/棕/黑/白/灰），
代表色写入 `coeffs.json` 的 `contours[].color`，并用于：
- 彩色线稿 `STEM_lineart_color.png`（线条本身带色，方便函数取色）
- 彩色的傅里叶还原预览与动画（默认开，`--no-color` 关闭）

辅助工具（Python，用户拍板颜色方案先用 Python 原型）：
```bash
PYTHONPATH=$HOME/.pywheels/pillow python3 tools/ftrace_color.py analyze   <图片> [-o 分类图.png]
PYTHONPATH=$HOME/.pywheels/pillow python3 tools/ftrace_color.py colorize <coeffs.json> <原图> [-o 前缀]
```

### 示例

```bash
# 带透明背景的图标（自动走 alpha 掩膜）
./target/release/ftrace test_images/heart_transparent.png

# 照片/插画：Canny 边缘
./target/release/ftrace test_images/anime_character.jpg --mode edge --blur 1.5

# 彩色实体图形：Otsu 掩膜轮廓
./target/release/ftrace test_images/heart_symbol.png --mode mask

# 高保真：更多谐波 + 更多采样
./target/release/ftrace logo.png --harmonics 512 --samples 2048 --frames 144
```

## 原理

1. **线稿**：`edge` 模式 = 灰度 → 高斯模糊 → Canny（Sobel 梯度 + 非极大抑制 + 双阈值迟滞）；
   `mask` 模式 = alpha 通道或 Otsu 阈值得到前景掩膜（**自动极性**：背景通常占据图像边框，
   以边框主色判定前后景，主体占满画面时自动退回"少数派"规则）→ 取其 1px 外轮廓环展示。
2. **轮廓/笔画**：`edge` 模式对细笔画做**笔画链追踪**（沿 8 连通中心线提取开放路径，分支点断开，
   弧长均匀重采样后以"去程+回程"闭合，避免横穿画面的跳线）；`mask` 模式用
   **Suzuki-Abe 外轮廓追踪**（seiza-imgproc，OpenCV `findContours` 的纯 Rust 移植）得到
   每连通组件一个外轮廓。再按面积/长度过滤、去重、限制数量。
3. **傅里叶**：把采样点视为复数 z[n]=x[n]+i·y[n]（去质心后），用 FFT 求
   `c[k] = (1/N)Σ z[n]·e^(−2πikn/N)`，按 |c[k]| 从大到小保留前 M 个谐波。
   每个谐波 = 一个旋转圆（半径 |c[k]|、频率 k、初相位 arg c[k]），串联即"本轮圆盘"：
   `z(t) = Σ c[k]·e^(2πikt)`。谐波越多，轨迹越逼近原轮廓。一条图片可含**多个轮廓/笔画**，
   每个各自一次傅里叶变换。
4. **输出**：SVG/GIF/PNG 渲染本轮链 + 渐进轨迹；JSON 保存系数（k/real/imag + 元数据）
   供其他程序二次使用。

## 性能参考（本机 aarch64，release 构建，96 帧 GIF 全输出）

| 输入 | 尺寸 | 耗时 |
|---|---|---|
| 合成图形（星/环/心） | 400–500px | <1s |
| 动漫插画（edge，12 条笔画链） | 862px | ~2.5s |
| 建筑照片（edge，2048px 降采样） | 5184×3456 → 2048 | ~6s |

## 已知限制

- mask 模式使用 RETR_EXTERNAL：只取每个连通组件的外轮廓，**组件内孔（如圆环内圈）不单独输出**。
- 傅里叶截断的固有性质：高曲率处（发梢、尖角）端点会被适度磨圆，尖角附近可能出现轻微振铃；
  这是本轮画法的通用现象（参考实现同级别效果），提高 `--harmonics` 可缓解但会放大振铃。
- 笔画在一点相切/粘连（数字拓扑病理情形）可能产生合并路径。

## 开源致谢与许可证

本项目代码采用 **MIT OR Apache-2.0** 双许可证。算法与工程实践取自以下开源项目（详见
[THIRD_PARTY.md](THIRD_PARTY.md) 的逐项许可证说明）：

- [img2svg](https://github.com/yingkitw/img2svg)（Apache-2.0）：Sobel 边缘 + 轮廓提取 → SVG 思路
- [seiza-imgproc](https://github.com/theatrus/seiza)（Apache-2.0）：Canny 算法参照 + **外轮廓追踪实际依赖**（Suzuki-Abe，OpenCV findContours 移植）
- [The Coding Train #130](https://thecodingtrain.com/challenges/130-drawing-with-fourier-transform-and-epicycles)（教学性开源）：DFT 路径 + 本轮圆盘链画法
- [fluffy-eureka / circles-sketch](https://github.com/laurentcarrie/fluffy-eureka)（其 crate `circles-sketch`）：轮廓→复 DFT→谐波列表
- [fourier-svg-rs](https://github.com/Inokinoki/fourier-svg-rs)（MIT）：SVG/GIF/JSON 导出格式
- 运行依赖 crate：`image`、`imageproc`、`rustfft`、`serde`、`serde_json`、`gif`、`clap`、
  `num-complex`、`rayon`、`seiza-imgproc`（均为 MIT 或 Apache-2.0）

> 注：`contour_tracing` crate 因采用 EUPL-1.2（copyleft）与本项目 MIT/Apache 许可证不兼容，
> 未引入；轮廓追踪改用 Apache-2.0 的 seiza-imgproc（Suzuki-Abe）实现。

## 构建

```bash
cargo build --release        # 产物: target/release/ftrace
cargo run --release -- --help
```

## 测试

```bash
# 合成图形（PPM，验证几何正确）：星形 / 圆环(含内孔→多轮廓) / 波浪线
./target/release/ftrace test_images/star.ppm
./target/release/ftrace test_images/ring.ppm
./target/release/ftrace test_images/wave.ppm
# Wikimedia Commons 公有领域图形
./target/release/ftrace test_images/heart_transparent.png
./target/release/ftrace test_images/heart_symbol.png --mode mask
# 动漫插画
./target/release/ftrace test_images/anime_character.jpg --mode edge
```

## 已知限制与路线图

- Moore 边界追踪对"笔画在一点相切/粘连"的重合像素可能产生合并环（属数字拓扑病理情形）。
- 轮廓内孔（如圆环内圈）会作为独立轮廓输出（这正是"多个傅里叶变换"的用途之一）。
- 后续可加：Suzuki-Abe 层级轮廓（内外孔关联）、Zhang-Suen 骨架化（细线单笔画）、
  FFT 谐波复用的多轮廓并行（rayon）、HSV/去色预处理选项。