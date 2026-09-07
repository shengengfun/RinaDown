// 新建下载对话框（对齐 design/web #dlg-new）—— 多行 URL 逐条创建任务；保存目录默认取自
// 服务器配置（['config'] 的 default_save_dir），支持 FsPicker 浏览服务器目录；高级选项
// （Cookies/自定义请求头/单任务代理/Checksum）为可折叠面板，行为对齐原型 #advToggle。
//
// 单条 http(s) URL 提交时先尝试 resolvePreview 探测是否为多文件清单（超时/异常/空清单/error
// 均静默回退直接建任务，行为与预解析前完全一致）；命中后交给 manifest-select.tsx 接管
// （本对话框保持打开，manifest 弹窗以嵌套模态盖在上层；确认建组后由 manifest-select.tsx
// 关闭本对话框，取消则回到本表单）。

import { useEffect, useMemo, useRef, useState } from 'react'
import * as Dialog from '@radix-ui/react-dialog'
import { useQuery, useQueryClient } from '@tanstack/react-query'
import { ChevronRight, Plus, X } from 'lucide-react'
import { api } from '../../lib/api'
import { parseCategories, resolveCategorySaveDir, visibleCategories } from '../../lib/categories'
import { CATEGORIES_KEY } from '../../lib/config'
import { cloudApi } from '../../lib/cloud/client'
import { cloudDeviceId, useCloudSession } from '../../lib/cloud/session'
import { deviceLabel } from '../../lib/cloud/deviceLabel'
import { useRemoteTasks } from '../../lib/cloud/useRemoteTasks'
import { linkApi } from '../../lib/link'
import { cn } from '../../lib/cn'
import { queueDisplayName } from '../../lib/format'
import { newDownloadOpenStore, openManifestSelect } from '../../lib/dialogs'
import { useI18n } from '../../lib/i18n'
import { manifestIsPreviewableUrl } from '../../lib/manifest-selection'
import { parseSiteAuthStore, siteKeyFromUrl } from '../../lib/site-auth'
import { UA_PRESETS } from '../../lib/ua-presets'
import { useStore } from '../../lib/ws'
import { FsPicker } from './fs-picker'
import { SelectField } from './select-field'

interface HeaderRow {
  id: number
  name: string
  value: string
}

/** resolvePreview 探测超时（对齐桌面端 90s）：超时后视同无清单，回退直接建任务。 */
const RESOLVE_PREVIEW_TIMEOUT_MS = 90_000

/** 与超时定时器赛跑；超时 resolve 为 timeout 标记，不影响仍在途的底层请求。 */
function raceTimeout<T>(promise: Promise<T>, ms: number): Promise<T> {
  return new Promise((resolve, reject) => {
    const timer = setTimeout(() => reject(new Error('timeout')), ms)
    promise.then(
      (v) => {
        clearTimeout(timer)
        resolve(v)
      },
      (err) => {
        clearTimeout(timer)
        reject(err)
      },
    )
  })
}

interface FormState {
  urls: string
  fileName: string
  segments: string
  saveDir: string
  saveDirTouched: boolean
  queueId: string
  /** 队列是否被用户改过；未改过时跟随 config 的 `default_queue_id`。 */
  queueIdTouched: boolean
  userAgent: string
  cookies: string
  headers: HeaderRow[]
  proxyUrl: string
  checksum: string
  httpUser: string
  httpPassword: string
  saveSiteAuth: boolean
  advOpen: boolean
  /** 下发目标：空串 = 本机（默认，走现有本地引擎路径）；`cloud:<deviceId>` = FluxCloud
   *  云中转已登录设备；`link:<fingerprint>` = 局域网直连(link)已配对设备。见下方
   *  showDeviceSelect/deviceOptions（两类来源互斥，提交时按前缀还原）。 */
  deviceId: string
}

function emptyForm(saveDir = ''): FormState {
  return {
    urls: '',
    fileName: '',
    segments: '0',
    saveDir,
    saveDirTouched: false,
    queueId: '',
    queueIdTouched: false,
    userAgent: '',
    cookies: '',
    headers: [],
    proxyUrl: '',
    checksum: '',
    httpUser: '',
    httpPassword: '',
    saveSiteAuth: false,
    advOpen: false,
    deviceId: '',
  }
}

