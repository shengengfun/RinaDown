// RinaDown 插件：yt-dlp 通用视频解析（classic script，入口挂 globalThis）。
//
// 原理：调用 RinaDown 自带的 yt-dlp 组件（flux.ytdlp），以 `-J`（--dump-single-json）
// 提取选定格式的直链，交回引擎做多段并发下载 + 断点续传。yt-dlp 自身负责客户端
// 伪装 / 签名解密 / 格式选择 / 站点适配——覆盖上千个站点（YouTube / Bilibili /
// Twitter / Vimeo / TikTok / Twitch …），本插件只做「解析格式 → 直链回填 +
// 画质档位供用户下载时选择」，不含任何站点专属抓取逻辑。
//
// 平台白名单（重要）：本插件是下载软件的一环，为避免误接管任意链接，仅对一批
// 社区默认、低版权风险的主流平台生效：YouTube / Bilibili（B站）/ Niconico /
// Twitch / Vimeo / Dailymotion / SoundCloud / AcFun（A站）。匹配收口在两处：
// manifest 的 match.urls 白名单 + resolve 内的 detectPlatform 兜底拒绝。命中平台
// 后若 yt-dlp 仍解析不出直链则 fail-closed（抛错，任务进 status=4），绝不把网页
// HTML 当视频存。
//
// 抗 bot 风控（"Sign in to confirm you're not a bot"，主要见于 YouTube，IP 级
// 间歇性风控）：
//   1. 多 player_client 回退：一次调用内令 yt-dlp 轮询 tv/android_vr/ios/web，
//      任一通过即用（比多次进程调用省）。该 extractor-args 仅对 YouTube 生效，
//      对其他站点 yt-dlp 会忽略，无副作用；
//   2. cookie（按平台隔离）：登录态 cookie 是绕过风控 / 下载登录墙内容（如 B站
//      大会员画质、YouTube 会员、Vimeo 私有）的可靠手段。每个需鉴权的平台有独立
//      cookie 设置项，来源优先级：任务级 ctx.cookies > 该平台专属 cookie 设置
//      （detectPlatform → platform.cookieKey）> 该平台匹配 seedHash 的已续期副本。
//      经 `flux.fs.writeFile('cookies.txt', …)` 写入插件工作区（= yt-dlp cwd 同根），
//      以相对名注入 `--cookies cookies.txt`，用完即删（`--cookies-from-browser`
//      被 bridge 拒）。ctx.cookies 为 "k=v; k2=v2" 头格式，本地转 Netscape，cookie
//      域按下载 URL 的主机名自动推导；设置项若已是 Netscape 文件内容则原样透传。
//   3. cookie 自续期：会话 token（如 Google 的 __Secure-*PSIDTS）由浏览器周期
//      轮换、旧值随即作废，静态导出的快照很快失效。而 yt-dlp 每次运行结束会把
//      服务端 Set-Cookie 下发的新 token 重写回 --cookies 文件——本插件在删除前
//      回读该文件，解析成功后经 flux.storage 持久化（cookieRotated:<平台 id>，
//      按平台隔离），下次优先使用，令 cookie 链与浏览器脱钩、自我延续。用户更新
//      设置项（seedHash 变化）时自动重播种；续期 cookie 解析失败时自动作废回退。
//
// 依赖 JS 运行时（仅 YouTube）：yt-dlp 2026 起将 YouTube n-sig 挑战求解外部化
// （EJS），必须有一个 JS 运行时（推荐 Node.js ≥ 22，或 Deno ≥ 2.3）安装在系统
// PATH 中，否则 YouTube 格式直链缺失、只能拿到缩略图（其他站点通常不受影响）。
// 默认用 node；「JS 运行时」设置项可切换。bridge 安全策略只允许裸名（不能填绝对
// 路径），故运行时须在 PATH。
//
// 高级：可经「附加 yt-dlp 参数」设置项追加任意 yt-dlp 参数（空格/引号分隔），
// 直通 yt-dlp 高级能力；RinaDown bridge 仍会拒绝危险开关（--exec 等）。播放列表 URL
// 可经「播放列表条目」设置项选择下载第几条（--playlist-items 单值）。
//
// yt-dlp 组件可在 App「组件」页安装；RinaDown 会自动注入 `--ffmpeg-location`（合并/
// remux 依赖 ffmpeg，插件自带的 --ffmpeg-location 会被 bridge 拒绝）。
//
// 返回值约定（ResolveResult）：
//   url / audioUrl / fileName / totalBytes / extraHeaders / ephemeral / rangeSupported
//   （详见各字段回填处注释）。
//   variants（可选，画质/格式多选项数组；非空时 RinaDown 弹框让用户选择，60s 超时或
//   headless/免打扰场景下自动回退 defaultVariantIndex 指向的档位）。元素字段
//   （camelCase，对应引擎 `plugin::runtime::ResolveVariant`）：
//     label       展示标签，如 "1080p MP4" / "Audio only (m4a)"（必填非空，≤200 字符）
//     url         该档的直链（语义同顶层 url：一次性签名直链，ephemeral）
//     audioUrl    音视频分离场景的配对音频直链；本插件恒显式传值（无需覆盖顶层
//                 时传 ''），避免 RinaDown 收敛逻辑遗留上一档的音频直链
//     fileName    覆盖顶层 fileName（含该档正确的容器扩展名）
//     totalBytes  该档总字节数，未知传 0（引擎侧等价于省略）
//     bandwidth   码率（bps），未知为 0，仅供弹框展示/排序
//     width/height 分辨率（px），未知为 0（纯音频档恒为 0）
//     container   容器/扩展名（如 "mp4"/"webm"/"m4a"），可为空
//   defaultVariantIndex：弹框超时/免打扰/headless 无交互时回退的档位。本实现恒
//   为 0——variants[0] 是「最佳画质」默认档，其字段与顶层 url/audioUrl/fileName/
//   totalBytes 逐一相同，故旧版 RinaDown（无 variants 支持）行为不回退；其后附加
//   各分辨率梯度档（去重）+ 1 个纯音频档，供用户在弹框选择。

