// FluxDown 插件：yt-dlp 通用解析 onDone 钩子（classic script，入口挂 globalThis）。
//
// 作用：关闭「优先 MP4 容器」时，下载完成后把非 mp4 产物（通常为 VP9/WebM）
// 用 ffmpeg 转码为 H.264/AAC 的兼容 mp4。站点无关——对任意来源的非 mp4 产物生效。
//
// 通知平面语义：fire-and-forget——失败仅记日志，绝不影响任务状态。仅当：
//   1. 设置「优先 MP4 容器」= 关（preferMp4 === false）；
//   2. 产物不是 .mp4/.m4a（即 webm 等非 mp4 容器）；
//   3. flux.ffmpeg 门面存在（manifest 已声明 permissions:["ffmpeg"] 授权）
//      且 FluxDown 装有 ffmpeg（可在 App「组件」页安装）。
//
// 约束（见 flux.ffmpeg 契约）：ffmpeg 在任务 save_dir 牢笼内执行，参数中的文件
// 一律用相对名（basename，前缀 './' 防以 '-' 开头的文件名被当作选项）；绝对路径
// / URL / '..' 会被拒。VP9 不能 `-c copy` 进 mp4 保持可播，故转码视频为 H.264、
// 音频为 AAC。源文件保留（插件无删除文件的能力），产物为同目录 <name>.mp4，
// 并经 flux.task.recordArtifact 登记——「删除任务并删除文件」时随任务一并删除。

function baseName(p) {
  var i = Math.max(p.lastIndexOf('/'), p.lastIndexOf('\\'));
  return i >= 0 ? p.slice(i + 1) : p;
}

function stripExt(name) {
  var i = name.lastIndexOf('.');
  return i > 0 ? name.slice(0, i) : name;
}

// './' 前缀：确保 ffmpeg 把参数当文件路径而非选项。
function rel(name) {
  return './' + name;
}

globalThis.onDone = async (ctx) => {
  var verbose = flux.settings.verbose;

  // 用户开启「优先 MP4」→ 产物本就是 mp4，无需处理。
  if (flux.settings.preferMp4) return;

  // 门面缺失（未授权，理论上不会——manifest 已声明权限）。
  if (!flux.ffmpeg) {
    if (verbose) flux.logger.warn('[ytdlp] onDone: flux.ffmpeg 门面不可用，跳过');
    return;
  }

  var filePath = ctx.filePath;
  if (!filePath) return;
  var inName = baseName(filePath);

  // 已是 mp4（渐进流）或纯音频 m4a → 无需转换。
  if (/\.(mp4|m4a)$/i.test(inName)) {
    if (verbose) flux.logger.info('[ytdlp] onDone: 已是 mp4/m4a，无需转换:', inName);
    return;
  }

  var avail = await flux.ffmpeg.available();
  if (!avail || !avail.available) {
    flux.logger.warn('[ytdlp] onDone: ffmpeg 未安装（可在「组件」页安装），跳过转 mp4');
    return;
  }

  var outName = stripExt(inName) + '.mp4';
  var args = ['-i', rel(inName)];

  // 引擎 mux 失败（如 AAC 无法封入 webm）时会留独立音频 sidecar，一并合并。
  var audioName = ctx.audioPath ? baseName(ctx.audioPath) : null;
  if (audioName) args.push('-i', rel(audioName));

  args = args.concat([
    '-c:v', 'libx264', '-crf', '20', '-preset', 'veryfast',
    '-c:a', 'aac', '-b:a', '192k',
    '-movflags', '+faststart',
  ]);
  if (audioName) args.push('-map', '0:v:0', '-map', '1:a:0');
  args.push('-y', rel(outName));

  if (verbose) flux.logger.info('[ytdlp] onDone: 转 mp4', inName, '→', outName);

  var started = Date.now();
  var r;
  try {
    r = await flux.ffmpeg.run({ args: args, timeoutMs: 20 * 60 * 1000 });
  } catch (e) {
    flux.logger.error('[ytdlp] onDone: ffmpeg 调用异常:', String(e));
    return;
  }

  if (r.timedOut) {
    flux.logger.error('[ytdlp] onDone: ffmpeg 转码超时:', inName);
    return;
  }
  if (r.code !== 0) {
    flux.logger.error(
      '[ytdlp] onDone: ffmpeg 转码失败 code=' + r.code,
      (r.stderr || '').slice(-400)
    );
    return;
  }

  var secs = ((Date.now() - started) / 1000).toFixed(1);
  flux.logger.info('[ytdlp] onDone: 已转为 mp4:', outName, '(' + secs + 's，源文件保留)');

  // 登记衍生产物：使 App「删除任务并删除文件」时把 mp4 与源 webm 一并删除，
  // 保证单一任务的所有文件成组管理。旧版 FluxDown 无此 API 时静默跳过。
  if (flux.task && flux.task.recordArtifact) {
    try {
      await flux.task.recordArtifact(outName);
    } catch (e) {
      flux.logger.warn('[ytdlp] onDone: recordArtifact 失败（不影响产物）:', String(e));
    }
  }
};
