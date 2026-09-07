// 登录凭证存储：记住设备 → localStorage；否则 sessionStorage。

const TOKEN_KEY = 'fluxdown.token'
const BASE_KEY = 'fluxdown.base'

export function getToken(): string {
  return sessionStorage.getItem(TOKEN_KEY) ?? localStorage.getItem(TOKEN_KEY) ?? ''
}

/** 服务器基址。同源部署时为空字符串（相对路径）。 */
export function getBase(): string {
  return sessionStorage.getItem(BASE_KEY) ?? localStorage.getItem(BASE_KEY) ?? ''
}

export function saveCredentials(base: string, token: string, remember: boolean) {
  clearCredentials()
  const store = remember ? localStorage : sessionStorage
  store.setItem(TOKEN_KEY, token)
  store.setItem(BASE_KEY, base)
}

/**
 * 就地替换已存凭证里的 token，保留原有的「记住此设备」选择。
 *
 * 服务器改密钥（设置页 / 重新生成）会**立即**让旧密钥失效——不同步本地凭证的话
 * 下一个请求就 401 把用户踢回登录页。未登录时不写入。
 */
export function updateStoredToken(token: string) {
  const store = sessionStorage.getItem(TOKEN_KEY) !== null ? sessionStorage : localStorage
  if (store.getItem(TOKEN_KEY) === null) return
  store.setItem(TOKEN_KEY, token)
}

export function clearCredentials() {
  sessionStorage.removeItem(TOKEN_KEY)
  sessionStorage.removeItem(BASE_KEY)
  localStorage.removeItem(TOKEN_KEY)
  localStorage.removeItem(BASE_KEY)
}

export function isAuthenticated(): boolean {
  return getToken() !== ''
}
