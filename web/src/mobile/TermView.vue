<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { Terminal } from '@xterm/xterm'
import { FitAddon } from '@xterm/addon-fit'
import { TERM_FONT, termFontReady } from '../termfont'
import { api, wsUrl, type Me, type TermSession, type TermWindow } from './api'
import { t } from './i18n'

const props = defineProps<{ session: string; window: string; windows: TermWindow[]; sessions: TermSession[]; readOnly: boolean }>()
const emit = defineEmits<{ back: []; select: [window: string]; switch: [session: string]; authLost: [] }>()

const host = ref<HTMLDivElement>()
const wrap = ref<HTMLDivElement>()
const input = ref('')
const ctrl = ref(false)
const notice = ref('')
const fitPhone = ref(load('tmx-fit') === '1')
const fontSize = ref(Number(load('tmx-font')) || 0)

let term: Terminal | undefined
let fit: FitAddon | undefined
let ws: WebSocket | undefined
let observer: ResizeObserver | undefined
let closed = false
let retry: number | undefined
let connId = 0

function load(k: string) {
  try { return localStorage.getItem(k) } catch { return null }
}
function store(k: string, v: string) {
  try { localStorage.setItem(k, v) } catch { /* 隐私模式 */ }
}

const current = computed(() => props.windows.find((w) => w.id === props.window))
const title = computed(() => current.value ? `${props.session} › ${current.value.name}` : props.session)

function send(msg: object) {
  if (ws?.readyState === WebSocket.OPEN) ws.send(JSON.stringify(msg))
}

// 输出历史（tmux 滚动缓冲区）
const hist = ref<{ text: string; lines: number; loading: boolean; error: string; copied: boolean } | null>(null)
const histBody = ref<HTMLPreElement>()

async function openHistory(lines = 5000) {
  const w = props.windows.find((x) => x.id === props.window)
  const p = w?.panes.find((x) => x.active) ?? w?.panes[0]
  if (!p) return
  hist.value = { text: hist.value?.text ?? '', lines, loading: true, error: '', copied: false }
  try {
    const r = await api.get<{ text: string }>(`/api/history?pane=${encodeURIComponent(p.id)}&lines=${lines}`)
    if (!hist.value) return
    hist.value.text = r.text
    await nextTick()
    if (histBody.value) histBody.value.scrollTop = histBody.value.scrollHeight
  } catch (e) {
    if (hist.value) hist.value.error = e instanceof Error ? e.message : String(e)
  } finally {
    if (hist.value) hist.value.loading = false
  }
}

async function copyHistory() {
  if (!hist.value?.text) return
  try {
    await navigator.clipboard.writeText(hist.value.text)
  } catch {
    const el = document.createElement('textarea')
    el.value = hist.value.text
    document.body.appendChild(el)
    el.select()
    document.execCommand('copy')
    el.remove()
  }
  hist.value.copied = true
}

function sendInput(data: string) {
  if (props.readOnly) return
  if (ctrl.value && data.length === 1) {
    const c = data.toUpperCase().charCodeAt(0)
    if (c >= 64 && c <= 95) data = String.fromCharCode(c - 64)
    ctrl.value = false
  }
  send({ type: 'input', data })
}

/** “保持电脑尺寸”时按可用宽度和高度选择字号，整屏放得下就不滚动；放不下时允许滚动 */
function autoFont(cols: number, rows: number) {
  const width = wrap.value?.clientWidth ?? 360
  const height = wrap.value?.clientHeight ?? 600
  const byWidth = (width - 8) / (cols * 0.602)
  const byHeight = (height - 8) / (rows * 1.1 * 1.2)
  return Math.max(7, Math.min(14, Math.floor(Math.min(byWidth, byHeight))))
}

/** 光标所在行不在可见区域时滚动过去（用户刚手动滚动过则不打扰） */
let userScrolledAt = 0
function markScroll() { userScrolledAt = Date.now() }
function followCursor() {
  const el = wrap.value
  if (!el || !term || fitPhone.value || Date.now() - userScrolledAt < 3000) return
  const screen = term.element?.querySelector('.xterm-screen') as HTMLElement | null
  if (!screen) return
  const cell = screen.clientHeight / term.rows
  const top = screen.offsetTop + term.buffer.active.cursorY * cell
  if (top < el.scrollTop || top + cell > el.scrollTop + el.clientHeight) {
    el.scrollTop = Math.max(0, top + cell * 2 - el.clientHeight)
  }
}

