// 任务级做种限制三态对话框 —— 由 seedLimitsStore（详情面板铅笔按钮触发）驱动开关。
// 存储编码与桌面 _SeedLimitsDialog 对齐：四维限制共用哨兵 -2=跟随全局 / -1=不限制 /
// >=0=自定义（分享率千分比、时长分钟，0 等效不限制）；seedUploadLimitBps 独立（0=不限，
// UI 以 KB/s 编辑 ×1024）。确认后经 WS setTaskSeedLimits 下发并乐观更新 ['tasks'] 缓存。

import { useEffect, useState } from 'react'
import * as Dialog from '@radix-ui/react-dialog'
import { useQueryClient } from '@tanstack/react-query'
import { X } from 'lucide-react'
import { seedLimitsStore } from '../../lib/dialogs'
import { useI18n } from '../../lib/i18n'
import type { TaskDto } from '../../lib/types'
import { sendWs, useStore } from '../../lib/ws'
import { SelectField } from './select-field'

type SeedLimitsMode = 'global' | 'unlimited' | 'custom'

/** 勾选：round(小数×1000) 千分比；非法/≤0 → 0（等效不限制）；未勾选 → -1。 */
function encodeRatio(on: boolean, s: string): number {
  if (!on) return -1
  const v = Number.parseFloat(s)
  return Number.isFinite(v) && v > 0 ? Math.round(v * 1000) : 0
}

/** 勾选：整数分钟；非法/≤0 → 0；未勾选 → -1。 */
function encodeMinutes(on: boolean, s: string): number {
  if (!on) return -1
  const v = Number.parseInt(s, 10)
  return Number.isFinite(v) && v > 0 ? v : 0
}

/** Checkbox + 数字输入的一行自定义限制（未勾选时输入禁用）。 */
function LimitRow({
  label,
  on,
  onToggle,
  value,
  onValue,
  step,
  suffix,
}: {
  label: string
  on: boolean
  onToggle: (on: boolean) => void
  value: string
  onValue: (v: string) => void
  step: string
  suffix?: string
}) {
  return (
    <div className="mt-2 flex items-center gap-2">
      <label className="mcheck min-w-0 flex-1">
        <input type="checkbox" checked={on} onChange={(e) => onToggle(e.target.checked)} />
        <i />
        <span className="truncate">{label}</span>
      </label>
      {/* 行内 style 定宽：design.css 的 .text-input{width:100%} 不在 cascade layer 里,
          会压过 Tailwind 工具类(layered),w-24 之类在这里不生效。 */}
      <input
        className="text-input shrink-0"
        style={{ width: 96 }}
        type="number"
        min="0"
        step={step}
        value={value}
        disabled={!on}
        onChange={(e) => onValue(e.target.value)}
        aria-label={label}
      />
      {suffix ? <span className="shrink-0 text-xs text-text3">{suffix}</span> : null}
    </div>
  )
}

