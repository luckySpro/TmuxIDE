// 手机端与远程端口通信。认证完全由服务端设置的 HttpOnly Cookie 完成，页面脚本拿不到任何凭证。

export class ApiError extends Error {
  constructor(message: string, public status: number, public needPin = false) {
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
  if (!res.ok) throw new ApiError(data?.error ?? `HTTP ${res.status}`, res.status, !!data?.need_pin)
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
export interface TermSession { name: string; attached: number; windows: TermWindow[] }
export interface TermList { host: string; sessions: TermSession[]; error: string | null }
