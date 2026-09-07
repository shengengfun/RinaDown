// FluxDown 插件 resolver 入口（classic script，非 ESM；入口挂 globalThis）。
//
// 引擎在每次实际发起下载前、协议判定之前调用 globalThis.resolve(ctx)：
//   - 返回对象 → 用其 url/fileName/... 覆盖后按协议重新分派（HTTP/HLS/BT/…）。
//   - 返回 null/undefined → 放行，按原始 URL 下载。
//
// 可用的宿主桥接 API（全局 flux）：
//   flux.fetch(opts)          -> Promise<{status, headers, body, truncated}>（带 SSRF 守卫）
//   flux.storage.get(key)     -> Promise<string|null>
//   flux.storage.set(key,val) -> Promise<void>
//   flux.settings.<key>       类型化只读设置快照（number/boolean/string）
//   flux.info                 { identity, version, appVersion }
//   flux.logger.info/warn/error(...) 与 console.*  -> 写入 App 日志
globalThis.resolve = async (ctx) => {
  if (flux.settings.verbose) {
    flux.logger.info('[echo] resolve', ctx.url, 'quality=', flux.settings.quality);
  }

  const target = flux.settings.target;
  if (!target) {
    // 未配置目标 → 放行原始 URL。
    return null;
  }

  // 记录一次调用计数（演示 storage）。
  const n = Number(await flux.storage.get('calls')) || 0;
  await flux.storage.set('calls', String(n + 1));

  const result = { url: target };
  if (flux.settings.saveDir) {
    result.fileName = 'echo-download.bin';
  }
  return result;
};
