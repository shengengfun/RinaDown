// 插件系统共享的读写 hooks —— 对齐 components/settings/useConfig.ts。
// 列表走 ['plugins'] Query 缓存（WS pluginsChanged 直接 invalidate，见 lib/ws.ts）；
// 各类写操作成功后统一 invalidate 该缓存以取回最新 enabled/settings/disabledReason。

import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { api } from '../lib/api'

export function usePluginsQuery() {
  return useQuery({ queryKey: ['plugins'], queryFn: api.listPlugins })
}

function useInvalidatePlugins() {
  const qc = useQueryClient()
  return () => qc.invalidateQueries({ queryKey: ['plugins'] })
}

export function useInstallPluginMutation() {
  const invalidate = useInvalidatePlugins()
  return useMutation({
    mutationFn: (zip: File | Blob | ArrayBuffer) => api.installPlugin(zip),
    onSuccess: invalidate,
  })
}

export function useInstallPluginDevMutation() {
  const invalidate = useInvalidatePlugins()
  return useMutation({
    mutationFn: (dirPath: string) => api.installPluginDev(dirPath),
    onSuccess: invalidate,
  })
}

export function useSetPluginEnabledMutation() {
  const invalidate = useInvalidatePlugins()
  return useMutation({
    mutationFn: ({ identity, enabled }: { identity: string; enabled: boolean }) =>
      api.setPluginEnabled(identity, enabled),
    onSuccess: invalidate,
  })
}

export function useUpdatePluginSettingsMutation() {
  const invalidate = useInvalidatePlugins()
  return useMutation({
    mutationFn: ({ identity, entries }: { identity: string; entries: Record<string, string> }) =>
      api.updatePluginSettings(identity, entries),
    onSuccess: invalidate,
  })
}

export function useUninstallPluginMutation() {
  const invalidate = useInvalidatePlugins()
  return useMutation({
    mutationFn: (identity: string) => api.uninstallPlugin(identity),
    onSuccess: invalidate,
  })
}

export function useIgnorePluginRetryMutation() {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: (taskId: string) => api.ignorePluginRetry(taskId),
    onSuccess: () => qc.invalidateQueries({ queryKey: ['tasks'] }),
  })
}

export function useMarketQuery() {
  return useQuery({ queryKey: ['market'], queryFn: api.listMarket })
}

export function useInstallFromMarket() {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: (pluginId: string) => api.installFromMarket(pluginId),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ['plugins'] })
      void qc.invalidateQueries({ queryKey: ['market'] })
    },
  })
}
