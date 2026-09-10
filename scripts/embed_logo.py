#!/usr/bin/env python3
"""embed_logo.py — 把栅格 logo（logo.jpg）包装成 assets/logo/rinadown_logo.svg

为什么要这一步：
    全平台图标流水线（scripts/gen_icons.ts）以 assets/logo/rinadown_logo.svg
    为唯一矢量源。logo 换成栅格插画后，该 SVG 变成「内嵌 JPEG + 圆角 clipPath」
    的包装文件——几何（viewBox 30…482、圆角矩形 56…456、rx=88）与旧矢量版
    完全一致，于是 gen_icons.ts 与各平台打包脚本都不需要改。

用法:
    python scripts/embed_logo.py [源图] [--size 1024] [--quality 88]

    源图默认取仓库根目录的 logo.jpg；换 logo 时替换它后重跑本脚本，再跑
    `bun scripts/gen_icons.ts` 重生成全平台图标。

依赖: Pillow (`python -m pip install Pillow`)
"""

from __future__ import annotations

import argparse
import base64
import io
import sys
from pathlib import Path

try:
    from PIL import Image
except ImportError:  # pragma: no cover
    sys.exit("找不到 Pillow，请先 `python -m pip install Pillow`")

REPO_ROOT = Path(__file__).resolve().parent.parent
OUT_SVG = REPO_ROOT / "assets" / "logo" / "rinadown_logo.svg"

# ── 与旧矢量 logo 对齐的几何（单位 = SVG 用户单位）────────────────
VIEW_MIN = 30.0  # viewBox 起点
VIEW_SIZE = 452.0  # viewBox 边长
RECT_MIN = 56.0  # 圆角矩形起点
RECT_SIZE = 400.0  # 圆角矩形边长
RECT_RADIUS = 88.0  # 圆角半径


def build_svg(jpeg_bytes: bytes, size: int) -> str:
    b64 = base64.b64encode(jpeg_bytes).decode("ascii")
    return (
        "<!-- 由 scripts/embed_logo.py 从 logo.jpg 生成，请勿手改 -->\n"
        '<svg xmlns="http://www.w3.org/2000/svg" '
        'xmlns:xlink="http://www.w3.org/1999/xlink" '
        f'viewBox="{VIEW_MIN:g} {VIEW_MIN:g} {VIEW_SIZE:g} {VIEW_SIZE:g}" '
        f'width="{size}" height="{size}">\n'
        "  <defs>\n"
        '    <clipPath id="rinadown-logo-frame">\n'
        f'      <rect x="{RECT_MIN:g}" y="{RECT_MIN:g}" width="{RECT_SIZE:g}" '
        f'height="{RECT_SIZE:g}" rx="{RECT_RADIUS:g}" ry="{RECT_RADIUS:g}"/>\n'
        "    </clipPath>\n"
        "  </defs>\n"
        f'  <image x="{RECT_MIN:g}" y="{RECT_MIN:g}" width="{RECT_SIZE:g}" '
        f'height="{RECT_SIZE:g}" preserveAspectRatio="xMidYMid slice" '
        'clip-path="url(#rinadown-logo-frame)" '
        f'href="data:image/jpeg;base64,{b64}"/>\n'
        "</svg>\n"
    )


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("source", nargs="?", default=str(REPO_ROOT / "logo.jpg"))
    ap.add_argument("--size", type=int, default=1024, help="内嵌 JPEG 边长（默认 1024）")
    ap.add_argument("--quality", type=int, default=88, help="内嵌 JPEG 质量（默认 88）")
    args = ap.parse_args()

    src = Path(args.source)
    if not src.is_file():
        return f"源图不存在: {src}"

    with Image.open(src) as im:
        im = im.convert("RGB")
        # 方形裁切（cover）后缩放，保证内嵌图本身就是正方形
        w, h = im.size
        side = min(w, h)
        im = im.crop(
            ((w - side) // 2, (h - side) // 2, (w - side) // 2 + side, (h - side) // 2 + side)
        )
        im = im.resize((args.size, args.size), Image.Resampling.LANCZOS)
        buf = io.BytesIO()
        im.save(buf, "JPEG", quality=args.quality, optimize=True, progressive=True)

    svg = build_svg(buf.getvalue(), args.size)
    OUT_SVG.parent.mkdir(parents=True, exist_ok=True)
    OUT_SVG.write_text(svg, encoding="utf-8")
    print(f"✓ {OUT_SVG.relative_to(REPO_ROOT)} ({len(svg) / 1024:.1f} KB)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
