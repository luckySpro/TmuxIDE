<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from 'vue'
import { api, ApiError, wsUrl, type Me, type TermList } from './api'
import { t } from './i18n'
import TermView from './TermView.vue'

type Screen = 'loading' | 'pair' | 'pin' | 'locked' | 'list' | 'term' | 'error'

const screen = ref<Screen>('loading')
const me = ref<Me | null>(null)
const error = ref('')
const busy = ref(false)

// 配对
const code = ref('')
const fromQr = ref(false)
const deviceName = ref(guessName())
// PIN
const pin = ref('')

// 终端列表
const list = ref<TermList | null>(null)
const live = ref(false)
let events: WebSocket | undefined
let retry: number | undefined
let stopped = false

// 打开的终端
const target = ref<{ session: string; window: string } | null>(null)

function guessName() {
  const ua = navigator.userAgent
  const kind = /iPad/.test(ua) ? 'iPad' : /iPhone/.test(ua) ? 'iPhone' : /Android/.test(ua) ? 'Android' : /Macintosh/.test(ua) ? 'Mac' : t('deviceName')
  return kind
}

function readPairFromHash() {
  const m = location.hash.match(/pair=([A-Za-z0-9-]+)/)
  if (m) {
    code.value = m[1].toUpperCase()
    fromQr.value = true
    history.replaceState(null, '', location.pathname + location.search)
  }
}

async function refreshMe() {
  try {
    me.value = await api.get<Me>('/api/me')
    document.title = `${me.value.host} · TmuxIDE`
    if (!me.value.paired) screen.value = 'pair'
    else if (me.value.locked) screen.value = 'locked'
    else if (!me.value.unlocked) screen.value = 'pin'
    else {
      if (screen.value !== 'term') screen.value = 'list'
      connectEvents()
    }
  } catch (e) {
    error.value = e instanceof Error ? e.message : String(e)
    screen.value = 'error'
  }
}

function connectEvents() {
  if (events && events.readyState <= WebSocket.OPEN) return
  events = new WebSocket(wsUrl('/api/events'))
  events.onopen = () => { live.value = true }
  events.onmessage = (e) => {
    try { list.value = JSON.parse(e.data) } catch { /* 忽略 */ }
  }
  events.onclose = () => {
    live.value = false
    events = undefined
    if (stopped) return
    // 断开可能是网络问题，也可能是 PIN 超时或设备被移除：先确认身份再重连
    window.clearTimeout(retry)
    retry = window.setTimeout(refreshMe, 2000)
  }
}

function closeEvents() {
  window.clearTimeout(retry)
  const ws = events
  events = undefined
  ws?.close()
}

async function pair() {
  busy.value = true
  error.value = ''
  try {
    const r = await api.post<{ need_pin: boolean }>('/api/pair', { code: code.value, name: deviceName.value })
    pin.value = ''
    if (r.need_pin) screen.value = 'pin'
    await refreshMe()
  } catch (e) {
    error.value = e instanceof Error ? e.message : String(e)
  } finally {
    busy.value = false
  }
}

async function unlock() {
  busy.value = true
  error.value = ''
  try {
    await api.post('/api/unlock', { pin: pin.value })
    pin.value = ''
    await refreshMe()
  } catch (e) {
    error.value = e instanceof Error ? e.message : String(e)
    pin.value = ''
    if (e instanceof ApiError && e.status === 423) screen.value = 'locked'
  } finally {
    busy.value = false
  }
}

async function lockNow() {
  closeEvents()
  target.value = null
  try { await api.post('/api/lock') } catch { /* 忽略 */ }
  await refreshMe()
}

async function forget() {
  if (!confirm(t('confirmForget'))) return
  closeEvents()
  try { await api.post('/api/forget') } catch { /* 忽略 */ }
  target.value = null
  await refreshMe()
}

function open(session: string, window: string) {
  target.value = { session, window }
  screen.value = 'term'
}

function closeTerm() {
  target.value = null
  screen.value = 'list'
}

/** 终端断开且身份校验失败时回到对应页面 */
async function termAuthLost() {
  target.value = null
  screen.value = 'list'
  await refreshMe()
}

const termSession = computed(() => list.value?.sessions.find((s) => s.name === target.value?.session) ?? null)

function activePane(w: TermList['sessions'][0]['windows'][0]) {
  return w.panes.find((p) => p.active) ?? w.panes[0]
}

function onVisible() {
  // 手机切回前台时立刻校验（可能已超过空闲锁定时间）
  if (document.visibilityState === 'visible' && screen.value !== 'pair') refreshMe()
}

onMounted(() => {
  readPairFromHash()
  refreshMe()
  document.addEventListener('visibilitychange', onVisible)
})

onBeforeUnmount(() => {
  stopped = true
  closeEvents()
  document.removeEventListener('visibilitychange', onVisible)
})
</script>

