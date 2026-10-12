import { reactive } from 'vue'
import { api, ApiError, type AppInfo, type RemoteSummary, type SavedEntry, type SessionInfo, type SyncStatus, type TemplateEntry, type UpdateView } from './api'
import { lang, t } from './i18n'

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
  remote: null as RemoteSummary | null,
  /** 从状态栏点进设置时直接打开某个分区 */
  settingsSection: '' as string,
  /** 会话选项卡只显示这个分组（空为全部） */
  groupFilter: loadGroup(),
  update: null as UpdateView | null,
})

function loadGroup(): string {
  try { return localStorage.getItem('tmx-group') ?? '' } catch { return '' }
}

export function setGroupFilter(g: string) {
  state.groupFilter = g
  try { localStorage.setItem('tmx-group', g) } catch { /* 忽略 */ }
}

/** 已有的分组名称（来自正在运行的会话） */
export function groupNames(): string[] {
  return [...new Set(state.sessions.map((s) => s.group).filter(Boolean))].sort((a, b) => a.localeCompare(b))
}

export async function copyText(text: string) {
  try {
    await navigator.clipboard.writeText(text)
  } catch {
    // 非安全上下文（例如通过 http 访问）时退回旧方法
    const el = document.createElement('textarea')
    el.value = text
    el.style.position = 'fixed'
    el.style.opacity = '0'
    document.body.appendChild(el)
    el.select()
    document.execCommand('copy')
    el.remove()
  }
  toast(t('copied'))
}

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
  pollRemote()
  setInterval(pollRemote, 4000)
  pollUpdate()
}

/** 升级状态：下载中每秒刷新，平时每分钟刷新（检查本身由服务定时执行） */
let updateTimer: number | undefined
export async function pollUpdate() {
  window.clearTimeout(updateTimer)
  try {
    state.update = await api.get<UpdateView>('/update')
  } catch { /* 旧版服务 */ }
  const busy = state.update && ['checking', 'downloading'].includes(state.update.state)
  updateTimer = window.setTimeout(pollUpdate, busy ? 1000 : 60000)
}

export async function updateAction(path: 'check' | 'install' | 'restart') {
  try {
    const r = await api.post<UpdateView | null>(`/update/${path}`)
    if (r) state.update = r
  } catch (e) {
    handle(e)
  }
  if (path !== 'restart') pollUpdate()
}

/** 有手机配对、打开终端或认证失败时在电脑上提醒 */
const NOTIFY_KINDS = ['pair', 'attach', 'pair_fail', 'unlock_fail', 'locked', 'expire']
async function pollRemote() {
  try {
    const since = state.remote?.seq ?? 0
    const r = await api.get<RemoteSummary>(`/remote/events?since=${since}`)
    if (since && r.notify) {
      for (const e of [...r.events].reverse()) {
        if (!NOTIFY_KINDS.includes(e.kind)) continue
        const bad = e.kind.endsWith('_fail') || e.kind === 'locked'
        toast(`${t('remoteBar')}：${t('remoteNotice', { device: e.device ?? '', detail: e.detail })}`, bad ? 'error' : 'ok')
      }
    }
    state.remote = r
  } catch { /* 旧版服务或暂时不可用 */ }
}

export function currentSession(): SessionInfo | undefined {
  return state.sessions.find((s) => s.name === state.selected)
}
