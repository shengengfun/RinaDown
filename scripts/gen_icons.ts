#!/usr/bin/env bun
/**
 * gen_icons.ts — 从 rinadown_logo.svg 生成全平台图标
 *
 * 用法:
 *     bun scripts/gen_icons.ts
 *
 * 依赖: sharp (bun 全局已安装)
 *
 * 源文件 assets/logo/rinadown_logo.svg 自 logo 换成栅格插画（logo.jpg）后，
 * 本身是一张「内嵌 JPEG + 圆角 clipPath」的 SVG 包装，几何（viewBox 30…482、
 * 圆角矩形 56…456、rx=88）与旧矢量版完全一致——流水线无需知道源是栅格。
 * 换 logo 时改 logo.jpg 后用 scripts/embed_logo.py 重生成该 SVG，再跑本脚本。
 *
 * 从 assets/logo/rinadown_logo.svg 生成以下全部图标:
 *
 *   assets/logo/
 *     rinadown_logo.png (600×600)
 *     rinadown_bolt.png (512×512, 内置备选应用图标「闪电」)
 *     logo.png (600×600)
 *     tray_iconTemplate.png (36×36, macOS 2x 菜单栏模板图标)
 *     tray_iconTemplate@1x.png (18×18, macOS 1x 菜单栏模板图标)
 *
 *   windows/runner/resources/
 *     app_icon.ico (16,32,48,64,256 多分辨率 ICO；Flutter 与 GPUI PC 客户端共用)
 *     tray_win_dark.ico (16,32 — 深色任务栏托盘图标)
 *     tray_win_light.ico (16,32 — 浅色任务栏托盘图标)
 *
 *   installer/windows/
 *     wizard_small.bmp (55×55 24-bit BMP — Inno Setup 向导右上角小图，
 *                       即安装包 logo，与应用图标同源)
 *
 *   macos/Runner/Assets.xcassets/AppIcon.appiconset/
 *     app_icon_{16,32,64,128,256,512,1024}.png
 *
 *   ios/Runner/Assets.xcassets/AppIcon.appiconset/
 *     Icon-App-20x20@{1x,2x,3x}.png  Icon-App-29x29@{1x,2x,3x}.png
 *     Icon-App-40x40@{1x,2x,3x}.png  Icon-App-60x60@{2x,3x}.png
 *     Icon-App-76x76@{1x,2x}.png     Icon-App-83.5x83.5@2x.png
 *     Icon-App-1024x1024@1x.png
 *
 *   android/app/src/main/res/
 *     mipmap-{mdpi,hdpi,xhdpi,xxhdpi,xxxhdpi}/ic_launcher.png
 *
 *   rinaDown/public/icon/
 *     {16,32,48,128}.png  {16,32,48,128}-disabled.png
 *     rinadown_logo.png (128×128)  rinadown_logo.svg (副本)
 *
 *   web/public/  (Vite React 客户端 + 服务器内嵌 SPA)
 *     favicon.svg  logo.png (256×256)
 *
 *   website/public/
 *     favicon.ico  favicon.svg  logo.png (1024×1024)  logo.svg (副本)
 */

import sharp from "sharp";
import {
  existsSync,
  mkdirSync,
  writeFileSync,
  copyFileSync,
  readFileSync,
} from "fs";
import { join, resolve, dirname } from "path";

// ─── 项目根目录 ───────────────────────────────────────────────────
const REPO_ROOT = resolve(
  dirname(new URL(import.meta.url).pathname.replace(/^\/([A-Z]:)/, "$1")),
);
// scripts/ 的父目录即项目根
const ROOT = resolve(REPO_ROOT, "..");

const SVG_SRC = join(ROOT, "assets", "logo", "rinadown_logo.svg");
// 内置备选图标「闪电」源 SVG（可选 — 不存在时跳过生成）
const BOLT_SVG_SRC = join(ROOT, "assets", "logo", "rinadown_bolt.svg");

