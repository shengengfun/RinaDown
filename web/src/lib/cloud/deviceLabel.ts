// 设备展示标签：设备名由用户自取（且默认取自主机名），同一账号下重名很常见——
// 两台都叫 DESKTOP-RSQ4S1D 的机器在下发选择器/侧栏/设置列表里必须能分辨，否则
// 下发目标只能靠猜。规则是「只在重名时才加短码」，避免给唯一名称也挂上噪音。

import type { CloudDevice } from './types'

/** 短码长度：6 位十六进制足够在个位数设备量级里区分，且不至于长到挤掉设备名。 */
const SHORT_ID_LEN = 6

/**
 * 同名设备可区分标签：名称在 `all` 中重复出现（≥2 次）时追加 deviceId 前 6 位短码，
 * 名称唯一则原样返回；名称为空时回退为短码。
 *
 * 硬约定：`all` 必须传入含本机在内的全量设备列表——本机与某台远端同名也要能被
 * 短码区分，否则侧栏/新建下载/账户设置三处入口对同一台远端设备算出的重名结果
 * 会不一致，导致同一设备在不同页面显示不同的名字。
 */
export function deviceLabel(device: CloudDevice, all: readonly CloudDevice[]): string {
  const name = device.name.trim()
  const short = device.deviceId.slice(0, SHORT_ID_LEN)
  if (!name) return short
  let seen = 0
  for (const d of all) {
    if (d.name.trim() === name && ++seen > 1) return `${name} · ${short}`
  }
  return name
}