function build() {
  term?.dispose()
  const cols = current.value?.cols ?? 80
  const rows = current.value?.rows ?? 24
  term = new Terminal({
    fontFamily: TERM_FONT,
    fontSize: fontSize.value || (fitPhone.value ? 12 : autoFont(cols, rows)),
    lineHeight: 1.1,
    cursorBlink: !props.readOnly,
    disableStdin: props.readOnly,
    scrollback: 0,
    allowProposedApi: true,
    ...(fitPhone.value ? {} : { cols, rows }),
    theme: {
      background: '#0b0e12', foreground: '#c9d1d9', cursor: '#4ade80', selectionBackground: '#264f78',
      black: '#484f58', red: '#ff7b72', green: '#4ade80', yellow: '#f5b84a', blue: '#6cb6ff', magenta: '#d2a8ff', cyan: '#56d4dd', white: '#c9d1d9',
      brightBlack: '#6e7681', brightRed: '#ffa198', brightGreen: '#86efac', brightYellow: '#f8d07a', brightBlue: '#a5d6ff', brightMagenta: '#e2c5ff', brightCyan: '#9be9f0', brightWhite: '#f0f6fc',
    },
  })
  fit = undefined
  if (fitPhone.value) {
    fit = new FitAddon()
    term.loadAddon(fit)
  }
  term.open(host.value!)
  termFontReady.then(() => term?.refresh(0, term.rows - 1))
  if (fit) {
    try { fit.fit() } catch { /* 容器尚未布局 */ }
  }
  term.onData(sendInput)
  term.onResize(({ cols: c, rows: r }) => { if (fitPhone.value) send({ type: 'resize', cols: c, rows: r }) })
}

function connect() {
  if (!term) return
  const id = ++connId
  notice.value = ''
  ws?.close()
  ws = new WebSocket(wsUrl('/api/attach', {
    session: props.session,
    window: props.window,
    fit: fitPhone.value ? 'true' : 'false',
    cols: String(term.cols),
    rows: String(term.rows),
  }))
  ws.binaryType = 'arraybuffer'
  ws.onmessage = (e) => {
    if (typeof e.data === 'string') term?.write(e.data, followCursor)
    else term?.write(new Uint8Array(e.data), followCursor)
  }
  ws.onclose = async () => {
    if (closed || id !== connId) return
    notice.value = t('reconnecting')
    // 先确认身份：PIN 超时、设备被移除或锁定时回到对应页面
    try {
      const me = await api.get<Me>('/api/me')
      if (!me.unlocked) {
        emit('authLost')
        return
      }
    } catch { /* 网络问题，稍后重试 */ }
    if (!props.windows.length) {
      notice.value = t('ended')
      return
    }
    window.clearTimeout(retry)
    retry = window.setTimeout(connect, 1500)
  }
}

function reopen() {
  build()
  connect()
}

function selectWindow(id: string) {
  if (id === props.window) return
  emit('select', id)
}

// 切换窗口：不同窗口尺寸不同，重新建立终端
watch(() => props.window, () => nextTick(reopen))

function toggleFit() {
  fitPhone.value = !fitPhone.value
  store('tmx-fit', fitPhone.value ? '1' : '0')
  fontSize.value = 0
  reopen()
}

function zoom(delta: number) {
  const cur = term?.options.fontSize ?? 12
  fontSize.value = Math.max(6, Math.min(20, cur + delta))
  store('tmx-font', String(fontSize.value))
  if (term) {
    term.options.fontSize = fontSize.value
    if (fit) { try { fit.fit() } catch { /* 忽略 */ } }
    else nextTick(followCursor)
  }
}

function submit() {
  userScrolledAt = 0
  const text = input.value
  input.value = ''
  if (ctrl.value && text.length === 1) {
    sendInput(text)
    return
  }
  sendInput(text + '\r')
}

const keys: { label: string; data?: string; aria?: string }[] = [
  { label: 'Esc', data: '\x1b' },
  { label: 'Tab', data: '\t' },
  { label: 'Ctrl' },
  { label: '↑', data: '\x1b[A', aria: 'Up' },
  { label: '↓', data: '\x1b[B', aria: 'Down' },
  { label: '←', data: '\x1b[D', aria: 'Left' },
  { label: '→', data: '\x1b[C', aria: 'Right' },
  { label: '|', data: '|' },
  { label: '~', data: '~' },
  { label: '/', data: '/' },
]

function key(k: { label: string; data?: string }) {
  if (!k.data) {
    ctrl.value = !ctrl.value
    return
  }
  sendInput(k.data)
}