if (!existsSync(SVG_SRC)) {
  console.error(`❌ 源文件不存在: ${SVG_SRC}`);
  process.exit(1);
}

// ─── 工具函数 ──────────────────────────────────────────────────────

function ensureDir(filePath: string): void {
  const dir = dirname(filePath);
  if (!existsSync(dir)) {
    mkdirSync(dir, { recursive: true });
  }
}

/** 从 SVG 渲染出指定尺寸的 RGBA PNG Buffer */
async function renderPng(size: number): Promise<Buffer> {
  // 使用高 density 渲染，确保清晰度（SVG viewBox 3508×3508）
  // density=72 → 原始尺寸, 我们用 resize 缩放以获得最佳质量
  return renderPngFrom(SVG_SRC, size);
}

/** 从任意 SVG 源渲染出指定尺寸的 RGBA PNG Buffer */
async function renderPngFrom(src: string, size: number): Promise<Buffer> {
  return sharp(src, { density: 300 })
    .resize(size, size, {
      kernel: sharp.kernel.lanczos3,
      fit: "contain",
      background: { r: 0, g: 0, b: 0, alpha: 0 },
    })
    .png({ compressionLevel: 9 })
    .toBuffer();
}

/**
 * 生成 macOS 应用图标 — 遵循 Apple HIG：圆角矩形形状精确占画布 824/1024（≈80.5%），
 * 四周各留 100px 透明边距（1024 画布）。系统会自动为图标叠加阴影，故源 SVG 不含手绘阴影，
 * viewBox 已收紧到圆角矩形边界（内容占比 1.0），此处直接把内容渲染到画布的 824/1024。
 */
const MAC_RECT_RATIO = 824 / 1024; // Apple HIG：形状占画布比例
async function renderMacAppIcon(size: number): Promise<Buffer> {
  const content = Math.round(size * MAC_RECT_RATIO);
  const logo = await renderPngFrom(SVG_SRC, content);
  return sharp({
    create: {
      width: size,
      height: size,
      channels: 4,
      background: { r: 0, g: 0, b: 0, alpha: 0 },
    },
  })
    .composite([{ input: logo, gravity: "center" }])
    .png({ compressionLevel: 9 })
    .toBuffer();
}

/** 生成灰度（disabled）版本的 PNG Buffer */
async function renderDisabledPng(size: number): Promise<Buffer> {
  const src = await renderPng(size);
  // 去色 + 降低对比度 → "禁用" 外观
  return (
    sharp(src)
      .grayscale()
      // 降低整体亮度使其看起来更"禁用"
      .modulate({ brightness: 0.6 })
      .png({ compressionLevel: 9 })
      .toBuffer()
  );
}

/**
 * 生成 macOS 菜单栏模板图标 — 纯黑圆角方块（透明背景）。
 *
 * macOS 模板图标规范:
 *   - 仅黑色 + alpha，系统按菜单栏外观自动着色（深色变白、浅色变黑）
 *   - 新 logo 是整块栅格插画，无法做成可辨认的剪影，故只保留图标本身的
 *     圆角方块轮廓（与应用图标的圆角几何一致）。
 *   几何与 rinadown_logo.svg 对齐: viewBox 30…482，圆角矩形 56…456，rx=88。
 */
async function renderMacTrayTemplate(size: number): Promise<Buffer> {
  const svg = `<svg width="512" height="512" viewBox="30 30 452 452" xmlns="http://www.w3.org/2000/svg">
    <rect x="56" y="56" width="400" height="400" rx="88" ry="88" fill="#000000"/>
  </svg>`;
  return sharp(Buffer.from(svg), { density: 300 })
    .resize(size, size, {
      kernel: sharp.kernel.lanczos3,
      fit: "contain",
      background: { r: 0, g: 0, b: 0, alpha: 0 },
    })
    .png({ compressionLevel: 9 })
    .toBuffer();
}

