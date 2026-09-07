// 引擎 config 表（`GET/PUT /api/v1/config`）的读写与 UI 区块偏好解析。
//
// config 表是整套客户端的共享键值存储：桌面端与本 SPA 写同一批键、连同一台引擎，
// 因此这里的区块显隐一改，桌面端下次读配置也跟着变（反之亦然）。键名与取值格式
// （布尔序列化成 `"true"` / `"false"`）由此成为跨端契约，不可单方面改动。
//
// staleTime: Infinity —— 仅在本地成功提交后失效重取，避免后台自动刷新打断正在编辑的字段。

import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { api } from './api'
import type { ConfigMap } from './types'

export function useConfigQuery() {
  return useQuery({ queryKey: ['config'], queryFn: api.getConfig, staleTime: Infinity })
}

export function useConfigMutation() {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: (entries: ConfigMap) => api.putConfig(entries),
    onSuccess: () => qc.invalidateQueries({ queryKey: ['config'] }),
  })
}

// ---------------------------------------------------------------------------
// 界面区块显隐
// ---------------------------------------------------------------------------

/** 可独立开关的界面区块。`status` 落在主区的状态页签上，其余四个是侧边栏区块。 */
export type UiSection = 'status' | 'queues' | 'rss' | 'category' | 'device'

/** 区块 → config 键。 */
export const SECTION_KEY: Record<UiSection, string> = {
  status: 'show_sidebar_status',
  queues: 'show_sidebar_queues',
  rss: 'show_sidebar_rss',
  category: 'show_sidebar_category',
  device: 'show_sidebar_device',
}

/** 区块展开状态 → config 键（区块标题左侧的折叠箭头）。 */
export const EXPANDED_KEY: Record<'queues' | 'rss' | 'category' | 'device', string> = {
  queues: 'sidebar_queues_expanded',
  rss: 'sidebar_rss_expanded',
  category: 'sidebar_category_expanded',
  device: 'sidebar_device_expanded',
}

/** 自定义分类列表（JSON 数组字符串）。 */
export const CATEGORIES_KEY = 'custom_categories'

/** 三态布尔：键不存在 = null（由调用方决定默认语义），否则按 `"true"` 判定。 */
export function readTriBool(config: ConfigMap | undefined, key: string): boolean | null {
  const raw = config?.[key]
  if (raw === undefined || raw === '') return null
  return raw === 'true' || raw === '1'
}

/** 缺省即开启：四个内容区块默认全显示，只有显式写过 `"false"` 才隐藏。 */
export function readBool(config: ConfigMap | undefined, key: string, fallback = true): boolean {
  return readTriBool(config, key) ?? fallback
}

export function boolEntry(key: string, value: boolean): ConfigMap {
  return { [key]: String(value) }
}
