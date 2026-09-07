// 访问密钥（管理 token）策略 —— Rust 侧 `native/server/src/config.rs`
// `validate_access_key` / `generate_access_key` 的逐条镜像。
//
// 两侧必须一致：前端放行、后端拒收 = 首次运行向导直接卡死在「按了没反应」。
// 改任一侧都要同步改另一侧（后端单测 `access_key_tests` 覆盖同一组用例）。

/** 最短长度，与 Rust `ACCESS_KEY_MIN_LEN` 一致。 */
export const ACCESS_KEY_MIN_LEN = 8
/** 最长长度，与 Rust `ACCESS_KEY_MAX_LEN` 一致。 */
export const ACCESS_KEY_MAX_LEN = 128

/** 校验失败原因；`null` = 通过。调用方据此查 i18n 文案。 */
export type AccessKeyIssue = 'badChars' | 'tooShort' | 'tooLong' | 'needsMix'

/**
 * 校验访问密钥。判定顺序与 Rust 侧一致，保证两端给出同一个原因。
 */
export function validateAccessKey(key: string): AccessKeyIssue | null {
  // ASCII 可见字符：排除空白（HTTP 头/命令行里会被静默吞掉）与非 ASCII。
  if (!/^[\x21-\x7e]*$/.test(key)) return 'badChars'
  if (key.length < ACCESS_KEY_MIN_LEN) return 'tooShort'
  if (key.length > ACCESS_KEY_MAX_LEN) return 'tooLong'
  if (!/[A-Za-z]/.test(key) || !/[0-9]/.test(key)) return 'needsMix'
  return null
}

const KEY_ALPHABET = 'ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz23456789'

/**
 * 生成一个满足策略的随机密钥（`fxd_` + 24 位易读字符）。
 *
 * 字母表剔除了 `0O1lI` 这类形近字符——密钥常被手抄到别的设备上。
 * 循环兜住「随机结果恰好缺字母或缺数字」的小概率取值。
 */
export function randomAccessKey(): string {
  for (;;) {
    const bytes = new Uint8Array(24)
    crypto.getRandomValues(bytes)
    const body = Array.from(bytes, (b) => KEY_ALPHABET[b % KEY_ALPHABET.length]).join('')
    const key = `fxd_${body}`
    if (validateAccessKey(key) === null) return key
  }
}