// 旧 logo（蓝色箭头矢量）专用的「亮度阈值剪影」与「纯色箭头」两个生成器已随
// logo 切换删除：托盘 / 菜单栏图标现在直接复用应用图标本身。

/**
 * 手动构建 ICO 文件（多分辨率，PNG 压缩帧）
 *
 * ICO 布局:
 *   [6B  文件头]        reserved=0, type=1, count=N
 *   [16B × N 目录条目]  width, height, colorCount, reserved, planes, bpp, size, offset
 *   [各帧 PNG 数据]
 */
function buildIco(frames: { size: number; data: Buffer }[]): Buffer {
  const n = frames.length;

  // 文件头: 6 bytes
  const header = Buffer.alloc(6);
  header.writeUInt16LE(0, 0); // reserved
  header.writeUInt16LE(1, 2); // type = ICO
  header.writeUInt16LE(n, 4); // count

  // 目录条目: 16 bytes each
  const directory = Buffer.alloc(16 * n);
  let dataOffset = 6 + 16 * n;

  for (let i = 0; i < n; i++) {
    const { size, data } = frames[i];
    const offset = i * 16;

    // width/height: 0 代表 256（ICO 规范，单字节无法存 256）
    directory.writeUInt8(size < 256 ? size : 0, offset); // width
    directory.writeUInt8(size < 256 ? size : 0, offset + 1); // height
    directory.writeUInt8(0, offset + 2); // color count (0 = true color)
    directory.writeUInt8(0, offset + 3); // reserved
    directory.writeUInt16LE(1, offset + 4); // planes
    directory.writeUInt16LE(32, offset + 6); // bits per pixel (RGBA)
    directory.writeUInt32LE(data.length, offset + 8); // data size
    directory.writeUInt32LE(dataOffset, offset + 12); // data offset

    dataOffset += data.length;
  }

  return Buffer.concat([header, directory, ...frames.map((f) => f.data)]);
}

/**
 * 手写 24 位 BMP（bottom-up、BGR、每行按 4 字节对齐）。
 *
 * 用途：Inno Setup 的 `WizardSmallImageFile`（安装向导右上角小图 = 安装包 logo）。
 * 所有 Inno 版本都接受 BMP，而 PNG/JPEG 要 6.3+；sharp 又没有 BMP 输出格式，
 * 所以这里自己把栅格像素封装成 BMP。透明区域合成白底（向导背景为浅色）。
 */
async function buildBmp(size: number, png: Buffer): Promise<Buffer> {
  const { data } = await sharp(png)
    .flatten({ background: { r: 255, g: 255, b: 255 } })
    .resize(size, size, { kernel: sharp.kernel.lanczos3 })
    .raw()
    .toBuffer({ resolveWithObject: true });

  const rowBytes = size * 3;
  const stride = rowBytes + ((4 - (rowBytes % 4)) % 4);
  const pixelBytes = stride * size;

  // BITMAPFILEHEADER (14B) + BITMAPINFOHEADER (40B)
  const header = Buffer.alloc(54);
  header.write("BM", 0, "ascii");
  header.writeUInt32LE(54 + pixelBytes, 2); // bfSize
  header.writeUInt32LE(0, 6); // bfReserved
  header.writeUInt32LE(54, 10); // bfOffBits
  header.writeUInt32LE(40, 14); // biSize
  header.writeInt32LE(size, 18); // biWidth
  header.writeInt32LE(size, 22); // biHeight (>0 → bottom-up)
  header.writeUInt16LE(1, 26); // biPlanes
  header.writeUInt16LE(24, 28); // biBitCount
  header.writeUInt32LE(0, 30); // biCompression = BI_RGB
  header.writeUInt32LE(pixelBytes, 34); // biSizeImage
  header.writeInt32LE(2835, 38); // biXPelsPerMeter
  header.writeInt32LE(2835, 42); // biYPelsPerMeter
  header.writeUInt32LE(0, 46); // biClrUsed
  header.writeUInt32LE(0, 50); // biClrImportant

  const body = Buffer.alloc(pixelBytes);
  for (let y = 0; y < size; y++) {
    const srcRow = (size - 1 - y) * size * 3; // BMP 自下而上
    const dstRow = y * stride;
    for (let x = 0; x < size; x++) {
      const s = srcRow + x * 3;
      const d = dstRow + x * 3;
      body[d] = data[s + 2]; // B
      body[d + 1] = data[s + 1]; // G
      body[d + 2] = data[s]; // R
    }
  }

  return Buffer.concat([header, body]);
}

