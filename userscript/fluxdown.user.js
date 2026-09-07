// ==UserScript==
// @name              FluxDown 下载接管 / FluxDown Download Capture
// @name:zh-CN        FluxDown 下载接管
// @namespace         https://github.com/zerx-lab/FluxDown
// @version           1.0.0
// @description       拦截浏览器下载与流媒体资源，一键发送到 FluxDown 桌面下载器（通过本地 RPC）。Intercept downloads & media, send them to the FluxDown desktop app via local RPC.
// @description:zh-CN 拦截浏览器下载与流媒体资源（HLS/DASH/视频/音频/压缩包/安装包等），一键发送到 FluxDown 桌面下载器。
// @author            zerx-lab
// @license           MIT
// @match             *://*/*
// @grant             GM_xmlhttpRequest
// @grant             GM_setValue
// @grant             GM_getValue
// @grant             GM_registerMenuCommand
// @grant             GM_unregisterMenuCommand
// @grant             GM_notification
// @grant             GM_addStyle
// @grant             unsafeWindow
// @connect           127.0.0.1
// @connect           localhost
// @run-at            document-start
// @noframes
// @homepageURL       https://github.com/zerx-lab/FluxDown
// @supportURL        https://github.com/zerx-lab/FluxDown/issues
// ==/UserScript==

/*
 * ============================================================================
 * FluxDown 下载接管用户脚本
 * ============================================================================
 *
 * 工作原理
 * --------
 * 油猴脚本运行在页面上下文，无法使用浏览器扩展专属的 chrome.downloads /
 * webRequest / cookies API，只能用 GM_xmlhttpRequest 与本机程序通信。本脚本：
 *
 *   1. 在 DOM 层拦截下载（点击下载链接、a[download]、程序化 .click()、window.open）；
 *   2. 在页面 JS 层 hook fetch / XMLHttpRequest / MediaSource 嗅探流媒体清单
 *      （HLS .m3u8 / DASH .mpd）与 AJAX 加载的可下载资源；
 *   3. 通过 GM_xmlhttpRequest POST 到 FluxDown 的本地 HTTP 接管服务
 *      （默认 http://127.0.0.1:17800/download），由桌面端弹出确认框后下载。
 *
 * 与浏览器扩展的能力差异（务必知悉）
 * ----------------------------------
 *   - 无法拦截「浏览器内核直接发起」的下载（非页面 JS 触发的、点击后直接由
 *     Content-Disposition 触发的下载）——这类请在 FluxDown 浏览器扩展中完成；
 *     本脚本覆盖「页面内可见的下载链接 / 媒体资源」，对大多数站点已足够。
 *   - 只能读取非 httpOnly 的 Cookie（document.cookie）。需要 httpOnly 鉴权的
 *     下载（如部分网盘）建议使用浏览器扩展。
 *
 * 安全
 * ----
 *   - 仅连接 127.0.0.1 / localhost；
 *   - 每个请求携带 X-FluxDown-Client 头（FluxDown 据此拦截恶意网页的跨域伪造请求）；
 *   - 可在菜单里设置 Token（与 FluxDown 设置页一致）做额外鉴权；
 *   - 最终所有下载都会在 FluxDown 弹出确认框，不会静默下载。
 *
 * 配置（点击油猴菜单 → FluxDown ...）
 * -----------------------------------
 *   - 接管开关 / 端口 / Token / 媒体嗅探面板 / 测试连接 / 下载本页全部链接
 * ============================================================================
 */