export function NewDownloadDialog() {
  const open = useStore(newDownloadOpenStore)
  const queryClient = useQueryClient()
  const [form, setForm] = useState<FormState>(() => emptyForm())
  const [lineErrors, setLineErrors] = useState<Record<number, string>>({})
  const [submitting, setSubmitting] = useState(false)
  const headerRowSeq = useRef(0)
  // HTTP 认证自动回填：authAutofill 记录最近一次自动写入的值（区分自动/手动），
  // authDirty 在用户手动编辑用户名/密码后置位，此后不再覆盖用户输入。
  const authAutofill = useRef<{ user: string; pass: string } | null>(null)
  const authDirty = useRef(false)
  const { t } = useI18n()
  const segmentOptions = [
    { value: '0', label: t('newDl.segmentsAuto') },
    { value: '1', label: t('newDl.segmentsN', { n: 1 }) },
    { value: '2', label: t('newDl.segmentsN', { n: 2 }) },
    { value: '4', label: t('newDl.segmentsN', { n: 4 }) },
    { value: '8', label: t('newDl.segmentsN', { n: 8 }) },
    { value: '16', label: t('newDl.segmentsN', { n: 16 }) },
    { value: '32', label: t('newDl.segmentsN', { n: 32 }) },
  ]
  const userAgentOptions = [
    { value: '', label: t('newDl.globalDefault') },
    ...UA_PRESETS.map((p) => ({ value: p.value, label: p.label })),
  ]

  const { data: config } = useQuery({ queryKey: ['config'], queryFn: api.getConfig, enabled: open })
  const { data: queues } = useQuery({ queryKey: ['queues'], queryFn: api.listQueues, enabled: open })
  const { data: stats } = useQuery({ queryKey: ['stats'], queryFn: api.stats, enabled: open })
  const session = useCloudSession()
  const { onlineDeviceIds } = useRemoteTasks()
  const { data: cloudDevices = [] } = useQuery({
    queryKey: ['cloud', 'devices'],
    queryFn: () => cloudApi.devices().then((r) => r.devices),
    enabled: open && session.status === 'authenticated',
    staleTime: 10_000,
  })
  // 展示只列远端（不含本机），但 deviceLabel 判重名基准用 cloudDevices（含本机）——
  // 见 deviceLabel.ts 的硬约定。
  const remoteDevices = cloudDevices.filter((d) => d.deviceId !== cloudDeviceId())
  const showCloudDevices = session.status === 'authenticated' && remoteDevices.length > 0
  // 本地设备(link)分组：局域网直连已配对设备，与云中转完全独立；宿主未启用/不支持时
  // 该请求失败（见 lib/link.ts isLinkUnsupportedError），静默按空列表处理，分组不出现。
  const { data: linkDevices = [] } = useQuery({
    queryKey: ['link', 'devices'],
    queryFn: () => linkApi.devices().then((r) => r.devices),
    enabled: open,
    staleTime: 10_000,
    retry: false,
  })
  const showLinkDevices = linkDevices.length > 0
  const showDeviceSelect = showCloudDevices || showLinkDevices
  // 离线设备保留在列表里：服务端支持离线下发，对端上线后会 catch-up 领取，
  // 因此只做视觉标注而不禁用。在线判定取「服务端快照 ∪ SSE 实时 presence」，
  // 后者能在设备刚上/下线、devices 查询尚未刷新时立即反映。
  const withOffline = (label: string, online: boolean) => (online ? label : `${label} (${t('link.offline')})`)
  const deviceOptions = [
    { value: '', label: t('cloud.deviceCurrent') },
    ...(showCloudDevices
      ? remoteDevices.map((d) => ({
          value: `cloud:${d.deviceId}`,
          label: withOffline(deviceLabel(d, cloudDevices), (d.isOnline ?? false) || onlineDeviceIds.has(d.deviceId)),
          group: t('newDl.deviceGroupCloud'),
        }))
      : []),
    ...(showLinkDevices
      ? linkDevices.map((d) => ({
          value: `link:${d.fingerprint}`,
          label: withOffline(d.name || '-', d.online),
          group: t('newDl.deviceGroupDirect'),
        }))
      : []),
  ]
  // 选中目标是离线云设备时给出提示：默认只把 "(Offline)" 追加进选项标签，
  // 提交是静默的，用户容易以为下发失败或没反应，见下方渲染处的提示行。
  const selectedCloudDeviceId = form.deviceId.startsWith('cloud:') ? form.deviceId.slice('cloud:'.length) : ''
  const selectedCloudDevice = selectedCloudDeviceId ? remoteDevices.find((d) => d.deviceId === selectedCloudDeviceId) : undefined
  const selectedTargetOffline =
    selectedCloudDevice !== undefined && !((selectedCloudDevice.isOnline ?? false) || onlineDeviceIds.has(selectedCloudDevice.deviceId))
  const demoMode = stats?.demoMode ?? false
  const demoUrl = stats?.demoUrl ?? ''

  // 每次打开都是一张新表单。
  useEffect(() => {
    if (open) {
      setForm(emptyForm())
      setLineErrors({})
      authAutofill.current = null
      authDirty.current = false
    }
  }, [open])

  // 演示模式：URL 锁定为服务器指定的演示文件（服务端 demo_guard 同样强制校验，
  // 这里只是避免用户输入注定被拒绝的链接）。
  useEffect(() => {
    if (open && demoMode && demoUrl) {
      setForm((f) => (f.urls === demoUrl ? f : { ...f, urls: demoUrl }))
      setLineErrors({})
    }
  }, [open, demoMode, demoUrl])

  // 生效的默认保存目录：开启「跟随上次保存位置」且已有记录时用 last_save_dir，
  // 否则用 default_save_dir（对齐桌面 SettingsProvider.effectiveDefaultSaveDir）。
  const rememberLastSaveDir = (config?.remember_last_save_dir ?? 'false') === 'true'
  const lastSaveDir = config?.last_save_dir ?? ''
  const effectiveSaveDir =
    rememberLastSaveDir && lastSaveDir !== '' ? lastSaveDir : (config?.default_save_dir ?? '')

  // 默认队列：未手动改过时跟随 config 的 default_queue_id（空 = 主队列）。
  useEffect(() => {
    const queueId = config?.default_queue_id
    if (open && !form.queueIdTouched && queueId) {
      setForm((f) => ({ ...f, queueId }))
    }
  }, [open, config, form.queueIdTouched])

  const urlLines = useMemo(
    () =>
      form.urls
        .split('\n')
        .map((l) => l.trim())
        .filter(Boolean),
    [form.urls],
  )
  const isSingle = urlLines.length <= 1

  // 分类保存目录：未手动改过保存目录时，按首条链接（或已填的文件名）匹配分类的
  // 「分类保存目录」，命中就用它，否则退回全局默认目录 —— 与桌面
  // NewDownloadDialog._tryAutoApplySaveDir 同语义。默认状态下没有任何分类设了
  // 目录，解析恒为空串，行为与本功能上线前完全一致。
  const categories = useMemo(
    () => visibleCategories(parseCategories(config?.[CATEGORIES_KEY])),
    [config],
  )
  const autoSaveDir =
    resolveCategorySaveDir(form.fileName.trim(), urlLines[0] ?? '', categories) || effectiveSaveDir

  // 保存目录默认值来自服务器配置；一旦用户手动改过就不再被自动值覆盖。
  useEffect(() => {
    if (open && !form.saveDirTouched && autoSaveDir) {
      setForm((f) => ({ ...f, saveDir: autoSaveDir }))
    }
  }, [open, autoSaveDir, form.saveDirTouched])

  // 站点凭据自动回填：单条 http/https URL 且认证区可见时，按站点键查已保存凭据回填
  // 用户名/密码；仅当字段为空或仍是上次自动值时才回填/更新，切到无凭据站点则清空。
  // 手动改过（authDirty）后完全放手。回填不拨动「为此网站保存」开关。
  const siteAuthStore = useMemo(() => parseSiteAuthStore(config?.site_auth_credentials), [config])
  const singleUrl = urlLines.length === 1 ? urlLines[0] : ''
  useEffect(() => {
    if (!open) return
    setForm((f) => {
      if (!f.advOpen || authDirty.current) return f
      const auto = authAutofill.current
      const userIsAuto = f.httpUser === '' || (auto != null && f.httpUser === auto.user)
      const passIsAuto = f.httpPassword === '' || (auto != null && f.httpPassword === auto.pass)
      if (!userIsAuto || !passIsAuto) return f
      const key = singleUrl ? siteKeyFromUrl(singleUrl) : null
      const cred = key ? siteAuthStore[key] : undefined
      if (cred) {
        authAutofill.current = { user: cred.user, pass: cred.pass }
        if (f.httpUser === cred.user && f.httpPassword === cred.pass) return f
        return { ...f, httpUser: cred.user, httpPassword: cred.pass }
      }
      if (auto != null && (f.httpUser !== '' || f.httpPassword !== '')) {
        authAutofill.current = null
        return { ...f, httpUser: '', httpPassword: '' }
      }
      return f
    })
  }, [open, singleUrl, siteAuthStore, form.advOpen])

  function set<K extends keyof FormState>(key: K, value: FormState[K]) {
    setForm((f) => ({ ...f, [key]: value }))
  }

  function addHeaderRow() {
    headerRowSeq.current += 1
    const row: HeaderRow = { id: headerRowSeq.current, name: '', value: '' }
    setForm((f) => ({ ...f, headers: [...f.headers, row] }))
  }

  function removeHeaderRow(id: number) {
    setForm((f) => ({ ...f, headers: f.headers.filter((h) => h.id !== id) }))
  }

  function updateHeaderRow(id: number, key: 'name' | 'value', value: string) {
    setForm((f) => ({ ...f, headers: f.headers.map((h) => (h.id === id ? { ...h, [key]: value } : h)) }))
  }

  function close() {
    newDownloadOpenStore.set(false)
  }

  async function handleSubmit(startPaused = false) {
    if (urlLines.length === 0 || submitting) return
    setSubmitting(true)
    // 「跟随上次保存位置」开启时记录本次实际使用的目录，供下次新建回填
    // （对齐桌面 SettingsProvider.recordLastSaveDir；开关关闭时完全不写）。
    const usedSaveDir = form.saveDir.trim()
    if (rememberLastSaveDir && usedSaveDir !== '' && usedSaveDir !== lastSaveDir) {
      void api
        .putConfig({ last_save_dir: usedSaveDir })
        .then(() => queryClient.invalidateQueries({ queryKey: ['config'] }))
        .catch((err: unknown) => console.warn('[last_save_dir] persist failed', err))
    }
    const headerEntries: Record<string, string> = {}
    for (const h of form.headers) {
      const name = h.name.trim()
      if (name) headerEntries[name] = h.value
    }
    const headers = Object.keys(headerEntries).length > 0 ? headerEntries : undefined
    // 队列归属挂在动作上：立即下载 = 表单所选；稍后下载 = 固定进
    // 「稍后下载」队列（事后可在详情面板移动队列）。
    const queueId = startPaused ? 'later' : form.queueId || undefined

    // 单条 http(s) 链接：先探测是否为多文件清单（90s 超时；异常/空清单/error 均静默回退到
    // 下方直接建任务路径，行为与预解析前完全一致）。命中后本对话框保持打开，manifest 弹窗
    // 以嵌套模态接管；确认建组后由 manifest-select.tsx 关闭本对话框，取消则回到本表单。
    if (!form.deviceId && isSingle && !demoMode && manifestIsPreviewableUrl(urlLines[0])) {
      const url = urlLines[0]
      try {
        const preview = await raceTimeout(
          api.resolvePreview({
            url,
            cookies: form.cookies.trim() || undefined,
            userAgent: form.userAgent || undefined,
            extraHeaders: headers,
          }),
          RESOLVE_PREVIEW_TIMEOUT_MS,
        )
        if (preview.error) console.warn('[resolvePreview]', preview.error)
        if (!preview.error && preview.items.length > 0) {
          setSubmitting(false)
          openManifestSelect({
            manifest: preview,
            sourceUrl: url,
            saveDir: form.saveDir.trim(),
            queueId: form.queueId,
            segments: Number(form.segments),
            cookies: form.cookies.trim(),
            userAgent: form.userAgent,
            proxyUrl: form.proxyUrl.trim(),
            extraHeaders: headerEntries,
          })
          return
        }
      } catch (err) {
        console.warn('[resolvePreview] failed, falling back to direct create', err)
      }
    }

    const nextErrors: Record<number, string> = {}
    let anyOk = false
    const linkFingerprint = form.deviceId.startsWith('link:') ? form.deviceId.slice('link:'.length) : ''
    const cloudTarget = form.deviceId.startsWith('cloud:') ? form.deviceId.slice('cloud:'.length) : ''
    for (let i = 0; i < urlLines.length; i++) {
      try {
        if (linkFingerprint) {
          await linkApi.dispatchTask(linkFingerprint, {
            url: urlLines[i],
            saveDir: form.saveDir.trim() || undefined,
            fileName: isSingle ? form.fileName.trim() || undefined : undefined,
          })
        } else if (cloudTarget) {
          await cloudApi.dispatchTask({
            toDevice: cloudTarget,
            url: urlLines[i],
            saveDir: form.saveDir.trim() || undefined,
            fileName: isSingle ? form.fileName.trim() || undefined : undefined,
          })
        } else {
          await api.createTask({
            url: urlLines[i],
            fileName: isSingle ? form.fileName.trim() || undefined : undefined,
            saveDir: form.saveDir.trim() || undefined,
            segments: Number(form.segments),
            cookies: form.cookies.trim() || undefined,
            headers,
            proxyUrl: form.proxyUrl.trim() || undefined,
            userAgent: form.userAgent || undefined,
            queueId,
            checksum: form.checksum.trim() || undefined,
            httpUser: form.httpUser.trim() || undefined,
            httpPassword: form.httpUser.trim() ? form.httpPassword : undefined,
            saveSiteAuth: form.httpUser.trim() && form.saveSiteAuth ? true : undefined,
            startPaused: startPaused || undefined,
          })
        }
        anyOk = true
      } catch (err) {
        nextErrors[i] = err instanceof Error ? err.message : String(err)
      }
    }
    setSubmitting(false)
    setLineErrors(nextErrors)
    if (anyOk) void queryClient.invalidateQueries({ queryKey: ['tasks'] })
    if (Object.keys(nextErrors).length === 0) close()
  }

  return (
    <Dialog.Root
      open={open}
      onOpenChange={(o) => {
        if (!o) close()
      }}
    >
      <Dialog.Portal>
        <Dialog.Overlay className="wbackdrop show" />
        <Dialog.Content
          asChild
          onPointerDownOutside={(e) => {
            // 表单对话框：点击外部不关闭（防误触丢失已填内容）。同时根治 Radix Select-in-Dialog
            // 的已知问题——Select 展开时 body 为 pointer-events:none，点击对话框内元素会命中
            // 遮罩层，被误判为 outside 而连带关掉整个弹窗（且其派发延迟于 Select 卸载，无法
            // 靠 DOM 探测区分）。关闭路径：✕ / 取消 / Esc。
            e.preventDefault()
          }}
        >
          <form
            className="dialog show"
            onSubmit={(e) => {
              e.preventDefault()
              void handleSubmit()
            }}
          >
            <header className="dlg-head">
              <Dialog.Title asChild>
                <b>{t('newDl.title')}</b>
              </Dialog.Title>
              <Dialog.Close asChild>
                <button type="button" className="icon-btn sm" aria-label={t('common.close')}>
                  <X size={16} />
                </button>
              </Dialog.Close>
            </header>
            <Dialog.Description className="sr-only">{t('newDl.desc')}</Dialog.Description>
            <div className="dlg-body">
              <label className="field-label" htmlFor="nd-urls">
                {demoMode ? t('newDl.urlLabelDemo') : t('newDl.urlLabel')}
              </label>
              <textarea
                id="nd-urls"
                className="text-input area"
                rows={3}
                spellCheck={false}
                readOnly={demoMode}
                aria-readonly={demoMode}
                value={form.urls}
                onChange={(e) => {
                  if (demoMode) return
                  set('urls', e.target.value)
                  setLineErrors({})
                }}
              />
              {demoMode && (
                <p className="mt-1 text-xs break-all text-text3">
                  {t('newDl.demoHint')}
                </p>
              )}
              <div className="grid2">
                <div>
                  <label className="field-label" htmlFor="nd-filename">
                    {t('newDl.fileName')}
                  </label>
                  <input
                    id="nd-filename"
                    className="text-input"
                    type="text"
                    placeholder={t('newDl.fileNamePlaceholder')}
                    disabled={!isSingle}
                    value={form.fileName}
                    onChange={(e) => set('fileName', e.target.value)}
                  />
                </div>
                <div>
                  <label className="field-label">{t('newDl.segments')}</label>
                  <SelectField value={form.segments} onChange={(v) => set('segments', v)} options={segmentOptions} ariaLabel={t('newDl.segments')} />
                </div>
              </div>
              <label className="field-label" htmlFor="nd-savedir">
                {t('newDl.saveDir')}
              </label>
              <div className="dir-row">
                <input
                  id="nd-savedir"
                  className="text-input"
                  type="text"
                  spellCheck={false}
                  value={form.saveDir}
                  onChange={(e) => setForm((f) => ({ ...f, saveDir: e.target.value, saveDirTouched: true }))}
                />
                <FsPicker value={form.saveDir} onChange={(p) => setForm((f) => ({ ...f, saveDir: p, saveDirTouched: true }))} />
              </div>
              {showDeviceSelect && (
                <>
                  <label className="field-label">{t('newDl.dispatchTo')}</label>
                  <SelectField value={form.deviceId} onChange={(v) => set('deviceId', v)} options={deviceOptions} ariaLabel={t('newDl.dispatchTo')} />
                  {selectedTargetOffline && <p className="mt-1 text-xs text-text3">{t('newDl.offlineTargetHint')}</p>}
                </>
              )}
              <div className="grid2">
                <div>
                  <label className="field-label">{t('newDl.queue')}</label>
                  <SelectField
                    value={form.queueId}
                    onChange={(v) => setForm((f) => ({ ...f, queueId: v, queueIdTouched: true }))}
                    options={[
                      { value: '', label: t('newDl.defaultQueue') },
                      ...(queues ?? []).map((q) => ({
                        value: q.queueId,
                        label: queueDisplayName(q),
                      })),
                    ]}
                    ariaLabel={t('newDl.queue')}
                  />
                </div>
                <div>
                  <label className="field-label">{t('newDl.userAgent')}</label>
                  <SelectField value={form.userAgent} onChange={(v) => set('userAgent', v)} options={userAgentOptions} ariaLabel={t('newDl.userAgent')} />
                </div>
              </div>
              <button type="button" className={cn('adv-toggle', form.advOpen && 'open')} onClick={() => set('advOpen', !form.advOpen)}>
                <ChevronRight size={13} />
                {t('newDl.advanced')}
              </button>
              <div className={cn('adv-panel', form.advOpen && 'open')}>
                <label className="field-label">{t('newDl.httpAuth')}</label>
                <div className="grid2">
                  <input
                    className="text-input"
                    type="text"
                    spellCheck={false}
                    autoComplete="off"
                    placeholder={t('newDl.httpAuthUser')}
                    aria-label={t('newDl.httpAuthUser')}
                    value={form.httpUser}
                    onChange={(e) => {
                      authDirty.current = true
                      set('httpUser', e.target.value)
                    }}
                  />
                  <input
                    className="text-input"
                    type="password"
                    autoComplete="new-password"
                    placeholder={t('newDl.httpAuthPassword')}
                    aria-label={t('newDl.httpAuthPassword')}
                    value={form.httpPassword}
                    onChange={(e) => {
                      authDirty.current = true
                      set('httpPassword', e.target.value)
                    }}
                  />
                </div>
                <label className="mcheck mt-2">
                  <input
                    type="checkbox"
                    checked={form.saveSiteAuth}
                    onChange={(e) => set('saveSiteAuth', e.target.checked)}
                  />
                  <i />
                  {t('newDl.httpAuthSave')}
                </label>
                <p className="mt-1 text-xs text-text3">{t('newDl.httpAuthHint')}</p>
                <label className="field-label" htmlFor="nd-cookies">
                  {t('newDl.cookies')}
                </label>
                <input
                  id="nd-cookies"
                  className="text-input"
                  type="text"
                  placeholder="key=value; key2=value2"
                  value={form.cookies}
                  onChange={(e) => set('cookies', e.target.value)}
                />
                <label className="field-label">{t('newDl.headers')}</label>
                <div className="flex flex-col gap-2">
                  {form.headers.map((h) => (
                    <div key={h.id} className="flex items-center gap-2">
                      <input
                        className="text-input flex-1"
                        type="text"
                        spellCheck={false}
                        placeholder={t('newDl.headerName')}
                        value={h.name}
                        onChange={(e) => updateHeaderRow(h.id, 'name', e.target.value)}
                      />
                      <input
                        className="text-input flex-1"
                        type="text"
                        spellCheck={false}
                        placeholder={t('newDl.headerValue')}
                        value={h.value}
                        onChange={(e) => updateHeaderRow(h.id, 'value', e.target.value)}
                      />
                      <button
                        type="button"
                        className="icon-btn sm shrink-0"
                        aria-label={t('common.delete')}
                        onClick={() => removeHeaderRow(h.id)}
                      >
                        <X size={14} />
                      </button>
                    </div>
                  ))}
                  <button type="button" className="btn ghost sm self-start" onClick={addHeaderRow}>
                    <Plus size={13} />
                    {t('newDl.headersAdd')}
                  </button>
                </div>
                <label className="field-label" htmlFor="nd-proxy">
                  {t('newDl.proxy')}
                </label>
                <input
                  id="nd-proxy"
                  className="text-input"
                  type="text"
                  placeholder="socks5://127.0.0.1:1080"
                  value={form.proxyUrl}
                  onChange={(e) => set('proxyUrl', e.target.value)}
                />
                <label className="field-label" htmlFor="nd-checksum">
                  {t('newDl.checksum')}
                </label>
                <input
                  id="nd-checksum"
                  className="text-input"
                  type="text"
                  placeholder={t('newDl.checksumPlaceholder')}
                  value={form.checksum}
                  onChange={(e) => set('checksum', e.target.value)}
                />
              </div>
              {Object.keys(lineErrors).length > 0 && (
                <div className="mt-3 flex flex-col gap-1">
                  {urlLines.map(
                    (line, i) =>
                      lineErrors[i] && (
                        <p key={i} className="text-xs break-all text-danger">
                          {t('newDl.lineError', { n: i + 1, line, error: lineErrors[i] })}
                        </p>
                      ),
                  )}
                </div>
              )}
            </div>
            <footer className="dlg-foot">
              <Dialog.Close asChild>
                <button type="button" className="btn ghost">
                  {t('common.cancel')}
                </button>
              </Dialog.Close>
              <button
                type="button"
                className="btn ghost"
                disabled={urlLines.length === 0 || submitting}
                onClick={() => void handleSubmit(true)}
              >
                {t('newDl.later')}
              </button>
              <button type="submit" className="btn primary" disabled={urlLines.length === 0 || submitting}>
                {submitting ? t('newDl.creating') : t('newDl.create')}
              </button>
            </footer>
          </form>
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  )
}