/** 保存 Buffer 到文件，打印路径 */
async function saveFile(relPath: string, data: Buffer | string): Promise<void> {
  const absPath = join(ROOT, relPath);
  ensureDir(absPath);
  writeFileSync(absPath, data);
  const size = Buffer.isBuffer(data) ? data.length : Buffer.byteLength(data);
  const sizeStr = size > 1024 ? `${(size / 1024).toFixed(1)} KB` : `${size} B`;
  console.log(`  ✓ ${relPath} (${sizeStr})`);
}

/** 渲染并保存指定尺寸 PNG */
async function savePng(relPath: string, size: number): Promise<void> {
  const buf = await renderPng(size);
  await saveFile(relPath, buf);
}

// ─── 预缓存常用尺寸 ──────────────────────────────────────────────
// 同一尺寸可能被多处使用，缓存避免重复渲染
const pngCache = new Map<number, Buffer>();

async function getCachedPng(size: number): Promise<Buffer> {
  if (!pngCache.has(size)) {
    pngCache.set(size, await renderPng(size));
  }
  return pngCache.get(size)!;
}

// ─── 主流程 ────────────────────────────────────────────────────────

async function main() {
  console.log("🦅 RinaDown 全平台图标生成器");
  console.log(`   源文件: ${SVG_SRC}`);
  console.log("");

  let totalCount = 0;

  // ──────────────────────────────────────────
  // 1. assets/logo/ — 源 PNG 和托盘模板图标
  // ──────────────────────────────────────────
  console.log("📁 assets/logo/");
  {
    const logo600 = await getCachedPng(600);
    await saveFile("assets/logo/rinadown_logo.png", logo600);
    await saveFile("assets/logo/logo.png", logo600);

    // macOS 菜单栏模板图标 — 黑色圆角方块 + 镂空箭头（保留 logo 识别度）
    const tray36 = await renderMacTrayTemplate(36);
    await saveFile("assets/logo/tray_iconTemplate.png", tray36);
    const tray18 = await renderMacTrayTemplate(18);
    await saveFile("assets/logo/tray_iconTemplate@1x.png", tray18);

    // 内置备选应用图标「闪电」— 512px，运行时由 AppIconService 转 ICO
    if (existsSync(BOLT_SVG_SRC)) {
      const bolt = await renderPngFrom(BOLT_SVG_SRC, 512);
      await saveFile("assets/logo/rinadown_bolt.png", bolt);
      totalCount += 1;
    }

    totalCount += 4;
  }

  // ──────────────────────────────────────────
  // 2. Windows ICO — 多分辨率
  // ──────────────────────────────────────────
  console.log("\n📁 windows/runner/resources/");
  {
    const icoSizes = [16, 32, 48, 64, 256];
    const frames: { size: number; data: Buffer }[] = [];
    for (const size of icoSizes) {
      const data = await getCachedPng(size);
      frames.push({ size, data });
    }
    const ico = buildIco(frames);
    await saveFile("windows/runner/resources/app_icon.ico", ico);
    console.log(
      `     (包含分辨率: ${icoSizes.map((s) => `${s}×${s}`).join(", ")})`,
    );
    totalCount += 1;
  }

  // ──────────────────────────────────────────
  // 2b. Windows 托盘图标 — 深/浅色共用同一彩色 logo
  //     整块栅格图在浅/深任务栏均可辨识，无需区分主题
  // ──────────────────────────────────────────
  console.log("\n📁 windows/runner/resources/ (tray icons)");
  {
    const traySizes = [16, 32];
    const trayFrames: { size: number; data: Buffer }[] = [];
    for (const size of traySizes) {
      trayFrames.push({ size, data: await getCachedPng(size) });
    }
    const trayIco = buildIco(trayFrames);
    // 两文件保持相同内容，避免改动 Rust 端在不同主题切换图标的现有逻辑
    await saveFile("windows/runner/resources/tray_win_dark.ico", trayIco);
    await saveFile("windows/runner/resources/tray_win_light.ico", trayIco);

    console.log(`     (含分辨率: ${traySizes.map((s) => `${s}×${s}`).join(", ")})`);
    totalCount += 2;
  }

  // ──────────────────────────────────────────
  // 3. macOS AppIcon — 7 个尺寸
  // ──────────────────────────────────────────
  console.log("\n📁 macos/Runner/Assets.xcassets/AppIcon.appiconset/");
  {
    const macSizes = [16, 32, 64, 128, 256, 512, 1024];
    for (const size of macSizes) {
      const buf = await renderMacAppIcon(size);
      await saveFile(
        `macos/Runner/Assets.xcassets/AppIcon.appiconset/app_icon_${size}.png`,
        buf,
      );
    }
    totalCount += macSizes.length;
  }

  // ──────────────────────────────────────────
  // 4. iOS AppIcon — 15 个文件
  // ──────────────────────────────────────────
  console.log("\n📁 ios/Runner/Assets.xcassets/AppIcon.appiconset/");
  {
    // { 文件名: 实际像素尺寸 }
    const iosIcons: Record<string, number> = {
      "Icon-App-20x20@1x.png": 20,
      "Icon-App-20x20@2x.png": 40,
      "Icon-App-20x20@3x.png": 60,
      "Icon-App-29x29@1x.png": 29,
      "Icon-App-29x29@2x.png": 58,
      "Icon-App-29x29@3x.png": 87,
      "Icon-App-40x40@1x.png": 40,
      "Icon-App-40x40@2x.png": 80,
      "Icon-App-40x40@3x.png": 120,
      "Icon-App-60x60@2x.png": 120,
      "Icon-App-60x60@3x.png": 180,
      "Icon-App-76x76@1x.png": 76,
      "Icon-App-76x76@2x.png": 152,
      "Icon-App-83.5x83.5@2x.png": 167,
      "Icon-App-1024x1024@1x.png": 1024,
    };
    for (const [filename, pixelSize] of Object.entries(iosIcons)) {
      const buf = await getCachedPng(pixelSize);
      await saveFile(
        `ios/Runner/Assets.xcassets/AppIcon.appiconset/${filename}`,
        buf,
      );
    }
    totalCount += Object.keys(iosIcons).length;
  }

  // ──────────────────────────────────────────
  // 5. Android mipmap — 5 个 DPI 变体
  // ──────────────────────────────────────────
  console.log("\n📁 android/app/src/main/res/");
  {
    const androidIcons: Record<string, number> = {
      "mipmap-mdpi": 48,
      "mipmap-hdpi": 72,
      "mipmap-xhdpi": 96,
      "mipmap-xxhdpi": 144,
      "mipmap-xxxhdpi": 192,
    };
    for (const [folder, size] of Object.entries(androidIcons)) {
      const buf = await getCachedPng(size);
      await saveFile(`android/app/src/main/res/${folder}/ic_launcher.png`, buf);
    }
    totalCount += Object.keys(androidIcons).length;
  }

  // ──────────────────────────────────────────
  // 6. 浏览器扩展图标 — 正常 + disabled + logo
  // ──────────────────────────────────────────
  console.log("\n📁 rinaDown/public/icon/");
  {
    const extSizes = [16, 32, 48, 128];

    // 正常图标
    for (const size of extSizes) {
      const buf = await getCachedPng(size);
      await saveFile(`rinaDown/public/icon/${size}.png`, buf);
    }

    // disabled（灰度）图标
    for (const size of extSizes) {
      const buf = await renderDisabledPng(size);
      await saveFile(`rinaDown/public/icon/${size}-disabled.png`, buf);
    }

    // 扩展 logo
    const extLogo = await getCachedPng(128);
    await saveFile("rinaDown/public/icon/rinadown_logo.png", extLogo);

    // 复制 SVG 到扩展
    const svgContent = readFileSync(SVG_SRC);
    await saveFile("rinaDown/public/icon/rinadown_logo.svg", svgContent);

    totalCount += extSizes.length * 2 + 2;
  }

  // ──────────────────────────────────────────
  // 7. 官网 — favicon + logo
  // ──────────────────────────────────────────
  console.log("\n📁 website/public/");
  {
    // favicon.ico（多分辨率: 16, 32, 48）
    const faviconSizes = [16, 32, 48];
    const faviconFrames: { size: number; data: Buffer }[] = [];
    for (const size of faviconSizes) {
      faviconFrames.push({ size, data: await getCachedPng(size) });
    }
    const faviconIco = buildIco(faviconFrames);
    await saveFile("website/public/favicon.ico", faviconIco);

    // favicon.svg — 内嵌 256px PNG。logo 现在是栅格插画，嵌 1024px 会让
    // favicon 涨到 ~1.4MB；favicon 实际最多渲染到 256px，256 足够。
    const faviconPng = await getCachedPng(256);
    const pngBase64 = faviconPng.toString("base64");
    const faviconSvg = [
      `<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" viewBox="0 0 256 256" width="256" height="256">`,
      `  <image href="data:image/png;base64,${pngBase64}" width="256" height="256"/>`,
      `</svg>`,
      "",
    ].join("\n");
    await saveFile("website/public/favicon.svg", faviconSvg);

    // logo.png (1024×1024)
    const logo1024 = await getCachedPng(1024);
    await saveFile("website/public/logo.png", logo1024);

    // logo.svg (复制源 SVG)
    const svgContent = readFileSync(SVG_SRC);
    await saveFile("website/public/logo.svg", svgContent);

    totalCount += 4;
  }

  // ──────────────────────────────────────────
  // 8. web/public/ — 服务器内嵌 SPA（Vite React）的 logo
  // ──────────────────────────────────────────
  console.log("\n📁 web/public/");
  {
    const webLogo = await getCachedPng(256);
    await saveFile("web/public/logo.png", webLogo);

    // favicon.svg — 内嵌 PNG（与 website 同构，避免再维护一份矢量源）
    const pngBase64 = webLogo.toString("base64");
    const faviconSvg = [
      `<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" viewBox="0 0 256 256" width="256" height="256">`,
      `  <image href="data:image/png;base64,${pngBase64}" width="256" height="256"/>`,
      `</svg>`,
      "",
    ].join("\n");
    await saveFile("web/public/favicon.svg", faviconSvg);

    totalCount += 2;
  }

  // ──────────────────────────────────────────
  // 9. installer/windows/ — 安装向导右上角小图（安装包 logo = 应用图标）
  // ──────────────────────────────────────────
  console.log("\n📁 installer/windows/");
  {
    const wizardBmp = await buildBmp(55, await getCachedPng(55));
    await saveFile("installer/windows/wizard_small.bmp", wizardBmp);
    totalCount += 1;
  }

  // ──────────────────────────────────────────
  // 完成
  // ──────────────────────────────────────────
  console.log(`\n✅ 完成！共生成 ${totalCount} 个文件。`);
  console.log("   重新构建各平台应用后图标即可更新。");
}

main().catch((err) => {
  console.error("❌ 生成失败:", err);
  process.exit(1);
});