(function () {
  'use strict';

  // 避免在同一页重复注入（某些管理器可能重复执行）。
  const W = typeof unsafeWindow !== 'undefined' ? unsafeWindow : window;
  if (W.__fluxdown_userscript__) return;
  W.__fluxdown_userscript__ = true;

  // ==========================================================================
  // 配置
  // ==========================================================================

  const CFG = {
    get port() { return GM_getValue('port', 17800); },
    set port(v) { GM_setValue('port', v); },
    get token() { return GM_getValue('token', ''); },
    set token(v) { GM_setValue('token', v); },
    get enabled() { return GM_getValue('enabled', true); },
    set enabled(v) { GM_setValue('enabled', v); },
    get sniffer() { return GM_getValue('sniffer', true); },
    set sniffer(v) { GM_setValue('sniffer', v); },
    // 点击拦截：按住该修饰键点击则放行给浏览器（默认 Alt）。
    get bypassKey() { return GM_getValue('bypassKey', 'alt'); },
  };

  function base() { return `http://127.0.0.1:${CFG.port}`; }

  // ==========================================================================
  // 可下载资源识别
  // ==========================================================================

  // 点击拦截的目标扩展名（视频/音频/压缩包/安装包/文档/镜像/种子等大文件）。
  const DOWNLOADABLE_EXTS = new Set([
    // 视频
    'mp4', 'mkv', 'avi', 'mov', 'wmv', 'flv', 'webm', 'm4v', 'mpg', 'mpeg', 'rmvb', 'ts',
    // 音频
    'mp3', 'flac', 'aac', 'wav', 'ogg', 'wma', 'ape', 'm4a', 'opus',
    // 压缩包
    'zip', 'rar', '7z', 'tar', 'gz', 'bz2', 'xz', 'zst', 'tgz', 'cab',
    // 安装包/可执行
    'exe', 'msi', 'dmg', 'pkg', 'deb', 'rpm', 'appimage', 'apk', 'xapk',
    // 文档
    'pdf', 'doc', 'docx', 'xls', 'xlsx', 'ppt', 'pptx', 'epub', 'csv',
    // 镜像
    'iso', 'img', 'vmdk',
    // 其它大文件
    'bin', 'dat', 'torrent',
  ]);

  // 明确排除的网页资源扩展名（避免误拦截）。
  const EXCLUDE_EXTS = new Set([
    'html', 'htm', 'php', 'asp', 'aspx', 'jsp', 'json', 'xml',
    'js', 'mjs', 'css', 'woff', 'woff2', 'eot', 'svg', 'ico', 'map',
  ]);

  // 去重时剥离的「缓存破坏 / 追踪」参数。
  const STRIP_PARAMS = new Set([
    't', 'ts', 'time', 'timestamp', '_', 'rand', 'random', 'nonce',
    'sig', 'signature', 'token', 'expire', 'expires', 'e',
    'utm_source', 'utm_medium', 'utm_campaign', 'utm_term', 'utm_content',
  ]);

  function extOf(url) {
    try {
      const pathname = new URL(url, location.href).pathname;
      const last = pathname.split('/').pop() || '';
      const dot = last.lastIndexOf('.');
      if (dot > 0 && dot < last.length - 1) return last.substring(dot + 1).toLowerCase();
    } catch (_) {
      const m = url.match(/\.([a-zA-Z0-9]{1,10})(?:[?#]|$)/);
      if (m) return m[1].toLowerCase();
    }
    return '';
  }

  function filenameOf(url) {
    try {
      const pathname = new URL(url, location.href).pathname;
      const last = decodeURIComponent(pathname.split('/').pop() || '');
      if (last && /\.[a-zA-Z0-9]{1,10}$/.test(last)) return last;
    } catch (_) { /* */ }
    return '';
  }

  function isStreamingUrl(url) {
    const l = url.toLowerCase();
    return l.includes('.m3u8') || l.includes('.mpd') || l.includes('/manifest') || l.includes('/playlist');
  }

  // 是否为流媒体分片（不单独展示，避免 HLS 分片刷屏）。
  function isStreamSegment(url) {
    const ext = extOf(url);
    if (ext === 'm4s') return true;
    if (ext === 'ts') {
      const l = url.toLowerCase();
      if (l.includes('/seg') || l.includes('/chunk') || l.includes('/fragment') ||
          l.includes('/hls') || l.includes('/ts/') || l.includes('/segments/')) return true;
      if (/[_-]\d{2,}\.ts/i.test(url) || /seg\d+/i.test(url)) return true;
      return false;
    }
    return false;
  }

  function isMediaContentType(ct) {
    const l = ct.toLowerCase();
    return l.startsWith('video/') || l.startsWith('audio/') ||
      l === 'application/vnd.apple.mpegurl' || l === 'application/x-mpegurl' ||
      l === 'application/dash+xml';
  }

  function isDownloadableContentType(ct) {
    if (isMediaContentType(ct)) return true;
    const l = ct.toLowerCase().split(';')[0].trim();
    const set = new Set([
      'application/pdf', 'application/msword', 'application/epub+zip', 'text/csv',
      'application/octet-stream', 'application/x-download', 'application/force-download',
      'application/zip', 'application/x-rar-compressed', 'application/x-7z-compressed',
      'application/gzip', 'application/x-tar', 'application/x-bzip2', 'application/x-xz',
      'application/zstd', 'application/x-msdownload', 'application/x-msi',
      'application/x-apple-diskimage', 'application/vnd.debian.binary-package',
      'application/vnd.android.package-archive', 'application/x-iso9660-image',
      'application/x-bittorrent',
    ]);
    if (set.has(l)) return true;
    if (l.startsWith('application/vnd.openxmlformats-officedocument')) return true;
    if (l.startsWith('application/vnd.ms-')) return true;
    return false;
  }

  function classifyStreamUrl(url) {
    const l = url.toLowerCase();
    if (l.includes('.m3u8')) return 'HLS';
    if (l.includes('.mpd')) return 'DASH';
    return 'stream';
  }

  // 用于去重的归一化 URL。
  function normalizeForDedup(url) {
    try {
      const u = new URL(url, location.href);
      u.hash = '';
      const del = [];
      u.searchParams.forEach((_v, k) => { if (STRIP_PARAMS.has(k.toLowerCase())) del.push(k); });
      for (const k of del) u.searchParams.delete(k);
      u.searchParams.sort();
      return u.toString();
    } catch (_) {
      return url;
    }
  }

  // 链接是否「看起来可下载」（用于点击拦截判断）。
  function looksDownloadable(url) {
    let protocol = '';
    try { protocol = new URL(url, location.href).protocol; } catch (_) { return false; }
    if (protocol === 'magnet:') return true;
    if (protocol !== 'http:' && protocol !== 'https:') return false;
    const ext = extOf(url);
    if (!ext) return false;
    if (EXCLUDE_EXTS.has(ext)) return false;
    return DOWNLOADABLE_EXTS.has(ext);
  }

  // ==========================================================================
  // 传输：发送到 FluxDown 本地服务
  // ==========================================================================

  function gmRequest(opts) {
    return new Promise((resolve, reject) => {
      try {
        GM_xmlhttpRequest({
          method: opts.method || 'GET',
          url: opts.url,
          headers: opts.headers || {},
          data: opts.data,
          timeout: opts.timeout || 8000,
          onload: (r) => resolve(r),
          onerror: (e) => reject(e),
          ontimeout: () => reject(new Error('timeout')),
        });
      } catch (e) {
        reject(e);
      }
    });
  }

  function authHeaders() {
    const h = {
      'Content-Type': 'application/json',
      'X-FluxDown-Client': 'userscript',
    };
    const tk = CFG.token;
    if (tk) h['X-FluxDown-Token'] = tk;
    return h;
  }

  async function ping() {
    try {
      const r = await gmRequest({ method: 'GET', url: `${base()}/ping`, timeout: 1500 });
      return r.status >= 200 && r.status < 300;
    } catch (_) {
      return false;
    }
  }

  // 把单个下载请求发给 FluxDown。返回 true 表示已被接受。
  async function sendDownload(payload) {
    try {
      const r = await gmRequest({
        method: 'POST',
        url: `${base()}/download`,
        headers: authHeaders(),
        data: JSON.stringify(payload),
      });
      const ok = r.status >= 200 && r.status < 300;
      if (!ok) console.warn('[FluxDown] send failed:', r.status, r.responseText);
      return ok;
    } catch (e) {
      console.warn('[FluxDown] send error:', e);
      return false;
    }
  }

  // 批量下载（单次请求，url 由 FluxDown 端按换行拆分，用户只需确认一次）。
  async function sendBatch(urls, shared) {
    if (!urls.length) return false;
    try {
      const r = await gmRequest({
        method: 'POST',
        url: `${base()}/download/batch`,
        headers: authHeaders(),
        data: JSON.stringify({
          urls,
          referrer: (shared && shared.referrer) || location.href,
          cookies: (shared && shared.cookies) || document.cookie || '',
        }),
      });
      return r.status >= 200 && r.status < 300;
    } catch (e) {
      console.warn('[FluxDown] batch error:', e);
      return false;
    }
  }

  // 构造一次下载的标准 payload。
  function buildPayload(url, opts) {
    opts = opts || {};
    return {
      url,
      filename: opts.filename || '',
      referrer: opts.referrer || location.href,
      // 仅能拿到非 httpOnly Cookie；够覆盖多数场景。
      cookies: document.cookie || '',
      fileSize: typeof opts.fileSize === 'number' ? opts.fileSize : undefined,
      mimeType: opts.mimeType || undefined,
    };
  }

  // 发送 + 用户反馈 + 失败回退。
  async function takeover(url, opts) {
    opts = opts || {};
    const ok = await sendDownload(buildPayload(url, opts));
    if (ok) {
      toast(`已发送到 FluxDown：${opts.filename || filenameOf(url) || url}`);
    } else {
      // 失败：确认 App 是否在线，决定是否回退浏览器下载。
      const alive = await ping();
      if (alive) {
        toast('FluxDown 收到请求（响应异常），请查看桌面端', true);
      } else {
        toast('FluxDown 未运行，已交回浏览器下载', true);
        if (opts.allowFallback) browserFallback(url, opts.filename);
      }
    }
    return ok;
  }

  // ==========================================================================
  // 点击 / 程序化下载拦截（DOM 层）
  // ==========================================================================

  // 放行集合：回退浏览器下载时短暂跳过拦截，防止环回。
  const bypassUrls = new Map(); // url -> expiry ts
  function markBypass(url) { bypassUrls.set(url, Date.now() + 15000); }
  function isBypassed(url) {
    const e = bypassUrls.get(url);
    if (!e) return false;
    if (e < Date.now()) { bypassUrls.delete(url); return false; }
    return true;
  }
  setInterval(() => {
    const now = Date.now();
    for (const [u, e] of bypassUrls) if (e < now) bypassUrls.delete(u);
  }, 30000);

  function browserFallback(url, filename) {
    markBypass(url);
    try {
      const a = document.createElement('a');
      a.href = url;
      if (filename) a.download = filename;
      a.setAttribute('data-fluxdown-skip', '1');
      (document.body || document.documentElement).appendChild(a);
      a.click();
      a.remove();
    } catch (e) {
      try { W.location.href = url; } catch (_) { /* */ }
    }
  }

  function bypassPressed(ev) {
    switch (CFG.bypassKey) {
      case 'alt': return ev && ev.altKey;
      case 'ctrl': return ev && (ev.ctrlKey || ev.metaKey);
      case 'shift': return ev && ev.shiftKey;
      default: return false;
    }
  }

  // 用户点击（捕获阶段，先于浏览器默认行为）。
  document.addEventListener('click', function (ev) {
    if (!CFG.enabled) return;
    if (bypassPressed(ev)) return; // 修饰键放行
    const a = ev.target && ev.target.closest
      ? ev.target.closest('a[href]')
      : null;
    if (!a) return;
    if (a.hasAttribute('data-fluxdown-skip')) return;

    const href = a.href;
    if (!href || isBypassed(href)) return;

    const hasDownloadAttr = a.hasAttribute('download');
    if (!hasDownloadAttr && !looksDownloadable(href)) return;

    // 命中：接管。必须在任何 await 之前同步阻断默认行为。
    ev.preventDefault();
    ev.stopPropagation();
    takeover(href, {
      filename: a.getAttribute('download') || '',
      referrer: location.href,
      allowFallback: true,
    });
  }, true);

  // 程序化 .click()（页面 JS 创建隐藏 a 并 .click() 触发下载）。
  try {
    const origClick = W.HTMLAnchorElement.prototype.click;
    W.HTMLAnchorElement.prototype.click = function () {
      try {
        if (CFG.enabled && !this.hasAttribute('data-fluxdown-skip')) {
          const href = this.href;
          if (href && !isBypassed(href) && (this.hasAttribute('download') || looksDownloadable(href))) {
            takeover(href, { filename: this.getAttribute('download') || '', allowFallback: false });
            return; // 拦截，不执行原生 click
          }
        }
      } catch (_) { /* */ }
      return origClick.apply(this, arguments);
    };
  } catch (_) { /* */ }

  // window.open(下载型 URL)。
  try {
    const origOpen = W.open;
    W.open = function (url) {
      try {
        if (CFG.enabled && typeof url === 'string' && url && !isBypassed(url) && looksDownloadable(url)) {
          takeover(url, { allowFallback: false });
          return null;
        }
      } catch (_) { /* */ }
      return origOpen.apply(this, arguments);
    };
  } catch (_) { /* */ }

  // ==========================================================================
  // 媒体嗅探（hook fetch / XHR / MediaSource，document-start 注入）
  // ==========================================================================

  // 嗅探到的资源（去重后）。{ url, kind, contentType, size, ts }
  const sniffed = [];
  const notified = new Set();

  function recordResource(kind, url, contentType, size) {
    if (!url) return;
    try {
      const abs = new URL(url, location.href).href;
      if (!/^https?:/i.test(abs)) return;
      if (isStreamSegment(abs)) return; // 过滤分片
      const key = `${kind}:${normalizeForDedup(abs)}`;
      if (notified.has(key)) return;
      notified.add(key);
      if (notified.size > 800) notified.clear();

      sniffed.unshift({ url: abs, kind, contentType: contentType || '', size: size || 0, ts: Date.now() });
      if (sniffed.length > 60) sniffed.length = 60;
      updateFab();
    } catch (_) { /* */ }
  }

  // ---- hook fetch ----
  try {
    const origFetch = W.fetch;
    if (origFetch) {
      W.fetch = function () {
        const args = arguments;
        let url = '';
        try {
          const a0 = args[0];
          if (typeof a0 === 'string') url = a0;
          else if (a0 instanceof W.Request) url = a0.url;
          else if (a0 instanceof W.URL) url = a0.href;
          else if (a0 && a0.url) url = a0.url;
        } catch (_) { /* */ }

        if (CFG.sniffer && url && isStreamingUrl(url)) {
          recordResource(classifyStreamUrl(url), url);
        }

        return origFetch.apply(this, args).then((resp) => {
          try {
            if (CFG.sniffer && resp) {
              const ct = resp.headers.get('content-type') || '';
              const cl = resp.headers.get('content-length');
              const finalUrl = resp.url || url;
              if (ct && isDownloadableContentType(ct)) {
                recordResource(ct.split('/')[0] || 'file', finalUrl, ct, cl ? parseInt(cl, 10) : 0);
              }
              if (finalUrl && finalUrl !== url && isStreamingUrl(finalUrl)) {
                recordResource(classifyStreamUrl(finalUrl), finalUrl, ct);
              }
            }
          } catch (_) { /* */ }
          return resp;
        });
      };
    }
  } catch (_) { /* */ }

  // ---- hook XHR ----
  try {
    const origXOpen = W.XMLHttpRequest.prototype.open;
    const origXSend = W.XMLHttpRequest.prototype.send;
    W.XMLHttpRequest.prototype.open = function (method, url) {
      try {
        this.__fd_url = typeof url === 'string' ? url : (url && url.href) || '';
        if (CFG.sniffer && this.__fd_url && isStreamingUrl(this.__fd_url)) {
          recordResource(classifyStreamUrl(this.__fd_url), this.__fd_url);
        }
      } catch (_) { /* */ }
      return origXOpen.apply(this, arguments);
    };
    W.XMLHttpRequest.prototype.send = function () {
      try {
        this.addEventListener('load', function () {
          try {
            if (!CFG.sniffer) return;
            const url = this.__fd_url;
            if (!url) return;
            const ct = this.getResponseHeader('content-type') || '';
            const cl = this.getResponseHeader('content-length');
            const responseUrl = this.responseURL || url;
            if (ct && isDownloadableContentType(ct)) {
              recordResource(ct.split('/')[0] || 'file', responseUrl, ct, cl ? parseInt(cl, 10) : 0);
            }
            if (responseUrl !== url && isStreamingUrl(responseUrl)) {
              recordResource(classifyStreamUrl(responseUrl), responseUrl, ct);
            }
          } catch (_) { /* */ }
        });
      } catch (_) { /* */ }
      return origXSend.apply(this, arguments);
    };
  } catch (_) { /* */ }

  // ---- hook MediaSource（辅助信号：触发一次 performance 扫描找清单）----
  try {
    const MS = W.MediaSource;
    if (MS && MS.prototype && MS.prototype.addSourceBuffer) {
      const origASB = MS.prototype.addSourceBuffer;
      MS.prototype.addSourceBuffer = function (mime) {
        try {
          if (CFG.sniffer && mime && (mime.startsWith('video/') || mime.startsWith('audio/'))) {
            scanPerformanceForStreams();
          }
        } catch (_) { /* */ }
        return origASB.apply(this, arguments);
      };
    }
  } catch (_) { /* */ }

  // performance 条目里找已加载的 m3u8/mpd（被动补充）。
  function scanPerformanceForStreams() {
    try {
      const entries = W.performance && W.performance.getEntriesByType
        ? W.performance.getEntriesByType('resource') : [];
      for (const e of entries) {
        if (e && e.name && isStreamingUrl(e.name) && !isStreamSegment(e.name)) {
          recordResource(classifyStreamUrl(e.name), e.name);
        }
      }
    } catch (_) { /* */ }
  }

  // ---- DOM 扫描 + MutationObserver（静态 <video>/<audio>/<source>/<a download>）----
  function scanDom(root) {
    try {
      const nodes = (root || document).querySelectorAll('video[src],audio[src],source[src],a[download][href]');
      for (const n of nodes) {
        const src = n.getAttribute('src') || n.getAttribute('href') || '';
        if (!src || src.startsWith('blob:') || src.startsWith('data:')) continue;
        const abs = new URL(src, location.href).href;
        if (n.tagName === 'A') {
          if (looksDownloadable(abs) || n.hasAttribute('download')) {
            recordResource('file', abs, '', 0);
          }
        } else {
          recordResource(n.tagName.toLowerCase(), abs, '', 0);
        }
      }
    } catch (_) { /* */ }
  }

  function startDomScan() {
    scanDom(document);
    scanPerformanceForStreams();
    try {
      const obs = new MutationObserver((muts) => {
        if (!CFG.sniffer) return;
        for (const m of muts) {
          if (m.type === 'childList') {
            for (const node of m.addedNodes) {
              if (node.nodeType === 1) scanDom(node);
            }
          } else if (m.type === 'attributes' && m.target && m.target.nodeType === 1) {
            scanDom(m.target.parentNode || m.target);
          }
        }
      });
      obs.observe(document.documentElement, {
        childList: true, subtree: true, attributes: true,
        attributeFilter: ['src', 'href'],
      });
    } catch (_) { /* */ }
  }

  // ==========================================================================
  // 悬浮 UI（Shadow DOM 隔离样式）
  // ==========================================================================

  let shadow = null, fabEl = null, panelEl = null, listEl = null, badgeEl = null, toastWrap = null;

  function buildUI() {
    if (shadow) return;
    const host = document.createElement('div');
    host.id = 'fluxdown-userscript-root';
    host.style.cssText = 'all:initial;position:fixed;z-index:2147483647;right:0;bottom:0;';
    (document.body || document.documentElement).appendChild(host);
    shadow = host.attachShadow({ mode: 'open' });

    const style = document.createElement('style');
    style.textContent = `
      :host { all: initial; }
      * { box-sizing: border-box; font-family: -apple-system, "Segoe UI", "Microsoft YaHei", sans-serif; }
      .fab {
        position: fixed; right: 18px; bottom: 18px; width: 46px; height: 46px;
        border-radius: 50%; background: #2563eb; color: #fff; cursor: pointer;
        display: flex; align-items: center; justify-content: center;
        box-shadow: 0 4px 16px rgba(0,0,0,.28); font-size: 20px; user-select: none;
        transition: transform .15s ease, background .15s ease;
      }
      .fab:hover { transform: scale(1.08); background: #1d4ed8; }
      .badge {
        position: absolute; top: -4px; right: -4px; min-width: 18px; height: 18px;
        padding: 0 5px; border-radius: 9px; background: #ef4444; color: #fff;
        font-size: 11px; line-height: 18px; text-align: center; display: none;
      }
      .badge.show { display: block; }
      .panel {
        position: fixed; right: 18px; bottom: 74px; width: 360px; max-height: 60vh;
        background: #fff; color: #111; border-radius: 12px; overflow: hidden;
        box-shadow: 0 8px 32px rgba(0,0,0,.3); display: none; flex-direction: column;
        border: 1px solid rgba(0,0,0,.08);
      }
      .panel.show { display: flex; }
      .hd { padding: 12px 14px; background: #2563eb; color: #fff; font-size: 14px; font-weight: 600;
            display: flex; align-items: center; justify-content: space-between; }
      .hd .acts { display: flex; gap: 8px; }
      .hd button { background: rgba(255,255,255,.18); border: 0; color: #fff; border-radius: 6px;
                   padding: 4px 8px; font-size: 12px; cursor: pointer; }
      .hd button:hover { background: rgba(255,255,255,.32); }
      .list { overflow-y: auto; flex: 1; }
      .empty { padding: 28px 14px; text-align: center; color: #888; font-size: 13px; }
      .item { padding: 10px 14px; border-bottom: 1px solid #f0f0f0; display: flex; gap: 10px; align-items: center; }
      .item:hover { background: #f7f8fa; }
      .ic { flex: 0 0 34px; height: 34px; border-radius: 8px; background: #eef2ff; color: #2563eb;
            display: flex; align-items: center; justify-content: center; font-size: 11px; font-weight: 700; }
      .meta { flex: 1; min-width: 0; }
      .nm { font-size: 13px; color: #111; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
      .sub { font-size: 11px; color: #999; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
      .dl { flex: 0 0 auto; background: #2563eb; color: #fff; border: 0; border-radius: 6px;
            padding: 6px 10px; font-size: 12px; cursor: pointer; }
      .dl:hover { background: #1d4ed8; }
      .ft { padding: 8px 14px; border-top: 1px solid #f0f0f0; display: flex; gap: 8px; }
      .ft button { flex: 1; border: 1px solid #d0d5dd; background: #fff; color: #111; border-radius: 8px;
                   padding: 7px; font-size: 12px; cursor: pointer; }
      .ft button:hover { background: #f2f4f7; }
      .ft .primary { background: #2563eb; color: #fff; border-color: #2563eb; }
      .ft .primary:hover { background: #1d4ed8; }
      .toasts { position: fixed; right: 18px; bottom: 78px; display: flex; flex-direction: column; gap: 8px; align-items: flex-end; }
      .toast { background: #111827; color: #fff; padding: 9px 13px; border-radius: 8px; font-size: 12px;
               max-width: 320px; box-shadow: 0 4px 16px rgba(0,0,0,.25); animation: fadein .2s ease; }
      .toast.warn { background: #b45309; }
      @keyframes fadein { from { opacity: 0; transform: translateY(6px); } to { opacity: 1; transform: none; } }
    `;
    shadow.appendChild(style);

    toastWrap = document.createElement('div');
    toastWrap.className = 'toasts';
    shadow.appendChild(toastWrap);

    fabEl = document.createElement('div');
    fabEl.className = 'fab';
    fabEl.title = 'FluxDown 资源面板';
    fabEl.innerHTML = '⬇<span class="badge"></span>';
    badgeEl = fabEl.querySelector('.badge');
    fabEl.addEventListener('click', togglePanel);
    shadow.appendChild(fabEl);

    panelEl = document.createElement('div');
    panelEl.className = 'panel';
    panelEl.innerHTML = `
      <div class="hd">
        <span>FluxDown 嗅探资源</span>
        <span class="acts">
          <button data-act="refresh">刷新</button>
          <button data-act="clear">清空</button>
          <button data-act="close">×</button>
        </span>
      </div>
      <div class="list"></div>
      <div class="ft">
        <button data-act="all" class="primary">全部发送</button>
        <button data-act="links">本页链接</button>
      </div>`;
    listEl = panelEl.querySelector('.list');
    panelEl.querySelector('[data-act="close"]').addEventListener('click', togglePanel);
    panelEl.querySelector('[data-act="clear"]').addEventListener('click', () => { sniffed.length = 0; notified.clear(); renderList(); updateFab(); });
    panelEl.querySelector('[data-act="refresh"]').addEventListener('click', () => { scanDom(document); scanPerformanceForStreams(); renderList(); });
    panelEl.querySelector('[data-act="all"]').addEventListener('click', sendAllSniffed);
    panelEl.querySelector('[data-act="links"]').addEventListener('click', downloadAllLinks);
    shadow.appendChild(panelEl);
  }

  function updateFab() {
    if (!badgeEl) return;
    const n = sniffed.length;
    if (n > 0) { badgeEl.textContent = n > 99 ? '99+' : String(n); badgeEl.classList.add('show'); }
    else badgeEl.classList.remove('show');
    if (panelEl && panelEl.classList.contains('show')) renderList();
  }

  function fmtSize(b) {
    if (!b || b <= 0) return '';
    const u = ['B', 'KB', 'MB', 'GB']; let i = 0; let n = b;
    while (n >= 1024 && i < u.length - 1) { n /= 1024; i++; }
    return `${n.toFixed(n < 10 && i > 0 ? 1 : 0)}${u[i]}`;
  }

  function renderList() {
    if (!listEl) return;
    if (!sniffed.length) {
      listEl.innerHTML = '<div class="empty">暂无嗅探到的资源<br>播放视频或刷新页面以重新嗅探</div>';
      return;
    }
    listEl.innerHTML = '';
    for (const r of sniffed) {
      const item = document.createElement('div');
      item.className = 'item';
      const label = (r.kind || 'file').toUpperCase().slice(0, 4);
      const name = filenameOf(r.url) || r.url.split('/').pop().split('?')[0] || r.url;
      const sub = [r.contentType, fmtSize(r.size)].filter(Boolean).join(' · ') || r.url;
      item.innerHTML = `
        <div class="ic">${label}</div>
        <div class="meta"><div class="nm"></div><div class="sub"></div></div>
        <button class="dl">下载</button>`;
      item.querySelector('.nm').textContent = name;
      item.querySelector('.sub').textContent = sub;
      item.querySelector('.dl').addEventListener('click', () => {
        takeover(r.url, { fileSize: r.size || undefined, mimeType: r.contentType || undefined, allowFallback: false });
      });
      listEl.appendChild(item);
    }
  }

  function togglePanel() {
    buildUI();
    const showing = panelEl.classList.toggle('show');
    if (showing) { scanDom(document); scanPerformanceForStreams(); renderList(); }
  }

  async function sendAllSniffed() {
    if (!sniffed.length) { toast('没有可发送的资源', true); return; }
    const urls = sniffed.map((r) => r.url);
    const ok = await sendBatch(urls);
    toast(ok ? `已发送 ${urls.length} 个资源到 FluxDown` : '发送失败，请确认 FluxDown 在运行', !ok);
  }

  async function downloadAllLinks() {
    const set = new Set();
    document.querySelectorAll('a[href]').forEach((a) => {
      const href = a.href;
      if (href && (looksDownloadable(href) || a.hasAttribute('download'))) set.add(href);
    });
    const urls = [...set];
    if (!urls.length) { toast('本页未发现可下载链接', true); return; }
    const ok = await sendBatch(urls);
    toast(ok ? `已发送本页 ${urls.length} 个链接到 FluxDown` : '发送失败，请确认 FluxDown 在运行', !ok);
  }

  // 轻量 toast（优先页面内 Shadow DOM，失败回退 GM_notification）。
  function toast(msg, warn) {
    try {
      buildUI();
      const t = document.createElement('div');
      t.className = 'toast' + (warn ? ' warn' : '');
      t.textContent = msg;
      toastWrap.appendChild(t);
      setTimeout(() => { t.style.transition = 'opacity .3s'; t.style.opacity = '0'; setTimeout(() => t.remove(), 300); }, 2600);
    } catch (_) {
      try { GM_notification({ text: msg, title: 'FluxDown', timeout: 3000 }); } catch (__) { /* */ }
    }
  }

  // ==========================================================================
  // 油猴菜单命令
  // ==========================================================================

  let menuIds = [];
  function registerMenu() {
    // 幂等：先注销上一轮的命令（支持 GM_unregisterMenuCommand 的管理器），
    // 避免切换状态时累积重复菜单项。
    if (typeof GM_unregisterMenuCommand === 'function') {
      for (const id of menuIds) { try { GM_unregisterMenuCommand(id); } catch (_) { /* */ } }
    }
    menuIds = [];
    const add = (caption, fn) => {
      try { menuIds.push(GM_registerMenuCommand(caption, fn)); } catch (_) { /* */ }
    };

    add(`${CFG.enabled ? '✅' : '⛔'} 下载接管：${CFG.enabled ? '开' : '关'}（点击切换）`, () => {
      CFG.enabled = !CFG.enabled;
      toast(`下载接管已${CFG.enabled ? '开启' : '关闭'}`);
      registerMenu();
    });
    add(`${CFG.sniffer ? '🎬' : '⛔'} 媒体嗅探：${CFG.sniffer ? '开' : '关'}（点击切换）`, () => {
      CFG.sniffer = !CFG.sniffer;
      toast(`媒体嗅探已${CFG.sniffer ? '开启' : '关闭'}`);
      registerMenu();
    });
    add('📂 显示/隐藏 资源面板', () => togglePanel());
    add('⬇ 下载本页全部链接', () => downloadAllLinks());
    add(`🔌 设置端口（当前 ${CFG.port}）`, () => {
      const p = prompt('FluxDown RPC 端口（与设置页一致，默认 17800）：', String(CFG.port));
      if (p !== null) {
        const n = parseInt(p.trim(), 10);
        if (n >= 1 && n <= 65535) { CFG.port = n; toast(`端口已设为 ${n}`); registerMenu(); }
        else toast('端口无效', true);
      }
    });
    add('🔑 设置 Token（可选）', () => {
      const t = prompt('FluxDown RPC 授权密钥（在 FluxDown 设置页生成，可留空）：', CFG.token);
      if (t !== null) { CFG.token = t.trim(); toast('Token 已保存'); }
    });
    add('🩺 测试连接', async () => {
      toast('正在测试…');
      const ok = await ping();
      toast(ok ? `已连接 FluxDown（${base()}）` : `无法连接 ${base()}，请确认 FluxDown 已启动且 RPC 服务已开启`, !ok);
    });
  }

  // ==========================================================================
  // 启动
  // ==========================================================================

  registerMenu();

  function onReady() {
    buildUI();
    if (CFG.sniffer) startDomScan();
  }

  if (document.readyState === 'loading') {
    document.addEventListener('DOMContentLoaded', onReady, { once: true });
  } else {
    onReady();
  }

  console.log('[FluxDown] userscript loaded; target =', base());
})();
