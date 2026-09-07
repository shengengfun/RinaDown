// 合成视图任务：REST TaskDto（['tasks'] 缓存）叠加 live 值（liveStore），
// live 优先于 REST（status/downloadedBytes/totalBytes/errorMessage/做种字段），
// 并补上 REST 没有的 speed/uploadSpeed/seedingTimeAt。

import { useQuery } from '@tanstack/react-query'
import { api } from '../../lib/api'
import { liveStore, useStore } from '../../lib/ws'
import type { TaskDto } from '../../lib/types'

export interface ViewTask extends TaskDto {
  speed: number
  /** BT 做种上传速率 B/s（无 live 帧 = 0）。 */
  uploadSpeed: number
  /** 最近一次做种帧到达的本地时刻（做种时长插值锚点；无 = 不插值）。 */
  seedingTimeAt?: number
}

export function useViewTasks(): ViewTask[] {
  const { data } = useQuery({ queryKey: ['tasks'], queryFn: api.listTasks })
  const live = useStore(liveStore)
  const tasks = data ?? []
  return tasks.map((t): ViewTask => {
    const l = live[t.taskId]
    if (!l) return { ...t, speed: 0, uploadSpeed: 0 }
    const seedingFrame = l.seedingStatus === 1 || l.seedingStatus === 8
    return {
      ...t,
      status: l.status,
      downloadedBytes: l.downloadedBytes,
      totalBytes: l.totalBytes || t.totalBytes,
      errorMessage: l.errorMessage,
      speed: l.speed,
      uploadSpeed: l.uploadSpeed ?? 0,
      uploadedBytes: l.uploadedBytes ?? t.uploadedBytes,
      seedingStatus: l.seedingStatus ?? t.seedingStatus,
      seedingMessage: l.seedingMessage ?? t.seedingMessage,
      // 下载期帧的 seedingTimeSecs 恒 0，采纳会把累计清零 —— 仅做种/排队帧采纳。
      seedingTimeSecs: seedingFrame ? (l.seedingTimeSecs ?? t.seedingTimeSecs) : t.seedingTimeSecs,
      seedingTimeAt: seedingFrame ? l.at : undefined,
    }
  })
}
