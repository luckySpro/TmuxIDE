import { reactive } from 'vue'
import { api, ApiError, type AppInfo, type SavedEntry, type SessionInfo, type SyncStatus, type TemplateEntry } from './api'
import { lang } from './i18n'

export type View = 'workspace' | 'settings' | 'history'

export interface Toast { id: number; text: string; kind: 'ok' | 'error' }

export const state = reactive({
  view: 'workspace' as View,
  info: null as AppInfo | null,
  sessions: [] as SessionInfo[],
  saved: [] as SavedEntry[],
  templates: [] as TemplateEntry[],
  git: null as SyncStatus | null,
  autosaveMinutes: 5,
  selected: null as string | null,
  historyTarget: null as { host: string; name: string } | null,
  toasts: [] as Toast[],
  busy: false,
  authError: false,
})

let toastId = 0
export function toast(text: string, kind: 'ok' | 'error' = 'ok') {
  const id = ++toastId
  state.toasts.push({ id, text, kind })
  setTimeout(() => {
    const i = state.toasts.findIndex((t) => t.id === id)
    if (i >= 0) state.toasts.splice(i, 1)
  }, kind === 'error' ? 7000 : 3500)
}

function handle(e: unknown) {
  if (e instanceof ApiError && e.status === 401) state.authError = true
  toast(e instanceof Error ? e.message : String(e), 'error')
}

/** 执行一个操作：显示忙碌状态、失败时提示，成功后刷新数据 */
export async function act<T>(fn: () => Promise<T>, ok?: (r: T) => string | void): Promise<T | undefined> {
  state.busy = true
  try {
    const r = await fn()
    const msg = ok?.(r)
    if (msg) toast(msg)
    await refreshAll()
    return r
  } catch (e) {
    handle(e)
    return undefined
  } finally {
    state.busy = false
  }
}

export async function refreshSessions() {
  try {
    state.sessions = await api.get<SessionInfo[]>('/sessions')
    if (state.selected && !state.sessions.some((s) => s.name === state.selected)) state.selected = null
    if (!state.selected && state.sessions.length) state.selected = state.sessions[0].name
  } catch (e) {
    if (e instanceof ApiError && e.status === 401) state.authError = true
  }
}

export async function refreshAll() {
  try {
    const [saved, templates, git] = await Promise.all([
      api.get<SavedEntry[]>('/saved'),
      api.get<TemplateEntry[]>('/templates'),
      api.get<SyncStatus>('/git/status'),
    ])
    state.saved = saved
    state.templates = templates
    state.git = git
  } catch (e) {
    if (e instanceof ApiError && e.status === 401) state.authError = true
  }
  await refreshSessions()
}

export async function init() {
  try {
    state.info = await api.get<AppInfo>('/info')
    const s = await api.get<{ config: { save: { autosave_minutes: number }; ui: { language: string } } }>('/settings')
    state.autosaveMinutes = s.config.save.autosave_minutes
    lang.value = s.config.ui.language || 'zh-CN'
  } catch (e) {
    handle(e)
  }
  await refreshAll()
  setInterval(refreshSessions, 2500)
  setInterval(refreshAll, 15000)
}

export function currentSession(): SessionInfo | undefined {
  return state.sessions.find((s) => s.name === state.selected)
}
