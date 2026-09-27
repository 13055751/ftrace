#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""ftrace 颜色工具（Python，合并版）

子命令：
  analyze  <图片> [-o 分类图.png]             调色板 + 占比 + 颜色分类图 + JSON
  colorize <coeffs.json> <原图> [-o 前缀]     按原图颜色给每条傅里叶笔画染色
                                             (静态彩色还原图 + 彩色动画 GIF)

依赖 Pillow（本机解压于 ~/.pywheels/pillow）：
  PYTHONPATH=$HOME/.pywheels/pillow python3 tools/ftrace_color.py <子命令> ...
"""
import os
import sys
import json
import math
import argparse

try:
    from PIL import Image, ImageDraw
except ImportError:
    sys.exit("需要 Pillow：PYTHONPATH=$HOME/.pywheels/pillow python3 tools/ftrace_color.py ...")

# ---- 12 类基本色（代表色 + HSV 判定，S/V 为 0..255）----
CATEGORIES = [
    ("黑", (30, 30, 35),       lambda h, s, v: v < 34),
    ("白", (245, 245, 245),    lambda h, s, v: s < 20 and v > 225),
    ("灰", (150, 150, 150),    lambda h, s, v: s < 20),
    ("红", (214, 45, 55),      lambda h, s, v: s >= 20 and (h < 12 or h >= 348)),
    ("棕", (128, 84, 48),      lambda h, s, v: s >= 20 and v < 170 and 12 <= h < 68),
    ("橙", (232, 126, 38),     lambda h, s, v: s >= 20 and 12 <= h < 35),
    ("黄", (232, 198, 48),     lambda h, s, v: s >= 20 and 35 <= h < 68 and v >= 170),
    ("绿", (64, 168, 76),      lambda h, s, v: s >= 20 and 68 <= h < 158),
    ("青", (52, 188, 188),     lambda h, s, v: s >= 20 and 158 <= h < 192),
    ("蓝", (52, 106, 226),     lambda h, s, v: s >= 20 and 192 <= h < 262),
    ("紫", (146, 88, 216),     lambda h, s, v: s >= 20 and 262 <= h < 305),
    ("粉", (232, 126, 178),    lambda h, s, v: s >= 20 and 305 <= h < 348),
]


def classify(rgb):
    r, g, b = rgb
    mx, mn = max(r, g, b), min(r, g, b)
    v = mx
    d = mx - mn
    s = 0 if mx == 0 else int(d * 255 / mx)
    if d == 0:
        h = 0
    elif mx == r:
        h = int(60 * ((g - b) / d) % 360)
    elif mx == g:
        h = int(60 * ((b - r) / d + 2))
    else:
        h = int(60 * ((r - g) / d + 4))
    h %= 360
    for name, rep, cond in CATEGORIES:
        if cond(h, s, v):
            return name, rep
    return "灰", (150, 150, 150)


def rep_of(name):
    for n, rep, _ in CATEGORIES:
        if n == name:
            return rep
    return (150, 150, 150)


# ═══════════════════════════ 子命令: analyze ═══════════════════════════
def cmd_analyze(args):
    img = Image.open(args.image).convert("RGB")
    w, h = img.size
    scale = min(1.0, args.max / max(w, h))
    if scale < 1.0:
        img = img.resize((max(1, int(w * scale)), max(1, int(h * scale))), Image.LANCZOS)
    px = img.load()
    counts, reps = {}, {}
    out = Image.new("RGB", img.size)
    opx = out.load()
    for y in range(img.height):
        for x in range(img.width):
            name, rep = classify(px[x, y])
            counts[name] = counts.get(name, 0) + 1
            reps.setdefault(name, rep)
            opx[x, y] = rep
    total = img.width * img.height
    palette = [{"name": n, "pct": round(c / total * 100, 1), "rgb": list(reps[n])}
               for n, c in sorted(counts.items(), key=lambda kv: -kv[1])]
    report = {"size": list(img.size), "original": os.path.basename(args.image),
              "palette": palette, "main": palette[0]["name"] if palette else "无"}
    stem = os.path.splitext(args.image)[0]
    out_map = args.out or stem + "_colors.png"
    out_json = os.path.splitext(out_map)[0] + ".json"
    out.save(out_map)
    with open(out_json, "w", encoding="utf-8") as f:
        json.dump(report, f, ensure_ascii=False, indent=2)
    print(f"主色: {report['main']}")
    for p in palette:
        print(f"  {p['name']:<4} {p['pct']:>5.1f}%  RGB{p['rgb']}  {'#' * int(p['pct'] / 2)}")
    print(f"分类图: {out_map}")
    print(f"JSON  : {out_json}")


# ═══════════════════════════ 子命令: colorize ═══════════════════════════
def evaluate_path(contour, steps):
    cx, cy = contour["centroid"]
    hs = contour["harmonics"]
    pts = []
    for i in range(steps + 1):
        t = i / steps
        qr = qi = 0.0
        for h in hs:
            th = 2 * math.pi * h["k"] * t
            c, s = math.cos(th), math.sin(th)
            qr += h["real"] * c - h["imag"] * s
            qi += h["real"] * s + h["imag"] * c
        pts.append((cx + qr, cy + qi))
    return pts


def stroke_color(path, px, w, h):
    counts = {}
    n = len(path)
    step = max(1, n // 240)
    for i in range(0, n, step):
        x, y = path[i]
        for dx, dy in ((0, 0), (1, 0), (-1, 0), (0, 1), (0, -1), (1, 1), (-1, 1), (1, -1), (-1, -1)):
            xx, yy = int(round(x + dx)), int(round(y + dy))
            if 0 <= xx < w and 0 <= yy < h:
                name, _ = classify(px[xx, yy])
                counts[name] = counts.get(name, 0) + 1
    if len(path) > 8:  # 封闭轮廓再采内部
        xs = [p[0] for p in path]
        ys = [p[1] for p in path]
        cx, cy = sum(xs) / len(xs), sum(ys) / len(ys)
        for k in range(32):
            ang = k / 32 * 2 * math.pi
            xx, yy = int(cx + min(w, h) * 0.012 * math.cos(ang)), int(cy + min(w, h) * 0.012 * math.sin(ang))
            if 0 <= xx < w and 0 <= yy < h:
                name, _ = classify(px[xx, yy])
                counts[name] = counts.get(name, 0) + 1
    pref = [n for n, _ in sorted(counts.items(), key=lambda kv: -kv[1]) if n not in ("白", "灰", "黑")]
    return rep_of(pref[0]) if pref else (40, 40, 45)


def cmd_colorize(args):
    d = json.load(open(args.coeffs, encoding="utf-8"))
    W, H = d["width"], d["height"]
    img = Image.open(args.image).convert("RGB").resize((W, H), Image.LANCZOS)
    px = img.load()
    stem = args.out or os.path.splitext(args.coeffs)[0]

    canvas = Image.new("RGB", (W, H), (255, 255, 255))
    draw = ImageDraw.Draw(canvas)
    paths, colors = [], []
    for c in d["contours"]:
        p = evaluate_path(c, args.steps)
        col = stroke_color(p, px, W, H)
        paths.append(p)
        colors.append(col)
        for i in range(len(p) - 1):
            draw.line([p[i], p[i + 1]], fill=col, width=args.width)
    canvas.save(stem + ".colored.png")
    print(f"彩色还原图: {stem}.colored.png ({len(paths)} 条笔画)")

    if not args.no_gif:
        frames, nf = [], max(2, args.frames)
        for f in range(nf):
            frame = Image.new("RGB", (W, H), (255, 255, 255))
            df = ImageDraw.Draw(frame)
            frac = f / (nf - 1)
            for p, col in zip(paths, colors):
                df.line(p[:max(1, int(len(p) * frac))], fill=col, width=args.width)
            frames.append(frame)
        gif = stem + ".colored.gif"
        frames[0].save(gif, save_all=True, append_images=frames[1:], duration=60, loop=0)
        print(f"彩色动画: {gif} ({nf} 帧)")


def main():
    ap = argparse.ArgumentParser(description="ftrace 颜色工具（分析 + 彩色傅里叶）", prog="ftrace_color.py")
    sub = ap.add_subparsers(dest="cmd", required=True)

    p1 = sub.add_parser("analyze", help="颜色调色板 + 分类图")
    p1.add_argument("image")
    p1.add_argument("-o", "--out", default=None)
    p1.add_argument("--max", type=int, default=512)
    p1.set_defaults(fn=cmd_analyze)

    p2 = sub.add_parser("colorize", help="按颜色给傅里叶笔画染色")
    p2.add_argument("coeffs")
    p2.add_argument("image")
    p2.add_argument("-o", "--out", default=None)
    p2.add_argument("--width", type=int, default=2)
    p2.add_argument("--frames", type=int, default=96)
    p2.add_argument("--steps", type=int, default=640)
    p2.add_argument("--no-gif", action="store_true")
    p2.set_defaults(fn=cmd_colorize)

    args = ap.parse_args()
    args.fn(args)


if __name__ == "__main__":
    main()