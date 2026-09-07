// BT 做种纯函数 —— 谓词/分享率/时长与桌面端 lib/src/models/download_task.dart 逐条对齐。
// 状态码语义（引擎 bt_seeding.rs SeedingStopReason::as_i32）：
// 0=none, 1=做种中, 2=分享率达标, 3=时长达标, 4=手动停止, 5=已删除,
// 6=会话释放, 7=不活跃达标, 8=排队做种。

import type { I18nKey } from './i18n'

/** 做种字段最小投影（ViewTask / TaskDto 均满足）。 */
export interface SeedingFields {
  status: number
  seedingStatus?: number
  uploadedBytes?: number
  uploadedAtCompletion?: number
  downloadedBytes: number
  seedingTimeSecs?: number
}

/** 归入「做种中」Tab：completed 且活跃做种(1)或排队(8)。 */
export function isSeeding(t: SeedingFields): boolean {
  const s = t.seedingStatus ?? 0
  return t.status === 3 && (s === 1 || s === 8)
}

/** 可「继续做种」：completed 且因限制/手动/会话释放停止（deleted(5) 不可恢复）。 */
export function isSeedingStopped(t: SeedingFields): boolean {
  const s = t.seedingStatus ?? 0
  return t.status === 3 && (s === 2 || s === 3 || s === 4 || s === 6 || s === 7)
}

/** 总分享率 = 累计上传 / 已下载；分母 <=0 兜底 0（同桌面，不出 Infinity）。 */
export function seedRatio(t: SeedingFields): number {
  return t.downloadedBytes <= 0 ? 0 : (t.uploadedBytes ?? 0) / t.downloadedBytes
}

/** 做种后分享率 =（累计上传 - 完成瞬间基准）/ 已下载。 */
export function postSeedRatio(t: SeedingFields): number {
  return t.downloadedBytes <= 0
    ? 0
    : ((t.uploadedBytes ?? 0) - (t.uploadedAtCompletion ?? 0)) / t.downloadedBytes
}

/**
 * 实时做种秒数：引擎累计值 + 采样锚点插值（仅活跃做种插值；排队/停止只显示累计）。
 * `anchorMs` 为最近一次采纳做种帧的本地时刻（无帧 = undefined，不插值）。
 */
export function liveSeedingTimeSecs(t: SeedingFields, anchorMs?: number, nowMs = Date.now()): number {
  const base = t.seedingTimeSecs ?? 0
  if ((t.seedingStatus ?? 0) === 1 && anchorMs !== undefined) {
    return base + Math.max(0, Math.floor((nowMs - anchorMs) / 1000))
  }
  return base
}

/** 做种状态 → i18n 键（详情「做种状态」行用，全量 0..8）。 */
export function seedingStatusKey(s: number): I18nKey {
  switch (s) {
    case 1: return 'seeding.status.seeding'
    case 2: return 'seeding.status.ratioReached'
    case 3: return 'seeding.status.timeReached'
    case 4: return 'seeding.status.userStopped'
    case 5: return 'seeding.status.deleted'
    case 6: return 'seeding.status.sessionReleased'
    case 7: return 'seeding.status.inactiveReached'
    case 8: return 'seeding.status.queued'
    default: return 'seeding.status.none'
  }
}

/** 做种时长格式：`23s` / `3m05s` / `1h02m03s`（对齐桌面 _formatDuration）。 */
export function fmtSeedDuration(totalSecs: number): string {
  const secs = Math.max(0, Math.floor(totalSecs))
  const h = Math.floor(secs / 3600)
  const m = Math.floor((secs % 3600) / 60)
  const s = secs % 60
  const pad = (n: number) => String(n).padStart(2, '0')
  if (h > 0) return `${h}h${pad(m)}m${pad(s)}s`
  if (m > 0) return `${m}m${pad(s)}s`
  return `${s}s`
}