<template>
  <div class="m-app">
    <div v-if="screen === 'loading'" class="center muted">…</div>

    <!-- 配对 -->
    <form v-else-if="screen === 'pair'" class="page form" @submit.prevent="pair">
      <div class="brand"><span class="logo mono">&gt;_</span>TmuxIDE</div>
      <h1>{{ t('connectTo', { host: me?.host ?? '' }) }}</h1>
      <p class="lead">{{ fromQr ? t('pairFromQr') : t('pairIntro') }}</p>
      <label class="field"><span>{{ t('code') }}</span>
        <input v-model="code" class="input mono code" autocomplete="one-time-code" autocapitalize="characters" spellcheck="false" maxlength="12" placeholder="XXXX-XXXX" required />
      </label>
      <label class="field"><span>{{ t('deviceName') }}</span>
        <input v-model="deviceName" class="input" maxlength="40" required />
        <small>{{ t('deviceNameHint') }}</small>
      </label>
      <p v-if="error" class="error" role="alert">{{ error }}</p>
      <button class="primary big" :disabled="busy || !code">{{ t('pairBtn') }}</button>
      <div class="note"><svg viewBox="0 0 24 24" aria-hidden="true"><rect x="4" y="11" width="16" height="10" rx="2" /><path d="M8 11V7a4 4 0 0 1 8 0v4" /></svg><span>{{ t('pairNote1') }}</span></div>
    </form>

    <!-- PIN -->
    <form v-else-if="screen === 'pin'" class="page form" @submit.prevent="unlock">
      <div class="brand"><span class="logo mono">&gt;_</span>{{ me?.host }}</div>
      <h1>{{ t('pinTitle') }}</h1>
      <p class="lead">{{ t('pinIntro') }}</p>
      <label class="field"><span class="sr-only">PIN</span>
        <input v-model="pin" class="input mono pin" type="password" inputmode="numeric" autocomplete="current-password" pattern="[0-9]*" maxlength="32" autofocus required />
      </label>
      <p v-if="error" class="error" role="alert">{{ error }}</p>
      <button class="primary big" :disabled="busy || pin.length < 6">{{ t('unlock') }}</button>
      <button type="button" class="link" @click="forget">{{ t('forget') }}</button>
    </form>

    <!-- 锁定 -->
    <div v-else-if="screen === 'locked'" class="page form">
      <div class="brand"><span class="logo mono">&gt;_</span>{{ me?.host }}</div>
      <h1>{{ t('lockedTitle') }}</h1>
      <p class="lead">{{ t('lockedIntro') }}</p>
      <button class="primary big" @click="refreshMe">{{ t('retry') }}</button>
    </div>

    <div v-else-if="screen === 'error'" class="page form">
      <h1>{{ t('error') }}</h1>
      <p class="error">{{ error }}</p>
      <button class="primary big" @click="refreshMe">{{ t('retry') }}</button>
    </div>

    <!-- 终端列表 -->
    <div v-else-if="screen === 'list'" class="list-page">
      <header class="bar">
        <span class="logo mono">&gt;_</span>
        <div class="title">
          <div class="host">{{ me?.host }}</div>
          <div class="state" :class="{ off: !live }"><span class="dot" />{{ live ? t('live') : t('offline') }}</div>
        </div>
        <a v-if="me?.admin" class="manage" href="/admin">{{ t('manage') }}</a>
        <button class="icon" :aria-label="t('lock')" @click="lockNow"><svg viewBox="0 0 24 24" aria-hidden="true"><rect x="4" y="11" width="16" height="10" rx="2" /><path d="M8 11V7a4 4 0 0 1 8 0v4" /></svg></button>
      </header>
      <main class="sessions">
        <p v-if="list?.error" class="error">{{ list.error }}</p>
        <p v-else-if="list && !list.sessions.length" class="empty muted">{{ t('noSessions') }}</p>
        <section v-for="s in list?.sessions ?? []" :key="s.name">
          <h2><span class="mono">{{ s.name }}</span>{{ t('windows', { n: s.windows.length }) }}<em v-if="s.attached">{{ t('usedOnDesktop') }}</em></h2>
          <button v-for="w in s.windows" :key="w.id" class="card" :class="{ active: w.active }" @click="open(s.name, w.id)">
            <div class="card-head">
              <span class="mono idx">{{ w.index }}</span>{{ w.name }}
              <span v-if="w.panes.length > 1" class="tag">{{ t('panes', { n: w.panes.length }) }}</span>
              <span class="cmd mono">{{ activePane(w)?.command }}</span>
            </div>
            <pre v-if="activePane(w)?.preview" class="preview">{{ activePane(w)?.preview }}</pre>
          </button>
        </section>
      </main>
      <footer class="foot">{{ t('footer', { device: me?.device ?? '' }) }} · <button class="link inline" @click="forget">{{ t('forget') }}</button></footer>
    </div>

    <TermView
      v-else-if="screen === 'term' && target"
      :session="target.session"
      :window="target.window"
      :windows="termSession?.windows ?? []"
      :read-only="!!me?.read_only"
      @back="closeTerm"
      @select="(w: string) => target && (target.window = w)"
      @auth-lost="termAuthLost"
    />
  </div>
</template>