onMounted(() => {
  reopen()
  observer = new ResizeObserver(() => {
    if (fit) { try { fit.fit() } catch { /* 忽略 */ } }
    else if (term && !fontSize.value) {
      // 保持电脑尺寸：窗口大小变化时重新选字号
      const f = autoFont(term.cols, term.rows)
      if (f !== term.options.fontSize) term.options.fontSize = f
      followCursor()
    }
  })
  observer.observe(wrap.value!)
})

onBeforeUnmount(() => {
  closed = true
  window.clearTimeout(retry)
  observer?.disconnect()
  ws?.close()
  term?.dispose()
})
</script>

<template>
  <div class="term-page">
    <header class="bar term-bar">
      <button class="icon" :aria-label="t('back')" @click="emit('back')"><svg viewBox="0 0 24 24" aria-hidden="true"><path d="m15 18-6-6 6-6" /></svg></button>
      <div class="title">
        <div class="host mono">{{ title }}</div>
        <button class="sub link inline" @click="toggleFit">{{ fitPhone ? t('fitPhone') : t('keepSize') }} ⇄</button>
      </div>
      <button class="icon small hist-open" @click="openHistory()">{{ t('history') }}</button>
      <button class="icon small" aria-label="A-" @click="zoom(-1)">A−</button>
      <button class="icon small" aria-label="A+" @click="zoom(1)">A+</button>
      <span class="badge" :class="{ ro: readOnly }">{{ readOnly ? t('readonly') : t('control') }}</span>
    </header>
    <nav v-if="sessions.length > 1" class="tabs stabs" aria-label="sessions">
      <button v-for="s in sessions" :key="s.name" class="tab mono" :class="{ on: s.name === session }" @click="s.name !== session && emit('switch', s.name)">
        <span v-if="s.group" class="g">{{ s.group }}</span>{{ s.name }}
      </button>
    </nav>
    <nav v-if="windows.length > 1" class="tabs" aria-label="windows">
      <button v-for="w in windows" :key="w.id" class="tab mono" :class="{ on: w.id === window }" @click="selectWindow(w.id)">{{ w.index }} {{ w.name }}</button>
    </nav>
    <div ref="wrap" class="term-wrap" :class="{ scroll: !fitPhone }" @wheel.passive="markScroll" @touchmove.passive="markScroll">
      <div ref="host" class="term-host" />
      <div v-if="notice" class="notice">{{ notice }}</div>
    </div>
    <div v-if="hist" class="hist" role="dialog" :aria-label="t('historyTitle')">
      <header class="bar term-bar">
        <div class="title"><div class="host">{{ t('historyTitle') }}</div><div class="sub mono">{{ title }}</div></div>
        <button class="icon small" @click="copyHistory">{{ hist.copied ? t('copied') : t('copyAll') }}</button>
        <button class="icon small" @click="hist = null">{{ t('close') }}</button>
      </header>
      <p v-if="hist.error" class="error">{{ hist.error }}</p>
      <pre ref="histBody" class="hist-body mono">{{ hist.text || (hist.loading ? '…' : t('historyEmpty')) }}</pre>
      <footer class="hist-foot">
        <span>{{ t('historyHint') }}</span>
        <button v-if="hist.lines < 50000" class="link inline" :disabled="hist.loading" @click="openHistory(50000)">{{ t('historyMore') }}</button>
      </footer>
    </div>
    <div v-if="readOnly" class="ro-note">{{ t('readonlyNote') }}</div>
    <div v-else class="controls">
      <div class="keys">
        <button v-for="k in keys" :key="k.label" class="key mono" :class="{ on: !k.data && ctrl }" :aria-label="k.aria" @click="key(k)">{{ k.label }}</button>
      </div>
      <form class="send" @submit.prevent="submit">
        <label class="sr-only" for="cmd">{{ t('inputPlaceholder') }}</label>
        <input id="cmd" v-model="input" class="input mono" :placeholder="t('inputPlaceholder')" autocomplete="off" autocapitalize="off" autocorrect="off" spellcheck="false" enterkeyhint="send" />
        <button class="primary">{{ t('send') }}</button>
      </form>
      <div class="keys wide">
        <button class="key" @click="sendInput('\x03')">{{ t('interrupt') }}</button>
        <button class="key mono" @click="sendInput('\x04')">^D</button>
        <button class="key" @click="sendInput('\x0c')">{{ t('clear') }}</button>
        <button class="key" @click="term?.focus()">{{ t('keyboard') }}</button>
      </div>
    </div>
  </div>
</template>