// 一次调用内让 yt-dlp 轮询的 player_client 顺序（任一通过即用，仅 YouTube 生效）。
var PLAYER_CLIENTS = 'default,tv,android_vr,ios,web_safari';

// variants 附加的分辨率梯度（从高到低；实际取「不超过该高度的最优档」，源分
// 辨率不足则跳过，非精确匹配）。加上默认档与纯音频档，实际条数远低于上限。
var VARIANT_HEIGHT_TIERS = [2160, 1440, 1080, 720, 480];
// variants 数组条数上限（引擎侧硬上限 50，这里按 UI 可用性收紧）。
var MAX_VARIANTS = 10;

// Windows/Unix 通用的文件名净化。
function sanitizeFileName(name) {
  return (
    (name || '')
      .replace(/[\\/:*?"<>|\u0000-\u001f]/g, ' ')
      .replace(/\s+/g, ' ')
      .trim()
      .slice(0, 120) || 'video'
  );
}

// 从下载 URL 推导 cookie 域（Netscape 文件的 domain 字段）。取主机名，去端口，
// 去开头的 www.，前缀 '.' 表示包含子域。解析失败时退回宽松的空域标记。
function cookieDomainFromUrl(url) {
  var m = /^[a-zA-Z][a-zA-Z0-9+.-]*:\/\/([^/?#]+)/.exec(url || '');
  if (!m) return null;
  var host = m[1];
  var at = host.lastIndexOf('@');
  if (at >= 0) host = host.slice(at + 1); // 去 userinfo
  host = host.replace(/:\d+$/, ''); // 去端口
  // IPv6 字面量（[::1]）或纯 IP：不加子域点，原样用。
  if (/^\[.*\]$/.test(host) || /^\d{1,3}(\.\d{1,3}){3}$/.test(host)) return host;
  host = host.replace(/^www\./i, '');
  if (!host) return null;
  return '.' + host;
}

// yt-dlp 格式选择器：始终取「最佳画质」作为顶层默认档（画质由用户在下载时经
// variants 弹框选择，不在设置里固定）。preferMp4 仅影响容器偏好（H.264/AAC mp4
// 优先 vs 允许 VP9 WebM），不限制画质。免打扰/headless 无弹框时即用此最佳档。
function buildFormat(preferMp4) {
  if (preferMp4) {
    return (
      'bestvideo[ext=mp4]+bestaudio[ext=m4a]/' +
      'bestvideo+bestaudio/' +
      'best'
    );
  }
  return 'bestvideo+bestaudio/best';
}

// 解析「附加 yt-dlp 参数」设置项为 argv 数组：空格分隔，支持单/双引号包裹含空格
// 的值，支持 \" 转义。空串 → 空数组。RinaDown bridge 会对危险开关二次拦截。
function parseExtraArgs(raw) {
  var s = (raw || '').trim();
  if (!s) return [];
  var out = [];
  var cur = '';
  var quote = null;
  var has = false;
  for (var i = 0; i < s.length; i++) {
    var c = s[i];
    if (quote) {
      if (c === '\\' && i + 1 < s.length && (s[i + 1] === quote || s[i + 1] === '\\')) {
        cur += s[++i];
      } else if (c === quote) {
        quote = null;
      } else {
        cur += c;
      }
    } else if (c === '"' || c === "'") {
      quote = c;
      has = true;
    } else if (c === ' ' || c === '\t' || c === '\n' || c === '\r') {
      if (has) { out.push(cur); cur = ''; has = false; }
    } else {
      cur += c;
      has = true;
    }
  }
  if (has) out.push(cur);
  return out;
}

function sizeOf(f) {
  if (!f) return 0;
  var n = Number(f.filesize);
  if (n > 0) return n;
  var a = Number(f.filesize_approx);
  return a > 0 ? a : 0;
}

function extOf(f, info, hasVideo) {
  var e = (f && f.ext) || info.ext || '';
  if (e) return '.' + e;
  return hasVideo ? '.mp4' : '.m4a';
}

// yt-dlp http_headers → extraHeaders（键为标准 HTTP 头名）。
function headersOf(f, info) {
  var h = (f && f.http_headers) || info.http_headers;
  if (!h || typeof h !== 'object') return null;
  var out = {};
  var keys = Object.keys(h);
  for (var i = 0; i < keys.length; i++) {
    var v = h[keys[i]];
    if (v != null) out[keys[i]] = String(v);
  }
  return keys.length ? out : null;
}

// 从完整格式列表（info.formats，不受 -f 选择器影响）中选出最佳纯音频轨
// （vcodec=none 且 acodec!=none）。preferMp4 时优先 m4a（AAC 容器，兼容性更
// 好），其余情形按码率/文件大小取最高。供多个 video-only 变体共享配对音频。
function pickBestAudio(formats, preferMp4) {
  var best = null;
  var bestScore = -1;
  for (var i = 0; i < formats.length; i++) {
    var f = formats[i];
    if (!f || !f.url) continue;
    var hasA = f.acodec && f.acodec !== 'none';
    var hasV = f.vcodec && f.vcodec !== 'none';
    if (!hasA || hasV) continue;
    var score = (Number(f.abr) || Number(f.tbr) || 0) * 1000 + sizeOf(f) / 1e6;
    if (preferMp4 && f.ext === 'm4a') score += 1e9;
    if (score > bestScore) {
      bestScore = score;
      best = f;
    }
  }
  return best;
}

// 从完整格式列表中，为目标高度梯度选出「不超过该高度、越接近越好」的最优视频
// 轨（可能是纯视频轨，也可能是已混流轨）。preferMp4 时同等条件优先 mp4 容器。
function pickVideoAtOrBelow(formats, targetHeight, preferMp4) {
  var best = null;
  var bestScore = -1;
  for (var i = 0; i < formats.length; i++) {
    var f = formats[i];
    if (!f || !f.url) continue;
    var hasV = f.vcodec && f.vcodec !== 'none';
    if (!hasV) continue;
    var h = Number(f.height) || 0;
    if (h <= 0 || h > targetHeight) continue;
    var score = h * 1e6 + (Number(f.tbr) || 0);
    if (preferMp4 && f.ext === 'mp4') score += 1e12;
    if (score > bestScore) {
      bestScore = score;
      best = f;
    }
  }
  return best;
}

// 组装单个 variant 对象（camelCase 字段，契约见文件头注释）。audioUrl /
// fileName / totalBytes 恒显式赋值（未知传 ''/0，而非省略字段），保证 RinaDown
// 收敛逻辑（download_manager.rs::collapse_resolve_variants，仅 Some 才覆盖
// 顶层字段）不会遗留上一次收敛/默认档的字段。
function buildVariant(opts) {
  return {
    label: opts.label,
    url: opts.url,
    audioUrl: opts.audioUrl || '',
    fileName: opts.fileName || '',
    totalBytes: opts.totalBytes || 0,
    bandwidth: opts.bandwidth || 0,
    width: opts.width || 0,
    height: opts.height || 0,
    container: opts.container || '',
  };
}

// 构建 resolve 返回值的 variants + defaultVariantIndex：
//   variants[0]      = singleMeta（「最佳画质」默认档，字段与顶层
//                       url/audioUrl/fileName/totalBytes 一致）
//   variants[1..]     = 从 info.formats 派生的分辨率梯度档（去重、至多 4 个）
//                       + 1 个纯音频档（若默认档本身已是纯音频则跳过，避免重复）
//   defaultVariantIndex 恒为 0（见 variants[0] 说明）。
// info.formats 缺失/为空时仅返回单元素数组（RinaDown 按 variants.len()<=1 跳过弹框）。
function buildVariants(info, preferMp4, base, singleMeta) {
  var list = [
    buildVariant({
      label: singleMeta.label,
      url: singleMeta.url,
      audioUrl: singleMeta.audioUrl,
      fileName: singleMeta.fileName,
      totalBytes: singleMeta.totalBytes,
      bandwidth: singleMeta.bandwidth,
      width: singleMeta.width,
      height: singleMeta.height,
      container: singleMeta.container,
    }),
  ];
  var seenHeights = {};
  if (singleMeta.height) seenHeights[singleMeta.height] = true;

  var formats = Array.isArray(info.formats) ? info.formats : [];
  if (formats.length) {
    var bestAudio = pickBestAudio(formats, preferMp4);

    for (var i = 0; i < VARIANT_HEIGHT_TIERS.length && list.length < MAX_VARIANTS; i++) {
      var vf = pickVideoAtOrBelow(formats, VARIANT_HEIGHT_TIERS[i], preferMp4);
      if (!vf || !vf.height || seenHeights[vf.height]) continue;
      seenHeights[vf.height] = true;
      var hasMuxedAudio = vf.acodec && vf.acodec !== 'none';
      var container = vf.ext || 'mp4';
      var pairAudio = !hasMuxedAudio && bestAudio;
      list.push(buildVariant({
        label: vf.height + 'p ' + container.toUpperCase(),
        url: vf.url,
        audioUrl: pairAudio ? bestAudio.url : '',
        fileName: base + '.' + container,
        totalBytes: sizeOf(vf) + (pairAudio ? sizeOf(bestAudio) : 0),
        bandwidth: Number(vf.tbr) ? Math.round(Number(vf.tbr) * 1000) : 0,
        width: Number(vf.width) || 0,
        height: Number(vf.height),
        container: container,
      }));
    }

    if (bestAudio && !singleMeta.isAudioOnly && list.length < MAX_VARIANTS) {
      var aExt = bestAudio.ext || 'm4a';
      list.push(buildVariant({
        label: 'Audio only (' + aExt + ')',
        url: bestAudio.url,
        audioUrl: '',
        fileName: base + '.' + aExt,
        totalBytes: sizeOf(bestAudio),
        bandwidth: Number(bestAudio.abr) ? Math.round(Number(bestAudio.abr) * 1000) : 0,
        width: 0,
        height: 0,
        container: aExt,
      }));
    }
  }

  return { variants: list, defaultIndex: 0 };
}

// 判定一段文本是否已是 Netscape cookie 文件（原样透传，不转换）。
function looksNetscape(text) {
  var t = text.replace(/^\uFEFF/, '').trimStart();
  if (/^#\s*(Netscape|HTTP Cookie File)/i.test(t)) return true;
  // 无表头但含 TAB 分隔的行（yt-dlp 也接受）。
  return /\t/.test(text);
}

// "k=v; k2=v2" HTTP Cookie 头 → Netscape 文件内容。域按下载 URL 主机名推导
// （cookieDomainFromUrl），推导失败退回 '.' 通配所有域的宽松兜底。会员/登录墙
// 内容需登录 cookie；此转换让浏览器扩展直传的头格式对任意站点可用。
function cookieHeaderToNetscape(header, domain) {
  var dom = domain || '.';
  var lines = ['# Netscape HTTP Cookie File'];
  var parts = header.split(';');
  for (var i = 0; i < parts.length; i++) {
    var kv = parts[i].trim();
    if (!kv) continue;
    var eq = kv.indexOf('=');
    if (eq <= 0) continue;
    var name = kv.slice(0, eq).trim();
    var value = kv.slice(eq + 1).trim();
    if (!name) continue;
    // domain \t includeSubdomains \t path \t secure \t expiry \t name \t value
    lines.push([dom, 'TRUE', '/', 'TRUE', '0', name, value].join('\t'));
  }
  return lines.length > 1 ? lines.join('\n') + '\n' : '';
}

// FNV-1a 32-bit 哈希（hex）——标记 cookie 设置项版本，检测用户是否更新过设置。
function fnv1a(s) {
  var h = 0x811c9dc5;
  for (var i = 0; i < s.length; i++) {
    h = Math.imul(h ^ s.charCodeAt(i), 0x01000193);
  }
  return (h >>> 0).toString(16);
}

// 平台白名单表：hostname 后缀 → { id, cookieKey }。id 用于 storage 隔离与日志；
// cookieKey 指向该平台专属 cookie 设置项（缺省走通用 cookiesGeneric）。顺序不敏感
// （按后缀匹配）。与 manifest 的 match.urls 白名单保持一致。
var PLATFORMS = [
  { id: 'youtube', hosts: ['youtube.com', 'youtu.be'], cookieKey: 'cookiesYoutube' },
  { id: 'bilibili', hosts: ['bilibili.com', 'b23.tv'], cookieKey: 'cookiesBilibili' },
  { id: 'douyin', hosts: ['douyin.com', 'iesdouyin.com'], cookieKey: 'cookiesGeneric' },
  { id: 'x', hosts: ['x.com', 'twitter.com', 't.co'], cookieKey: 'cookiesGeneric' },
  { id: 'niconico', hosts: ['nicovideo.jp', 'nico.ms'], cookieKey: 'cookiesNiconico' },
  { id: 'twitch', hosts: ['twitch.tv'], cookieKey: 'cookiesTwitch' },
  { id: 'vimeo', hosts: ['vimeo.com'], cookieKey: 'cookiesVimeo' },
  { id: 'dailymotion', hosts: ['dailymotion.com', 'dai.ly'], cookieKey: 'cookiesGeneric' },
  { id: 'soundcloud', hosts: ['soundcloud.com'], cookieKey: 'cookiesGeneric' },
  { id: 'acfun', hosts: ['acfun.cn'], cookieKey: 'cookiesGeneric' },
];

// 由下载 URL 主机名判定平台。返回 PLATFORMS 表项，未命中白名单则 null（防御性：
// manifest 已用 match.urls 白名单收口，正常不会走到 resolve；此处兜底拒绝）。
function detectPlatform(url) {
  var m = /^[a-zA-Z][a-zA-Z0-9+.-]*:\/\/([^/?#]+)/.exec(url || '');
  if (!m) return null;
  var host = m[1].replace(/:\d+$/, '').toLowerCase();
  var at = host.lastIndexOf('@');
  if (at >= 0) host = host.slice(at + 1);
  for (var i = 0; i < PLATFORMS.length; i++) {
    var p = PLATFORMS[i];
    for (var j = 0; j < p.hosts.length; j++) {
      var h = p.hosts[j];
      if (host === h || host.endsWith('.' + h)) return p;
    }
  }
  return null;
}

// 组装 cookie 上下文。来源优先级：任务级 ctx.cookies（头格式）> 该平台专属 cookie
// 设置项（platform.cookieKey）> 若匹配 seedHash 的续期副本更新鲜则用它。均空 →
// text=null（不注入 --cookies）。头格式的 cookie 域按 ctx.url 推导。cookie 自续期
// 的 storage key 按平台 id 隔离（cookieRotated:<id> / cookieSeedHash:<id>），避免
// 不同平台的续期 cookie 互相覆盖。返回 { text, rotatable, usedRotated, seedHash,
// platformId }：rotatable = 来源为设置项，成功后可回存续期副本；usedRotated =
// 本次用的是续期副本（失败时须作废）。
async function buildCookieContext(ctx, platform) {
  var domain = cookieDomainFromUrl(ctx.url);
  var pid = platform ? platform.id : 'generic';
  var task = (ctx.cookies || '').trim();
  if (task) {
    return {
      text: cookieHeaderToNetscape(task, domain),
      rotatable: false,
      usedRotated: false,
      seedHash: '',
      platformId: pid,
    };
  }
  var key = (platform && platform.cookieKey) || 'cookiesGeneric';
  var setting = (flux.settings[key] || '').trim();
  if (!setting) {
    return { text: null, rotatable: false, usedRotated: false, seedHash: '', platformId: pid };
  }
  var seed = looksNetscape(setting) ? setting : cookieHeaderToNetscape(setting, domain);
  var seedHash = fnv1a(seed);
  try {
    if ((await flux.storage.get('cookieSeedHash:' + pid)) === seedHash) {
      var rotated = await flux.storage.get('cookieRotated:' + pid);
      if (rotated && looksNetscape(rotated)) {
        return { text: rotated, rotatable: true, usedRotated: true, seedHash: seedHash, platformId: pid };
      }
    }
  } catch (e) {
    // storage 读失败不致命，退回设置项。
  }
  return { text: seed, rotatable: true, usedRotated: false, seedHash: seedHash, platformId: pid };
}

// 从 yt-dlp stderr 提取用户可读的失败原因（友好错误）。
function friendlyError(url, r, cookiesUsed) {
  var stderr = (r.stderr || '').trim();
  // 缺 JS 运行时的典型症状（YouTube）：n-sig 求解失败 / 只有缩略图 / 无 JS runtime。
  if (/n challenge solving failed|Only images are available|No supported JavaScript runtime|nsig extraction failed/i.test(stderr)) {
    return (
      '缺少 JS 运行时，无法解出 YouTube 视频直链。请安装 Node.js（≥ 22）或 Deno（≥ 2.3）并确保其在系统 PATH 中' +
      (flux.settings.jsRuntime && flux.settings.jsRuntime !== 'node'
        ? '（当前设置的运行时为「' + flux.settings.jsRuntime + '」，请确认已安装）'
        : '') +
      '。详见 https://github.com/yt-dlp/yt-dlp/wiki/EJS 。原始信息: ' + stderr.slice(-200)
    );
  }
  if (/confirm you.?re not a bot|Sign in to confirm/i.test(stderr)) {
    return (
      '站点要求验证「你不是机器人」（IP 级风控）。' +
      (cookiesUsed
        ? '当前 cookie 未能通过，请在浏览器登录该站点后重新导出 cookie 填入任务或插件设置。'
        : '请在浏览器登录该站点后导出 cookie，填入新建下载的 Cookie 字段或插件「Cookie」设置项。') +
      ' 原始信息: ' + stderr.slice(-300)
    );
  }
  if (/Unsupported URL|is not a valid URL|No video formats found|Unable to extract/i.test(stderr)) {
    return 'yt-dlp 无法从该页面解析出视频（站点不受支持或页面无可下载媒体）：' + stderr.slice(-300);
  }
  if (/Video unavailable|Private video|members-only|age.?restricted|login required|This video is only available/i.test(stderr)) {
    return '视频不可用（私有/会员/年龄限制/需登录）：' + stderr.slice(-300) + '。受限内容需填入登录 cookie。';
  }
  if (stderr) return 'yt-dlp 解析失败: ' + stderr.slice(-400);
  return 'yt-dlp 未返回可用数据（可能被风控拦截或站点不受支持），请尝试填入登录 cookie';
}

globalThis.resolve = async (ctx) => {
  var verbose = flux.settings.verbose;

  if (!flux.ytdlp) {
    throw new Error('flux.ytdlp 门面不可用（manifest 需声明 permissions:["ytdlp"]）');
  }
  var avail = await flux.ytdlp.available();
  if (!avail || !avail.available) {
    throw new Error('yt-dlp 未安装或不可用，请在 App「组件」页安装 yt-dlp 组件');
  }

  var platform = detectPlatform(ctx.url);
  if (!platform) {
    throw new Error('该链接不在本插件支持的平台白名单内（仅 YouTube / Bilibili / 抖音 / X / Niconico / Twitch / Vimeo / Dailymotion / SoundCloud / AcFun）: ' + ctx.url);
  }
  var fmt = buildFormat(flux.settings.preferMp4);
  var ck = await buildCookieContext(ctx, platform);
  var cookiesText = ck.text;
  var args = [
    '-J',
    '--no-warnings',
    '--extractor-args', 'youtube:player_client=' + PLAYER_CLIENTS,
    '-f', fmt,
  ];
  // 播放列表条目：URL 为播放列表时选下载第几条（--playlist-items 单值）。留空时
  // 显式 --no-playlist，避免 yt-dlp 对含 list= 的单视频 URL 误当整表处理（-J 会
  // 输出 playlist entries、info 结构不同）。设了值则用 --playlist-items <n>。
  var pItem = (flux.settings.playlistItem || '').trim();
  if (pItem) {
    args.push('--playlist-items', pItem);
  } else {
    args.push('--no-playlist');
  }
  // JS 运行时：yt-dlp 2026 起把 YouTube 的 n-sig 挑战求解强制外部化（EJS），
  // 缺运行时则 YouTube 格式 URL 缺失、只剩 storyboard（下不了）。RinaDown 自动注入的
  // --ffmpeg-location 不含 JS 运行时，故须显式指定。bridge 校验器拒绝含盘符/
  // 绝对路径的参数，因此只能传裸名（如 'node'），运行时须在 PATH 中。默认 node，
  // 设置项可切 deno/quickjs 或 none（none = 不注入，靠 nsig 缓存，多数视频会失败）。
  var jsRuntime = (flux.settings.jsRuntime || 'node').trim();
  if (jsRuntime && jsRuntime !== 'none') {
    args.push('--js-runtimes', jsRuntime);
  }
  // cookie 经 flux.fs 物化进插件工作区（= yt-dlp cwd），以相对名注入 --cookies；
  // 用完即删（敏感数据不长驻）。取代旧 spec.cookiesText 字段——通用文件能力。
  if (cookiesText) {
    await flux.fs.writeFile('cookies.txt', cookiesText);
    args.push('--cookies', 'cookies.txt');
  }
  // 附加参数（高级）：追加到命令末尾（URL 之前）。RinaDown bridge 二次拦截危险开关。
  var extra = parseExtraArgs(flux.settings.extraArgs);
  for (var ei = 0; ei < extra.length; ei++) args.push(extra[ei]);
  args.push(ctx.url);

  if (verbose) {
    flux.logger.info(
      '[ytdlp] yt-dlp -f', fmt,
      pItem ? 'item=' + pItem : 'no-playlist',
      extra.length ? 'extra=' + extra.length : 'no-extra',
      cookiesText ? 'with-cookies' : 'no-cookies',
      ctx.url
    );
  }

  var r;
  var rotatedBack = null;
  try {
    r = await flux.ytdlp.run({
      args: args,
      timeoutMs: 40 * 1000,
    });
  } catch (e) {
    throw new Error('yt-dlp 调用异常: ' + String(e));
  } finally {
    if (cookiesText) {
      // yt-dlp 运行结束会把轮换后的新 token 重写回 cookies.txt——删除前回读，
      // 解析成功后持久化（cookie 自续期，见文件头注释第 3 点）。
      if (ck.rotatable) {
        try {
          rotatedBack = await flux.fs.readFile('cookies.txt');
        } catch (e) {
          rotatedBack = null;
        }
      }
      try {
        await flux.fs.remove('cookies.txt');
      } catch (e) {
        // 清理失败不致命（工作区隔离、下次覆盖写）。
      }
    }
  }

  if (r.timedOut) throw new Error('yt-dlp 解析超时（40s）: ' + ctx.url);

  // 关键：yt-dlp 遇 bot 风控时退出码可能为 0 但 stdout 输出 "null"/空。
  // 不能只看 r.code——须校验 stdout 为合法非空对象，否则给出友好错误。
  var raw = (r.stdout || '').trim();
  if (r.code !== 0 || !raw || raw === 'null') {
    // 用续期副本失败 → 作废之，下次回退到设置项重新播种。
    if (ck.usedRotated) {
      try {
        await flux.storage.set('cookieRotated:' + ck.platformId, '');
      } catch (e) {}
    }
    throw new Error(friendlyError(ctx.url, r, !!cookiesText));
  }

  var info;
  try {
    info = JSON.parse(raw);
  } catch (e) {
    throw new Error('yt-dlp 输出非法 JSON: ' + String(e) + ' | ' + raw.slice(0, 200));
  }
  if (!info || typeof info !== 'object') {
    throw new Error(friendlyError(ctx.url, r, !!cookiesText));
  }
  // 播放列表 URL 未指定条目时 yt-dlp 可能返回 playlist（entries 数组）而非单视频。
  // 取第一条 entry 作为解析目标（单任务对单直链契约，不展开整表）。
  if (info._type === 'playlist' && Array.isArray(info.entries) && info.entries.length) {
    var first = info.entries[0];
    if (first && typeof first === 'object') info = first;
  }

  // 解析成功且 cookie 来自设置项 → 持久化 yt-dlp 回写的续期副本。
  if (ck.rotatable && rotatedBack && looksNetscape(rotatedBack)) {
    try {
      await flux.storage.set('cookieRotated:' + ck.platformId, rotatedBack);
      await flux.storage.set('cookieSeedHash:' + ck.platformId, ck.seedHash);
    } catch (e) {
      // 回存失败不致命，仅失去续期加成。
    }
  }

  var title = info.title || info.id || 'video';
  var base = sanitizeFileName(title);
  var preferMp4 = flux.settings.preferMp4;
  // 视频解析窗口选定的规格会以二次 resolver item 回传；把它变成默认索引，
  // 由引擎的静默二次收敛复用同一规格，重试时不会再次弹窗或回退到最佳档。
  var selectedVariantIndex = -1;
  var variantMatch = /^__flux_variant:(\d+)$/.exec(ctx.resolverItem || '');
  if (variantMatch) selectedVariantIndex = Number(variantMatch[1]);
  var reqs = Array.isArray(info.requested_formats) ? info.requested_formats : null;

  // 情形 A：requested_formats（音视频分离或选定单流）。
  if (reqs && reqs.length >= 1) {
    var vf = null;
    var af = null;
    for (var i = 0; i < reqs.length; i++) {
      var f = reqs[i];
      var hasV = f.vcodec && f.vcodec !== 'none';
      var hasA = f.acodec && f.acodec !== 'none';
      if (hasV && !vf) vf = f;
      else if (hasA && !hasV && !af) af = f;
      else if (hasA && !af) af = f;
    }

    if (vf && vf.url) {
      var vFileName = base + extOf(vf, info, true);
      var vContainer = extOf(vf, info, true).slice(1) || 'mp4';
      var vHeight = Number(vf.height) || Number(info.height) || 0;
      var result = {
        url: vf.url,
        fileName: vFileName,
        totalBytes: (sizeOf(vf) + sizeOf(af)) || null,
        extraHeaders: headersOf(vf, info),
        ephemeral: true,
        rangeSupported: true,
      };
      if (af && af.url) result.audioUrl = af.url;
      if (verbose) {
        flux.logger.info(
          '[ytdlp] video', vf.format_id, vf.ext,
          af ? 'audio ' + af.format_id : 'muxed'
        );
      }
      var builtV = buildVariants(info, preferMp4, base, {
        label: (vHeight ? vHeight + 'p ' : '') + vContainer.toUpperCase(),
        url: vf.url,
        audioUrl: (af && af.url) ? af.url : '',
        fileName: vFileName,
        totalBytes: sizeOf(vf) + sizeOf(af),
        bandwidth: Number(vf.tbr) ? Math.round(Number(vf.tbr) * 1000) : 0,
        width: Number(vf.width) || 0,
        height: vHeight,
        container: vContainer,
        isAudioOnly: false,
      });
      result.variants = builtV.variants;
      result.defaultVariantIndex = selectedVariantIndex >= 0
        ? selectedVariantIndex : builtV.defaultIndex;
      return result;
    }
  }

  // 情形 B：单一 muxed 流（顶层 url）。
  if (info.url) {
    var hasVideo = !!(info.vcodec && info.vcodec !== 'none') ||
      Number(info.height) > 0 || Number(info.width) > 0;
    if (verbose) flux.logger.info('[ytdlp] muxed single', info.format_id, info.ext);
    var bFileName = base + extOf(info, info, hasVideo);
    var bContainer = extOf(info, info, hasVideo).slice(1) || (hasVideo ? 'mp4' : 'm4a');
    var bHeight = hasVideo ? (Number(info.height) || 0) : 0;
    var single2 = {
      url: info.url,
      fileName: bFileName,
      totalBytes: sizeOf(info) || null,
      extraHeaders: headersOf(null, info),
      ephemeral: true,
      rangeSupported: true,
    };
    var builtB = buildVariants(info, preferMp4, base, {
      label: hasVideo
        ? (bHeight ? bHeight + 'p ' : '') + bContainer.toUpperCase()
        : 'Audio only (' + bContainer + ')',
      url: info.url,
      audioUrl: '',
      fileName: bFileName,
      totalBytes: sizeOf(info),
      bandwidth: Number(info.tbr) ? Math.round(Number(info.tbr) * 1000) : 0,
      width: hasVideo ? (Number(info.width) || 0) : 0,
      height: bHeight,
      container: bContainer,
      isAudioOnly: !hasVideo,
    });
    single2.variants = builtB.variants;
    single2.defaultVariantIndex = selectedVariantIndex >= 0
      ? selectedVariantIndex : builtB.defaultIndex;
    return single2;
  }

  throw new Error('yt-dlp 未返回可用直链: ' + ctx.url + '（该内容可能无可下载媒体）');
};
