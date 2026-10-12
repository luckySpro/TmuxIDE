// 手机端与远程端口通信。认证完全由服务端设置的 HttpOnly Cookie 完成，页面脚本拿不到任何凭证。

export class ApiError extends Error {
  constructor(message: string, public status: number, public needPin = false, public tunnelAuth = false) {
    super(message)
  }
}

async function request<T>(method: string, path: string, body?: unknown): Promise<T> {
  const res = await fetch(path, {
    method,
    credentials: 'same-origin',
    headers: body !== undefined ? { 'content-type': 'application/json' } : {},
    body: body !== undefined ? JSON.stringify(body) : undefined,
  })
  const text = await res.text()
  let data: any = null
  try { data = text ? JSON.parse(text) : null } catch { /* 非 JSON 错误页 */ }
  if (!res.ok) {
    // 不是 TmuxIDE 的 JSON 错误：请求在隧道一层就被拦下了。
    // Cloudflare 快速隧道开启“允许的邮箱”后，邮箱验证过期时 POST 会直接得到纯文本 401。
    const tunnelAuth = res.status === 401 && !data?.error
    throw new ApiError(data?.error ?? `HTTP ${res.status}`, res.status, !!data?.need_pin, tunnelAuth)
  }
  return data as T
}

export const api = {
  get: <T>(p: string) => request<T>('GET', p),
  post: <T>(p: string, b: unknown = {}) => request<T>('POST', p, b),
}

export function wsUrl(path: string, params?: Record<string, string>) {
  const proto = location.protocol === 'https:' ? 'wss' : 'ws'
  const q = params ? `?${new URLSearchParams(params)}` : ''
  return `${proto}://${location.host}${path}${q}`
}

export interface Me {
  host: string
  version: string
  paired: boolean
  unlocked: boolean
  need_pin?: boolean
  locked?: boolean
  device?: string
  read_only?: boolean
  admin?: boolean
}

export interface TermPane { id: string; index: number; active: boolean; command: string; cwd: string; preview: string }
export interface TermWindow { id: string; index: number; name: string; active: boolean; cols: number; rows: number; panes: TermPane[] }
export interface TermSession { name: string; attached: number; group: string; windows: TermWindow[] }
export interface TermList { host: string; sessions: TermSession[]; error: string | null }
