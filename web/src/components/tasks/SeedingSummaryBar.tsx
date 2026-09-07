// 做种汇总条：statusTab=seeding 时列表顶部的一条紧凑水平摘要（对齐桌面 SeedingSummaryBar，
// 见 lib/src/widgets/task_list.dart）。遍历全量任务：活跃做种计数+速度+累计上传；
// 排队做种计数+累计上传（不计速度）。

import { ArrowUp } from 'lucide-react'
import { fmtBytes, fmtSpeed } from '../../lib/format'
import { useI18n } from '../../lib/i18n'
import { useViewTasks } from './useViewTasks'

export function SeedingSummaryBar() {
  const { t } = useI18n()
  const tasks = useViewTasks()

  let active = 0
  let queued = 0
  let uploadSpeed = 0
  let uploadedTotal = 0
  for (const task of tasks) {
    if (task.status !== 3) continue
    if (task.seedingStatus === 1) {
      active++
      uploadSpeed += task.uploadSpeed
      uploadedTotal += task.uploadedBytes ?? 0
    } else if (task.seedingStatus === 8) {
      queued++
      uploadedTotal += task.uploadedBytes ?? 0
    }
  }

  return (
    <div className="flex items-center gap-3 px-1 pt-2 pb-1 text-[11.5px] text-text3 whitespace-nowrap">
      <span className="font-semibold">{t('seeding.summaryActive', { n: active })}</span>
      {queued > 0 && <span>{t('seeding.summaryQueued', { n: queued })}</span>}
      <span className="flex items-center gap-1 text-success font-semibold">
        <ArrowUp size={12} />
        {fmtSpeed(uploadSpeed)}
      </span>
      <span>
        {t('seeding.uploadedTotal')} {fmtBytes(uploadedTotal)}
      </span>
    </div>
  )
}
