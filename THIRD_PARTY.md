# 第三方开源致谢与许可证声明（ftrace）

本项目自己的代码使用 **MIT OR Apache-2.0** 双许可证（见 LICENSE-MIT / LICENSE-APACHE）。
以下开源项目为本项目提供了算法参照、技术路线与依赖，特此致谢并声明许可证兼容性。

## 参考的开源项目（算法/技术路线来源）

| 项目 | 许可证 | 本项目如何使用 |
|---|---|---|
| [yingkitw/img2svg](https://github.com/yingkitw/img2svg) | Apache-2.0 | 线稿/轮廓管线参考：Sobel 边缘、轮廓提取、图像→矢量思路 |
| [theatrus/seiza (seiza-imgproc)](https://github.com/theatrus/seiza) | Apache-2.0 | **实际依赖**：`seiza_imgproc::contours::find_external_contours`（Suzuki-Abe 外轮廓追踪，OpenCV findContours 移植）；Canny 算法也以其为参照（本项目按算法自实现，未复制代码） |
| [The Coding Train #130](https://thecodingtrain.com/challenges/130-drawing-with-fourier-transform-and-epicycles) | 教程型开源（MIT 风格教学材料） | DFT 路径分解 + 按幅值排序谐波 + 本轮圆盘链画法 |
| [laurentcarrie/fluffy-eureka (circles-sketch)](https://github.com/laurentcarrie/fluffy-eureka) | 见其仓库 | 轮廓插值→复 DFT→谐波列表 的数据组织方式 |
| [Inokinoki/fourier-svg-rs](https://github.com/Inokinoki/fourier-svg-rs) | MIT | SVG/GIF/JSON 导出形态参考 |
| [STPR/contour_tracing](https://github.com/STPR/contour_tracing) | EUPL-1.2 | **未作为依赖使用**（license 与 MIT/Apache 不兼容）；轮廓追踪改用 seiza-imgproc 实现 |

依据各许可证要求：Apache-2.0 项目若未直接复制其源码文本，仅需保留本致谢声明；若未来
直接引入其代码段，需随文件保留原 LICENSE 文本与本段声明。

## Rust 依赖（cargo 自动获取，许可证以各 crate 发布元数据为准）

| crate | 许可证 |
|---|---|
| image 0.25 | MIT OR Apache-2.0 |
| imageproc 0.27 | MIT |
| rustfft 6.x | MIT OR Apache-2.0 |
| num-complex 0.4 | MIT OR Apache-2.0 |
| serde / serde_json | MIT OR Apache-2.0 |
| gif | MIT OR Apache-2.0 |
| clap | Apache-2.0 OR MIT |
| rayon | MIT OR Apache-2.0 |
| seiza-imgproc | Apache-2.0 |

## 测试素材来源

- `test_images/heart_transparent.png`：Wikimedia Commons 文件
  `Red-simple-heart-icon-transparent-background.png`（[CC0 1.0 公有领域](https://commons.wikimedia.org/wiki/File:Red-simple-heart-icon-transparent-background.png)）
- `test_images/heart_symbol.png`：Wikimedia Commons 文件
  `Simple_heart_symbol_-_01.png`（[公有领域](https://commons.wikimedia.org/wiki/File:Simple_heart_symbol_-_01.png)）
- `test_images/circle_simple.svg`：Wikimedia Commons（公有领域，纯几何图形不具版权）
- `test_images/star.ppm` / `ring.ppm` / `wave.ppm`：本项目生成的合成测试图（公有领域）
- `test_images/anime_character.jpg`：用户提供的测试图片，仅作本地测试