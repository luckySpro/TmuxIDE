<script setup lang="ts">
import { onActivated, onBeforeUnmount, onMounted, ref } from 'vue'
import { Terminal } from '@xterm/xterm'
import { FitAddon } from '@xterm/addon-fit'
import { WebLinksAddon } from '@xterm/addon-web-links'
import { attachUrl } from '../api'
import { TERM_FONT, termFontReady } from '../termfont'
import { state } from '../store'

const props = defineProps<{ session: string }>()
const host = ref<HTMLDivElement>()
let term: Terminal | undefined
let ws: WebSocket | undefined
let observer: ResizeObserver | undefined
let closed = false
let retry: number | undefined

const fit = new FitAddon()

function send(msg: object) {
  if (ws?.readyState === WebSocket.OPEN) ws.send(JSON.stringify(msg))
}

const exists = () => state.sessions.some((s) => s.name === props.session)

function connect() {
  if (!term) return
  ws = new WebSocket(attachUrl(props.session, term.cols, term.rows))
  ws.binaryType = 'arraybuffer'
  ws.onmessage = (e) => {
    if (typeof e.data === 'string') term?.write(e.data)
    else term?.write(new Uint8Array(e.data))
  }
  ws.onclose = () => {
    // tmux 客户端意外断开（例如服务重启）时自动重连；会话被关闭时列表刷新会卸载本组件
    // 被选项卡缓存的终端：会话已关闭或改名时不再重连
    if (!closed && exists()) retry = window.setTimeout(connect, 1500)
  }
}

onMounted(() => {
  term = new Terminal({
    fontFamily: TERM_FONT,
    fontSize: 13,
    lineHeight: 1.15,
    cursorBlink: true,
    allowProposedApi: true,
    macOptionIsMeta: true,
    scrollback: 0,
    theme: {
      background: '#0d1014', foreground: '#c9d1d9', cursor: '#4ade80', selectionBackground: '#264f78',
      black: '#484f58', red: '#ff7b72', green: '#4ade80', yellow: '#f5b84a', blue: '#6cb6ff', magenta: '#d2a8ff', cyan: '#56d4dd', white: '#c9d1d9',
      brightBlack: '#6e7681', brightRed: '#ffa198', brightGreen: '#86efac', brightYellow: '#f8d07a', brightBlue: '#a5d6ff', brightMagenta: '#e2c5ff', brightCyan: '#9be9f0', brightWhite: '#f0f6fc',
    },
  })
  term.loadAddon(fit)
  term.loadAddon(new WebLinksAddon())
  term.open(host.value!)
  termFontReady.then(() => term?.refresh(0, term.rows - 1))
  fit.fit()
  term.onData((data) => send({ type: 'input', data }))
  term.onResize(({ cols, rows }) => send({ type: 'resize', cols, rows }))
  observer = new ResizeObserver(() => {
    // 切到其他选项卡时终端被移出页面，尺寸为 0，不能据此缩小 tmux 窗口
    if (!host.value?.isConnected || !host.value.clientWidth || !host.value.clientHeight) return
    try { fit.fit() } catch { /* 容器隐藏时忽略 */ }
  })
  observer.observe(host.value!)
  connect()
  term.focus()
})

onActivated(() => {
  try { fit.fit() } catch { /* 忽略 */ }
  if (ws?.readyState === WebSocket.CLOSED && exists()) {
    window.clearTimeout(retry)
    connect()
  }
  term?.focus()
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
  <div ref="host" class="term-host" />
</template>

<style scoped>
.term-host { flex: 1; min-height: 0; overflow: hidden; background: var(--bg); }
</style>
