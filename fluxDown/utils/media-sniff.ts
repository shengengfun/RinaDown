/**
 * 媒体资源嗅探纯函数（无 chrome / wxt / DOM 依赖）。
 *
 * 设计为可在两处复用：
 *   1. Main World 注入脚本 fetch-interceptor.ts（读页面响应体判定清单 / 深扫 JSON 内嵌媒体 URL）
 *   2. Node/Vitest 单元测试（因零浏览器依赖，可直接 import 测试）
 *
 * 归属（License）：
 *   - `scanForMediaUrls` 的「字段级媒体 URL 判定」思路移植自 cat-catch（GPL-3.0）
 *     catch-script/search.js:59-131 (findMedia)；其递归控制（深度 / 节点数 / 字符串长度三重预算）
 *     为 FluxDown 重写，cat-catch 原版仅有深度守卫、无宽度 / 长度预算。
 *   - 其余函数为 FluxDown 自研，检测思路受 cat-catch 启发，不含 cat-catch 代码。
 *   - cat-catch: https://github.com/xifangczy/cat-catch （GPL-3.0）
 *   本文件随 FluxDown 以 AGPL-3.0 分发。
 */

import { isSniffableExtension, isStreamingUrl } from "./resource-types";

/** 仅 http(s)：挡 blob: / data: / 页面 URL 被上报（引擎无法 fetch 非 http 资源）。 */
export function isHttpUrl(url: string): boolean {
  return url.startsWith("http://") || url.startsWith("https://");
}

/**
 * 明确无需嗅探响应体的 Content-Type（图片 / 字体 / 样式 / 脚本）。
 * 用于响应体嗅探的前置闸门，省成本 + 降噪。
 */
export function isSkippableCt(ct: string): boolean {
  const lower = ct.toLowerCase();
  return (
    lower.startsWith("image/") ||
    lower.startsWith("font/") ||
    lower.startsWith("text/css") ||
    lower.startsWith("text/javascript") ||
    lower.startsWith("application/javascript") ||
    lower.startsWith("application/x-javascript")
  );
}

/**
 * 判断响应体前缀是否为 HLS / DASH 清单。
 *
 * HLS 规范要求 `#EXTM3U` 为文件首行 → 用严格前缀判定（比全串扫描更快也更符合规范）。
 * DASH MPD 有 XML 序言（`<?xml ...?>`）→ 用有界（前 300 字符）`<MPD` includes 判定。
 *
 * @param text 响应体（或其前缀切片）
 * @returns 命中的清单哨兵字符串（下游 mapFetchEventType 映射为 ResourceType:"stream"），否则 null
 */
export function sniffManifestMagic(
  text: string,
): "hls-manifest" | "dash-manifest" | null {
  if (!text) return null;
  const head = text.trimStart();
  if (head.slice(0, 7).toUpperCase() === "#EXTM3U") return "hls-manifest";
  if (head.slice(0, 300).toUpperCase().includes("<MPD")) return "dash-manifest";
  return null;
}

/** 响应体是否像 JSON（据此决定是否 JSON.parse 后深扫内嵌媒体 URL）。 */
export function looksLikeJson(contentType: string, text: string): boolean {
  if (contentType && contentType.toLowerCase().includes("json")) return true;
  const first = text.trimStart()[0];
  return first === "{" || first === "[";
}

/** 递归深扫预算 —— cat-catch 原版仅有深度守卫，此处补齐宽度 / 长度预算防扁平大数组 / 大字段炸开销。 */
const MAX_SCAN_DEPTH = 20; // 沿用 cat-catch search.js:98 已验证常量
const MAX_SCAN_NODES = 5000; // 节点访问上限（宽度守卫，cat-catch 无）
const MAX_STRING_LEN = 4096; // 超长字符串跳过语义判定（内联 base64 / HTML / bundle）

/** 一个绝对 URL 是否指向可下载媒体：清单（.m3u8/.mpd/manifest/playlist）或可嗅探媒体扩展名（排除 image/other/magnet）。 */
function isMediaUrl(absUrl: string): boolean {
  return isStreamingUrl(absUrl) || isSniffableExtension(absUrl);
}

/**
 * 递归扫描已解析的 JSON 对象，提取内嵌的媒体 URL（相对 URL 用 baseUrl 绝对化 + 去重）。
 *
 * 三重预算：深度 ≤ MAX_SCAN_DEPTH、节点访问 ≤ MAX_SCAN_NODES、字符串 > MAX_STRING_LEN 直接跳过。
 * 全函数 + 每个 `new URL` 均包 try/catch，异常绝不冒泡（不干扰页面）。
 * 绝不附带任何凭证：只返回 URL 字符串；cookies/headers 一律留给下载时按目标 URL 现查
 * （延续 background 现状「检测时不挂凭证」，杜绝跨域凭证泄露）。
 *
 * @param root    JSON.parse 的结果（或 responseType==="json" 的 xhr.response）
 * @param baseUrl 该响应的绝对 URL（用于解析内嵌相对路径）
 * @returns 去重后的绝对媒体 URL 列表
 */
export function scanForMediaUrls(root: unknown, baseUrl: string): string[] {
  const found = new Set<string>();
  let nodes = 0;

  const visit = (value: unknown, depth: number): void => {
    if (depth > MAX_SCAN_DEPTH) return;
    if (++nodes > MAX_SCAN_NODES) return;

    if (typeof value === "string") {
      if (value.length > MAX_STRING_LEN) return;
      // 廉价预筛 + 假阳性闸门：不含路径分隔符的字符串一律跳过。
      // 裸文件名（如 JSON API 里的 "name":"App-1.0.apk"）虽能被 new URL
      // 相对解析成 baseUrl 同目录下的地址，但该地址从未被页面引用过，纯属
      // 捏造（官网 /api/release 的 assets[].name 曾被解析成不存在的
      // /api/<file> 并 404）。合法的 JSON 内嵌媒体引用要么是绝对 URL、
      // 要么带路径段（"/live/i.m3u8"、"hls/x.mpd"），必含 "/"。
      if (!value.includes("/")) return;
      try {
        const abs = new URL(value, baseUrl).href;
        if (isHttpUrl(abs) && isMediaUrl(abs)) found.add(abs);
      } catch {
        /* 非 URL 字符串，忽略 */
      }
      return;
    }

    if (Array.isArray(value)) {
      for (const item of value) {
        if (nodes > MAX_SCAN_NODES) return;
        visit(item, depth + 1);
      }
      return;
    }

    if (value && typeof value === "object") {
      for (const key in value as Record<string, unknown>) {
        if (nodes > MAX_SCAN_NODES) return;
        visit((value as Record<string, unknown>)[key], depth + 1);
      }
    }
  };

  try {
    visit(root, 0);
  } catch {
    /* 深扫异常绝不冒泡 */
  }
  return Array.from(found);
}