export function SeedLimitsDialog() {
  const { t } = useI18n()
  const payload = useStore(seedLimitsStore)
  const open = payload !== null
  const qc = useQueryClient()

  const [mode, setMode] = useState<SeedLimitsMode>('global')
  const [ratioOn, setRatioOn] = useState(false)
  const [ratio, setRatio] = useState('1.0')
  const [postOn, setPostOn] = useState(false)
  const [post, setPost] = useState('1.0')
  const [timeOn, setTimeOn] = useState(false)
  const [time, setTime] = useState('4320')
  const [inactiveOn, setInactiveOn] = useState(false)
  const [inactive, setInactive] = useState('30')
  const [uploadOn, setUploadOn] = useState(false)
  const [uploadKb, setUploadKb] = useState('512')

  // 每次新请求到达时按当前任务字段回填：模式三态判定 + 各行 enabled/值（千分比 ÷1000 展示）。
  useEffect(() => {
    if (!payload) return
    const values = [payload.ratioLimitMilli, payload.postRatioLimitMilli, payload.seedTimeLimitMinutes, payload.inactiveTimeLimitMinutes]
    setMode(values.every((v) => v === -2) ? 'global' : values.every((v) => v === -1) ? 'unlimited' : 'custom')
    setRatioOn(payload.ratioLimitMilli >= 0)
    setRatio(payload.ratioLimitMilli >= 0 ? String(payload.ratioLimitMilli / 1000) : '1.0')
    setPostOn(payload.postRatioLimitMilli >= 0)
    setPost(payload.postRatioLimitMilli >= 0 ? String(payload.postRatioLimitMilli / 1000) : '1.0')
    setTimeOn(payload.seedTimeLimitMinutes >= 0)
    setTime(payload.seedTimeLimitMinutes >= 0 ? String(payload.seedTimeLimitMinutes) : '4320')
    setInactiveOn(payload.inactiveTimeLimitMinutes >= 0)
    setInactive(payload.inactiveTimeLimitMinutes >= 0 ? String(payload.inactiveTimeLimitMinutes) : '30')
    setUploadOn(payload.uploadLimitBps > 0)
    setUploadKb(payload.uploadLimitBps > 0 ? String(Math.round(payload.uploadLimitBps / 1024)) : '512')
  }, [payload])

  function cancel() {
    seedLimitsStore.set(null)
  }

  function confirm() {
    if (!payload) return
    const [ratioLimitMilli, postRatioLimitMilli, seedTimeLimitMinutes, inactiveTimeLimitMinutes] =
      mode === 'global'
        ? [-2, -2, -2, -2]
        : mode === 'unlimited'
          ? [-1, -1, -1, -1]
          : [encodeRatio(ratioOn, ratio), encodeRatio(postOn, post), encodeMinutes(timeOn, time), encodeMinutes(inactiveOn, inactive)]
    const kbps = Number.parseInt(uploadKb, 10)
    const uploadLimitBps = uploadOn && Number.isFinite(kbps) && kbps > 0 ? kbps * 1024 : 0
    sendWs({
      type: 'setTaskSeedLimits',
      taskId: payload.taskId,
      ratioLimitMilli,
      postRatioLimitMilli,
      seedTimeLimitMinutes,
      inactiveTimeLimitMinutes,
      uploadLimitBps,
    })
    // 乐观更新任务缓存的五个哨兵字段（引擎稍后经 WS 推帧兜底纠偏）。
    qc.setQueryData<TaskDto[]>(['tasks'], (prev) =>
      prev?.map((task) =>
        task.taskId === payload.taskId
          ? {
              ...task,
              seedRatioLimitMilli: ratioLimitMilli,
              seedPostRatioLimitMilli: postRatioLimitMilli,
              seedTimeLimitMinutes,
              seedInactiveTimeLimitMinutes: inactiveTimeLimitMinutes,
              seedUploadLimitBps: uploadLimitBps,
            }
          : task,
      ),
    )
    seedLimitsStore.set(null)
  }

  return (
    <Dialog.Root
      open={open}
      onOpenChange={(o) => {
        if (!o) cancel()
      }}
    >
      <Dialog.Portal>
        <Dialog.Overlay className="wbackdrop show" />
        <Dialog.Content className="dialog sm show" style={{ width: 'min(440px, calc(100vw - 20px))' }} aria-describedby={undefined}>
          <header className="dlg-head">
            <Dialog.Title asChild>
              <b>{t('detail.seedLimits')}</b>
            </Dialog.Title>
            <Dialog.Close asChild>
              <button type="button" className="icon-btn sm" aria-label={t('common.close')}>
                <X size={16} />
              </button>
            </Dialog.Close>
          </header>
          <form
            onSubmit={(e) => {
              e.preventDefault()
              confirm()
            }}
          >
            <div className="dlg-body">
              <SelectField
                value={mode}
                onChange={(v) => setMode(v as SeedLimitsMode)}
                ariaLabel={t('detail.seedLimits')}
                options={[
                  { value: 'global', label: t('detail.seedLimitsGlobal') },
                  { value: 'unlimited', label: t('detail.seedLimitsUnlimited') },
                  { value: 'custom', label: t('detail.seedLimitsCustom') },
                ]}
              />
              {mode === 'custom' ? (
                <>
                  <LimitRow label={t('set.bt.seedRatioLimit')} on={ratioOn} onToggle={setRatioOn} value={ratio} onValue={setRatio} step="0.1" />
                  <LimitRow label={t('set.bt.seedPostRatioLimit')} on={postOn} onToggle={setPostOn} value={post} onValue={setPost} step="0.1" />
                  <LimitRow
                    label={t('set.bt.seedTimeLimit')}
                    on={timeOn}
                    onToggle={setTimeOn}
                    value={time}
                    onValue={setTime}
                    step="1"
                    suffix={t('set.bt.timeUnitMinutes')}
                  />
                  <LimitRow
                    label={t('set.bt.seedInactiveTimeLimit')}
                    on={inactiveOn}
                    onToggle={setInactiveOn}
                    value={inactive}
                    onValue={setInactive}
                    step="1"
                    suffix={t('set.bt.timeUnitMinutes')}
                  />
                </>
              ) : null}
              <LimitRow
                label={t('detail.seedUploadLimit')}
                on={uploadOn}
                onToggle={setUploadOn}
                value={uploadKb}
                onValue={setUploadKb}
                step="1"
                suffix="KB/s"
              />
              <p className="mt-2 text-xs text-text3">{t('detail.seedUploadLimitHint')}</p>
            </div>
            <footer className="dlg-foot">
              <Dialog.Close asChild>
                <button type="button" className="btn ghost">
                  {t('common.cancel')}
                </button>
              </Dialog.Close>
              <button type="submit" className="btn primary">
                {t('common.confirm')}
              </button>
            </footer>
          </form>
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  )
}
