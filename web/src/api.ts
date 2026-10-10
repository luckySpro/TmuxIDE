// 与本地 TmuxIDE 服务通信。令牌由启动方通过 URL # 片段传入，只保存在本标签页的 sessionStorage。

const KEY = 'tmuxide-token'
let token = ''

export function captureToken() {
  const m = location.hash.match(/token=([0-9a-f]+)/)
  if (m) {
    token = m[1]
    try { sessionStorage.setItem(KEY, token) } catch { /* 隐私模式下忽略 */ }
    history.replaceState(null, '', location.pathname + location.search)
  } else {
    try { token = sessionStorage.getItem(KEY) ?? '' } catch { token = '' }
  }
}

export function getToken() {
  return token
}

export class ApiError extends Error {
  constructor(message: string, public status: number) {
    super(message)
  }
}

async function request<T>(method: string, path: string, body?: unknown): Promise<T> {
  const res = await fetch(`/api${path}`, {
    method,
    headers: {
      'x-tmuxide-token': token,
      ...(body !== undefined ? { 'content-type': 'application/json' } : {}),
    },
    body: body !== undefined ? JSON.stringify(body) : undefined,
  })
  const text = await res.text()
  const data = text ? JSON.parse(text) : null
  if (!res.ok) throw new ApiError(data?.error ?? `HTTP ${res.status}`, res.status)
  return data as T
}

export const api = {
  get: <T>(p: string) => request<T>('GET', p),
  post: <T>(p: string, b: unknown = {}) => request<T>('POST', p, b),
  put: <T>(p: string, b: unknown) => request<T>('PUT', p, b),
  del: <T>(p: string) => request<T>('DELETE', p),
}

export function attachUrl(session: string, cols: number, rows: number) {
  const proto = location.protocol === 'https:' ? 'wss' : 'ws'
  const q = new URLSearchParams({ session, cols: String(cols), rows: String(rows), token })
  return `${proto}://${location.host}/api/attach?${q}`
}

export const enc = encodeURIComponent

// ---- 类型 ----

export interface PaneInfo { index: number; id: string; active: boolean; pid: number; width: number; height: number; command: string; cwd: string }
export interface WindowInfo { index: number; id: string; name: string; active: boolean; layout: string; panes: PaneInfo[] }
export interface SessionInfo { name: string; id: string; attached: number; created: number; windows: WindowInfo[] }
export interface SavedEntry { host: string; name: string; saved_at: string; windows: number; panes: number; local: boolean; running: boolean }
export interface TemplateEntry { name: string; windows: string[]; panes: number }
export interface CommitInfo { id: string; short: string; summary: string; time: number; author: string }
export interface DiffLine { tag: ' ' | '+' | '-'; old_no: number | null; new_no: number | null; text: string }
export interface SyncStatus { branch: string; has_remote: boolean; ahead: number; behind: number; head: string | null; dirty: boolean }
export interface AppInfo {
  version: string; host: string; data_dir: string; repo_dir: string; git_engine: string; os: string; arch: string
  tmux: { path: string; version: string; bundled: boolean; socket: string }
}
export interface SaveResult { saved: string[]; unchanged: string[]; commit: string | null; pushed: boolean; push_error: string | null }
export interface RestoreStep { kind: string; session?: string; window?: string; cwd?: string; window_pos?: number; pane_pos?: number; cmd?: string; layout?: string }
export interface RestoreResult { session: string; skipped: boolean; report: { warnings: string[] } | null }
export interface Config {
  git: {
    repo_dir: string; remote: string; branch: string; auth: string; ssh_key: string; username: string
    author_name: string; author_email: string; auto_commit: boolean; auto_push: boolean; sync_tmux_conf: boolean
  }
  save: { autosave_minutes: number; scrollback: boolean; scrollback_lines: number; exclude: string[] }
  tmux: { use_system: boolean; socket: string }
  ui: { language: string }
}
export interface SettingsView { config: Config; has_token: boolean; has_passphrase: boolean; token?: string | null; passphrase?: string | null }
