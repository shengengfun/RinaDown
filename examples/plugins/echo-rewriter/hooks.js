// FluxDown 插件通知钩子入口（classic script；入口挂 globalThis）。
//
// 全部钩子均为 fire-and-forget：抛错/超时只记日志，绝不影响任务状态。
// 仅 onError 内可调 flux.task.requestRetry({delayMs}) 命令式请求重试
// （受 App 的 max_auto_retries 上限约束）；其他事件调用会被忽略并记 warn。

globalThis.onStart = async (ctx) => {
  flux.logger.info('[echo] task started:', ctx.taskId, ctx.url);
};

globalThis.onDone = async (ctx) => {
  flux.logger.info('[echo] task done:', ctx.taskId, '->', ctx.filePath);
};

globalThis.onError = async (ctx) => {
  flux.logger.warn('[echo] task error:', ctx.taskId, ctx.message);
  // 对可重试错误请求一次延迟重试（2s 后），受 max_auto_retries 约束。
  flux.task.requestRetry({ delayMs: 2000 });
};
