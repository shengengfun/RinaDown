// 组件（ffmpeg / yt-dlp）共享的读写 hooks —— 对齐 usePlugins.ts。
// 状态走 ['ffmpegStatus'] Query 缓存：WS componentResult 直接 invalidate（见 lib/ws.ts），
// 安装/卸载 mutation 成功后同样 invalidate 以取回最新来源/版本。

import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { api } from '../lib/api'

export function useFfmpegStatusQuery() {
  return useQuery({ queryKey: ['ffmpegStatus'], queryFn: api.getFfmpegStatus })
}

export function useFfmpegVersionsQuery(enabled: boolean) {
  return useQuery({ queryKey: ['ffmpegVersions'], queryFn: api.getFfmpegVersions, enabled })
}

export function useInstallFfmpegMutation() {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: (version?: string) => api.installFfmpeg(version),
    onSuccess: () => qc.invalidateQueries({ queryKey: ['ffmpegStatus'] }),
  })
}

export function useUninstallFfmpegMutation() {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: () => api.uninstallFfmpeg(),
    onSuccess: () => qc.invalidateQueries({ queryKey: ['ffmpegStatus'] }),
  })
}

export function useYtdlpStatusQuery() {
  return useQuery({ queryKey: ['ytdlpStatus'], queryFn: api.getYtdlpStatus })
}

export function useYtdlpVersionsQuery(enabled: boolean) {
  return useQuery({ queryKey: ['ytdlpVersions'], queryFn: api.getYtdlpVersions, enabled })
}

export function useInstallYtdlpMutation() {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: (version?: string) => api.installYtdlp(version),
    onSuccess: () => qc.invalidateQueries({ queryKey: ['ytdlpStatus'] }),
  })
}

export function useUninstallYtdlpMutation() {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: () => api.uninstallYtdlp(),
    onSuccess: () => qc.invalidateQueries({ queryKey: ['ytdlpStatus'] }),
  })
}
