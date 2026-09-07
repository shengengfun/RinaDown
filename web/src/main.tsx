import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { RouterProvider } from '@tanstack/react-router'
import * as Tooltip from '@radix-ui/react-tooltip'
import './index.css'
import { router } from './router'
import { ConfirmDialog } from './components/dialogs/confirm-dialog'
import { OVERFLOW_TOOLTIP_DELAY } from './components/OverflowTooltip'
import { ThemeProvider } from './lib/theme'
import { ToastHost } from './lib/toast'
import { I18nProvider } from './lib/i18n'
import { connectWs } from './lib/ws'
import { isAuthenticated, saveCredentials } from './lib/auth'
import { attachCdnServices } from './lib/cloud/cdn'
import { attachRemoteTasks } from './lib/cloud/useRemoteTasks'

const queryClient = new QueryClient({
  defaultOptions: {
    queries: {
      staleTime: 10_000,
      retry: 1,
      refetchOnWindowFocus: false,
    },
  },
})

// URL 携带 ?token=（可选 ?base=）时自动登录——用于演示站分享链接，
// 保存凭证后立即从地址栏抹除令牌，避免泄露到历史记录/截图。
const params = new URLSearchParams(window.location.search)
const urlToken = params.get('token')
if (urlToken) {
  saveCredentials(params.get('base') ?? '', urlToken, true)
  params.delete('token')
  params.delete('base')
  const qs = params.toString()
  window.history.replaceState(
    null,
    '',
    window.location.pathname + (qs ? `?${qs}` : '') + window.location.hash,
  )
}

// 已登录会话（刷新页面）直接建立 WS。
if (isAuthenticated()) connectWs(queryClient)

// FluxCloud CDN 聚合配置拉取 + 众包遥测上报：常开后台服务，云账户登录即生效
// （未登录静默待命；断网静默重试，对齐桌面端 home_page 的接线，见 lib/cloud/cdn.ts）。
attachCdnServices()

// FluxCloud 跨设备任务：登录即常驻 `/tasks/events` SSE（查看端快照 + 本机接单执行端）。
// 必须与路由无关地常开——否则本机在别的设备眼里时隐时现，下发到本机的任务也收不到。
attachRemoteTasks(queryClient)

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <QueryClientProvider client={queryClient}>
      <I18nProvider>
        <ThemeProvider>
          {/* 全局唯一 Tooltip.Provider：skipDelayDuration 让相邻条目之间连续划过时
              第二个气泡立刻出现，而不是每行都重新等 500ms（见 OverflowTooltip）。 */}
          <Tooltip.Provider delayDuration={OVERFLOW_TOOLTIP_DELAY} skipDelayDuration={300}>
            <RouterProvider router={router} />
          </Tooltip.Provider>
          <ConfirmDialog />
          <ToastHost />
        </ThemeProvider>
      </I18nProvider>
    </QueryClientProvider>
  </StrictMode>,
)
