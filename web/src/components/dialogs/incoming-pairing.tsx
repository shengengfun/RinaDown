// 入站配对核验对话框（本机作为**被添加方**）—— 由 incomingPairingStore（WS
// linkIncomingPairing）驱动开关，是 SAS 双端核对的另一半。
//
// 为什么必须有这个弹窗：配对协议的 SAS 设计意图是「双端肉眼核对，中间人会与两端各自
// 协商出不同 z → 两端 SAS 不一致 → 用户看得见」。此前只有发起方能看到 SAS 并决定
// 接受/拒绝，被添加设备全程无感知、无否决权，唯一门槛是一次性配对码——SAS 的防中间人
// 能力只在单边生效，不构成真正的双端互认证。
//
// 时序：对端 POST /link/pair/hello 成功 → 本机引擎推 linkIncomingPairing → 本弹窗出现；
// 对端随后的 POST /link/pair/confirm 会**阻塞等待**本机决策，上限 60 秒——且这 60 秒
// 锚点是 hello 到达时刻（引擎侧 `entry.created + LOCAL_DECISION_TIMEOUT`），不是等
// confirm 抵达才重新起算，发起方核对 SAS 花掉的时间同样计入这 60 秒。本弹窗的倒计时
// 必须用同一锚点（见 DECISION_WINDOW_MS 文档），归零即自行关闭，不回传任何决策。

import { useEffect, useState } from 'react'
import * as Dialog from '@radix-ui/react-dialog'
import { useMutation, useQueryClient } from '@tanstack/react-query'
import { ShieldCheck } from 'lucide-react'
import { useI18n } from '../../lib/i18n'
import { friendlyLinkError, linkApi } from '../../lib/link'
import { incomingPairingStore, useStore } from '../../lib/ws'

/**
 * 决策窗口：与引擎 `PairingResponder::handle_confirm` 的 LOCAL_DECISION_TIMEOUT
 * （60s）一致，锚点是 hello 到达时刻——`incomingPairingStore.at` 是 WS 消息到达时
 * 打的时间戳，与引擎侧 `entry.created + LOCAL_DECISION_TIMEOUT` 同锚点，不是本弹窗
 * 实际渲染出来的时刻（两者通常很接近，但语义上是两回事，不能混用）。
 */
const DECISION_WINDOW_MS = 60_000

export function IncomingPairingDialog() {
  const { t } = useI18n()
  const qc = useQueryClient()
  const request = useStore(incomingPairingStore)
  const open = request !== null
  const [error, setError] = useState('')
  const [remaining, setRemaining] = useState(DECISION_WINDOW_MS / 1000)

  // 倒计时按到达时间戳算（而不是每秒自减），弹窗重渲染/标签页挂起都不会让它跑偏。
  useEffect(() => {
    if (!request) return
    setError('')
    const tick = () => {
      const left = Math.ceil((request.at + DECISION_WINDOW_MS - Date.now()) / 1000)
      setRemaining(Math.max(0, left))
      // 归零即关闭，不回传任何决策：引擎侧决策窗口与本地倒计时同锚在 hello
      // 到达时刻，归零意味着引擎那头此刻同样判定超时，对端此时会直接从
      // handle_confirm 拿到 PairingTimeout——不需要、也不应该主动回传拒绝
      // （会让对端看到「对方拒绝了配对」这种失实结论）。
      if (left <= 0) incomingPairingStore.set(null)
    }
    tick()
    const timer = setInterval(tick, 1000)
    return () => clearInterval(timer)
  }, [request])

  const decideMut = useMutation({
    mutationFn: ({ sessionId, accept }: { sessionId: string; accept: boolean }) =>
      linkApi.approveIncoming(sessionId, accept),
    onSuccess: (_data, vars) => {
      incomingPairingStore.set(null)
      // 批准后对端会完成落库，本机名册随之新增一台设备。
      if (vars.accept) void qc.invalidateQueries({ queryKey: ['link', 'devices'] })
    },
    onError: (err) => setError(friendlyLinkError(t, err)),
  })

  function decide(accept: boolean) {
    if (!request) return
    decideMut.mutate({ sessionId: request.sessionId, accept })
  }

  return (
    <Dialog.Root open={open}>
      <Dialog.Portal>
        <Dialog.Overlay className="wbackdrop show" />
        {/* 安全决策不能靠点外面糊弄过去：不响应点击遮罩与 ESC。 */}
        <Dialog.Content
          className="dialog sm show"
          onPointerDownOutside={(e) => e.preventDefault()}
          onEscapeKeyDown={(e) => e.preventDefault()}
        >
          <header className="dlg-head">
            <Dialog.Title asChild>
              <b>
                <ShieldCheck size={15} className="mr-1.5 inline-block align-[-2px]" />
                {t('link.incomingTitle')}
              </b>
            </Dialog.Title>
          </header>
          <div className="dlg-body">
            <Dialog.Description className="dlg-sub">{t('link.incomingHint')}</Dialog.Description>
            <div className="token-box" style={{ justifyContent: 'center' }}>
              <b className="text-[22px] font-semibold tracking-[5px]">{request?.sas}</b>
            </div>
            <p className="mt-2 text-center text-[12px] text-text3">
              {t('link.incomingFrom', { name: request?.name || '' })}
              {request?.platform ? ` · ${request.platform}` : ''}
            </p>
            <p className="mt-1 text-center text-[11.5px] text-text3">
              {t('link.incomingCountdown', { n: remaining })}
            </p>
            {error && <p className="mt-2 text-center text-[11.5px] text-danger">{error}</p>}
          </div>
          <footer className="dlg-foot">
            <button type="button" className="btn ghost" disabled={decideMut.isPending} onClick={() => decide(false)}>
              {t('link.incomingReject')}
            </button>
            <button type="button" className="btn primary" disabled={decideMut.isPending} onClick={() => decide(true)}>
              {decideMut.isPending ? t('common.loading') : t('link.incomingAccept')}
            </button>
          </footer>
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  )
}
