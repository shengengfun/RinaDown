// 对话框开关的全局 store（跨组件打开新建下载对话框用）。
// HLS/BT 选择对话框由 ws.ts 的 hlsRequestStore / btRequestStore 驱动，不在此列。

import { Store } from './ws'
import type { ResolvePreviewResponse } from './types'

export const newDownloadOpenStore = new Store<boolean>(false)

export function openNewDownload() {
  newDownloadOpenStore.set(true)
}

/** manifest 前置选择弹窗的触发载荷：resolvePreview 命中多文件清单后，new-download.tsx
 *  连同当前表单已填字段一起交给本弹窗（弹窗确认后据此拼 CreateGroupRequest）。 */
export interface ManifestSelectPayload {
  manifest: ResolvePreviewResponse
  sourceUrl: string
  saveDir: string
  queueId: string
  segments: number
  cookies: string
  userAgent: string
  proxyUrl: string
  extraHeaders: Record<string, string>
}

/** 待处理的 manifest 选择请求（弹窗消费后置 null，对齐 ws.ts 的 btRequestStore 等模式）。 */
export const manifestSelectStore = new Store<ManifestSelectPayload | null>(null)

export function openManifestSelect(payload: ManifestSelectPayload) {
  manifestSelectStore.set(payload)
}

/** 重命名任务弹窗的触发载荷（消费后置 null）。 */
export interface RenameTaskPayload {
  taskId: string
  fileName: string
}

export const renameTaskStore = new Store<RenameTaskPayload | null>(null)

export function openRenameTask(payload: RenameTaskPayload) {
  renameTaskStore.set(payload)
}

/** 做种限制对话框的触发载荷：当前任务的哨兵编码原值（-2/-1/>=0，见 types.ts TaskDto）。 */
export interface SeedLimitsPayload {
  taskId: string
  ratioLimitMilli: number
  postRatioLimitMilli: number
  seedTimeLimitMinutes: number
  inactiveTimeLimitMinutes: number
  uploadLimitBps: number
}

export const seedLimitsStore = new Store<SeedLimitsPayload | null>(null)

export function openSeedLimits(payload: SeedLimitsPayload) {
  seedLimitsStore.set(payload)
}
