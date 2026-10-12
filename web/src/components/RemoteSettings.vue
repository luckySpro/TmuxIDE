<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from 'vue'
import Icon from './Icon.vue'
import { api, type RemoteConfig, type RemoteView } from '../api'
import { formatTime, t } from '../i18n'
import { toast } from '../store'

const view = ref<RemoteView | null>(null)
const draft = ref<RemoteConfig | null>(null)
const original = ref('')
const tokenInput = ref('')
const emails = ref('')
const busy = ref(false)
const pinOpen = ref(false)
const pin1 = ref('')
const pin2 = ref('')
let timer: number | undefined

const modes = [
  { id: 'cloudflare-quick', label: 'modeCfQuick', hint: 'modeCfQuickHint' },
  { id: 'cloudflare-token', label: 'modeCfToken', hint: 'modeCfTokenHint' },
  { id: 'tailscale-serve', label: 'modeTsServe', hint: 'modeTsServeHint' },
  { id: 'tailscale-direct', label: 'modeTsDirect', hint: 'modeTsDirectHint' },
  { id: 'local', label: 'modeLocal', hint: 'modeLocalHint' },
]

function parseEmails() {
  return emails.value.split(/[,，\s]+/).map((s) => s.trim()).filter(Boolean)
}

function draftJson() {
  return draft.value ? JSON.stringify({ ...draft.value, allowed_emails: parseEmails() }) : ''
}

const dirty = computed(() => !!draft.value && (draftJson() !== original.value || !!tokenInput.value))

async function load(resetDraft = false) {
  const v = await api.get<RemoteView>('/remote')
  view.value = v
  if (resetDraft || !draft.value || !dirty.value) {
    draft.value = structuredClone(v.config)
    emails.value = v.config.allowed_emails.join(', ')
    original.value = JSON.stringify(v.config)
  }
}

async function apply(cfg: RemoteConfig) {
  busy.value = true
  try {
    const v = await api.put<RemoteView>('/remote/config', {
      config: cfg,
      token: tokenInput.value ? tokenInput.value : null,
    })
    tokenInput.value = ''
    view.value = v
    draft.value = structuredClone(v.config)
    emails.value = v.config.allowed_emails.join(', ')
    original.value = JSON.stringify(v.config)
    if (v.status.state === 'error') toast(v.status.message, 'error')
    else toast(t('remoteSaved'))
  } catch (e) {
    toast(e instanceof Error ? e.message : String(e), 'error')
  } finally {
    busy.value = false
  }
}

function save() {
  if (draft.value) apply({ ...draft.value, allowed_emails: parseEmails() })
}

function toggleEnabled() {
  if (!draft.value) return
  apply({ ...draft.value, allowed_emails: parseEmails(), enabled: !draft.value.enabled })
}

async function restart() {
  busy.value = true
  try {
    view.value = await api.post<RemoteView>('/remote/restart')
  } catch (e) {
    toast(e instanceof Error ? e.message : String(e), 'error')
  } finally {
    busy.value = false
  }
}

async function savePin() {
  if (pin1.value !== pin2.value) {
    toast(t('pinMismatch'), 'error')
    return
  }
  try {
    await api.post('/remote/pin', { pin: pin1.value })
    toast(t('pinSaved'))
    pinOpen.value = false
    pin1.value = ''
    pin2.value = ''
    await load()
  } catch (e) {
    toast(e instanceof Error ? e.message : String(e), 'error')
  }
}

async function call(fn: () => Promise<unknown>) {
  try {
    await fn()
    await load()
  } catch (e) {
    toast(e instanceof Error ? e.message : String(e), 'error')
  }
}

const pair = () => call(() => api.post('/remote/pair'))
const cancelPair = () => call(() => api.del('/remote/pair'))
const setReadOnly = (id: string, ro: boolean) => call(() => api.patch(`/remote/devices/${id}`, { read_only: ro }))
function setAdmin(id: string, name: string, admin: boolean) {
  if (admin && !confirm(t('adminConfirm', { name }))) return
  call(() => api.patch(`/remote/devices/${id}`, { admin }))
}
const unlockDevice = (id: string) => call(() => api.patch(`/remote/devices/${id}`, { unlock: true }))
const disconnect = (id: number) => call(() => api.del(`/remote/connections/${id}`))
function removeDevice(id: string, name: string) {
  if (confirm(t('confirmRemoveDevice', { name }))) call(() => api.del(`/remote/devices/${id}`))
}

function viewing(deviceId: string) {
  const conns = view.value?.connections.filter((c) => c.device_id === deviceId) ?? []
  const term = conns.find((c) => c.kind === 'terminal')
  if (term) return { text: t('viewing', { target: term.target }), id: term.id }
  if (conns.length) return { text: t('viewingList'), id: conns[0].id }
  return null
}

const stateLabel = computed(() => {
  const s = view.value?.status.state
  return s === 'online' ? t('stOnline') : s === 'starting' ? t('stStarting') : s === 'error' ? t('stError') : t('stOff')
})

const idleOptions = [5, 15, 30, 60, 240, 720]
const dayOptions = [7, 30, 90, 365]

onMounted(() => {
  load(true).catch((e) => toast(String(e), 'error'))
  timer = window.setInterval(() => load().catch(() => {}), 3000)
})
onBeforeUnmount(() => window.clearInterval(timer))
</script>

<template>
  <div v-if="view && draft" class="remote">
    <div class="intro">
      <p>{{ t('remoteIntro') }}</p>
      <div class="master">
        <span>{{ t('enableRemote') }}</span>
        <button class="switch" role="switch" :aria-checked="view.config.enabled" :aria-label="t('enableRemote')" :disabled="busy" @click="toggleEnabled" />
      </div>
    </div>

    <div class="grid">
      <div class="col">
        <section class="card">
          <h2>{{ t('network') }}</h2>
          <div class="opts">
            <label v-for="m in modes" :key="m.id" class="opt" :class="{ on: draft.mode === m.id }">
              <input v-model="draft.mode" type="radio" :value="m.id" />
              <span><b>{{ t(m.label) }}</b><small>{{ t(m.hint) }}</small></span>
            </label>
          </div>

          <div v-if="draft.mode === 'cloudflare-token'" class="two">
            <label class="field"><span>{{ t('tunnelToken') }}</span>
              <input v-model="tokenInput" type="password" class="input" autocomplete="off" :placeholder="view.has_token ? t('savedSecret') : 'eyJh…'" />
              <small class="hint">{{ t('tunnelTokenHint') }}</small></label>
            <label class="field"><span>{{ t('publicHost') }}</span>
              <input v-model.trim="draft.public_host" class="input" placeholder="tmux.example.com" />
              <small class="hint">{{ t('publicHostHint', { port: draft.port }) }}</small></label>
          </div>
          <label v-if="draft.mode === 'cloudflare-quick'" class="field top"><span>{{ t('allowedEmails') }}</span>
            <input v-model="emails" class="input" placeholder="me@example.com" />
            <small class="hint">{{ t('allowedEmailsHint') }}</small></label>
          <div v-if="draft.mode === 'tailscale-serve'" class="two">
            <label class="field"><span>{{ t('tsHttpsPort') }}</span><input v-model.number="draft.tailscale_https_port" type="number" min="1" max="65535" class="input" /></label>
          </div>
          <label v-if="draft.mode === 'local'" class="field top"><span>{{ t('publicHost') }}</span>
            <input v-model.trim="draft.public_host" class="input" placeholder="tmux.example.com" />
            <small class="hint">{{ t('publicHostLocalHint') }}</small></label>
          <p v-if="draft.mode.startsWith('tailscale') && !view.tailscale.found" class="warn-line"><Icon name="warn" />{{ t('tsMissing') }}</p>
          <label class="field top narrow"><span>{{ t('remotePort') }}</span>
            <input v-model.number="draft.port" type="number" min="1024" max="65535" class="input short" />
            <small class="hint">{{ t('remotePortHint') }}</small></label>

          <div class="status-line" :class="view.status.state">
            <span class="dot" />
            <span>{{ stateLabel }}</span>
            <a v-if="view.status.url" class="mono url" :href="view.status.url" target="_blank" rel="noreferrer">{{ view.status.url }}</a>
            <span v-if="view.status.message && view.status.state !== 'online'" class="msg">{{ view.status.message }}</span>
            <span class="grow" />
            <span v-if="draft.mode.startsWith('cloudflare')" class="hint">{{ view.cloudflared.bundled ? t('cfBuiltin') : view.cloudflared.found ? t('cfSystem') : t('cfMissing') }}</span>
            <button v-if="view.config.enabled" class="btn small" :disabled="busy" @click="restart">{{ t('reconnect') }}</button>
          </div>
        </section>

        <section class="card">
          <h2>{{ t('security') }}</h2>
          <div class="toggle first"><div><div>{{ t('pinLabel') }}</div><div class="hint">{{ view.has_pin ? t('pinHint') : t('pinNotSet') }}</div></div>
            <div class="row-actions">
              <button class="btn small" @click="pinOpen = !pinOpen">{{ view.has_pin ? t('changePin') : t('setPin') }}</button>
              <button class="switch" role="switch" :aria-checked="draft.require_pin" :aria-label="t('pinLabel')" @click="draft.require_pin = !draft.require_pin" />
            </div>
          </div>
          <form v-if="pinOpen" class="pin-form" @submit.prevent="savePin">
            <input v-model="pin1" type="password" inputmode="numeric" class="input mono" :placeholder="t('pinPrompt')" autocomplete="new-password" />
            <input v-model="pin2" type="password" inputmode="numeric" class="input mono" :placeholder="t('pinPromptAgain')" autocomplete="new-password" />
            <button class="btn primary" :disabled="pin1.length < 6">{{ t('confirm') }}</button>
          </form>
          <div class="toggle"><div><div>{{ t('idleLock') }}</div><div class="hint">{{ t('idleLockHint') }}</div></div>
            <select v-model.number="draft.idle_minutes" class="input select" :aria-label="t('idleLock')">
              <option v-for="m in idleOptions" :key="m" :value="m">{{ m < 60 ? t('minutes', { n: m }) : t('hours', { n: m / 60 }) }}</option>
            </select></div>
          <div class="toggle"><div><div>{{ t('deviceExpiry') }}</div><div class="hint">{{ t('deviceExpiryHint') }}</div></div>
            <select v-model.number="draft.device_days" class="input select" :aria-label="t('deviceExpiry')">
              <option v-for="d in dayOptions" :key="d" :value="d">{{ t('daysUnused', { n: d }) }}</option>
            </select></div>
          <div class="toggle"><div><div>{{ t('notifyLabel') }}</div><div class="hint">{{ t('notifyHint') }}</div></div>
            <button class="switch" role="switch" :aria-checked="draft.notify" :aria-label="t('notifyLabel')" @click="draft.notify = !draft.notify" /></div>
          <div class="toggle"><div><div>{{ t('keepAwake') }}</div><div class="hint">{{ t('keepAwakeHint') }}</div></div>
            <button class="switch" role="switch" :aria-checked="draft.keep_awake" :aria-label="t('keepAwake')" @click="draft.keep_awake = !draft.keep_awake" /></div>
        </section>

        <div class="save-row">
          <button class="btn" :disabled="!dirty" @click="load(true)">{{ t('discard') }}</button>
          <button class="btn primary" :disabled="!dirty || busy" @click="save">{{ t('saveSettings') }}</button>
        </div>
      </div>

      <div class="col">
        <section class="card">
          <h2>{{ t('pairNew') }}<button v-if="view.pairing" class="btn small" @click="cancelPair">{{ t('pairCancel') }}</button></h2>
          <div v-if="view.pairing" class="pairing">
            <!-- 二维码由本机服务生成（SVG），内容是远程地址 + 一次性配对码 -->
            <div v-if="view.pairing.qr_svg" class="qr" v-html="view.pairing.qr_svg" />
            <div>
              <p class="hint">{{ view.pairing.qr_svg ? t('pairScan') : t('pairNoUrl') }}</p>
              <div class="code mono">{{ view.pairing.code }}</div>
              <div class="hint">{{ t('pairExpires', { s: view.pairing.expires_in }) }}</div>
              <ul><li>{{ t('pairBullet1') }}</li><li>{{ t('pairBullet2') }}</li></ul>
            </div>
          </div>
          <button v-else class="btn primary" :disabled="!view.config.enabled" @click="pair"><Icon name="phone" />{{ t('pairNew') }}</button>
        </section>

        <section class="card">
          <h2>{{ t('devices') }}<span class="hint">{{ view.devices.length }}</span></h2>
          <p v-if="!view.devices.length" class="hint">{{ t('noDevices') }}</p>
          <div v-for="d in view.devices" :key="d.id" class="dev">
            <span class="dev-icon" :class="{ on: d.online }"><Icon name="phone" :size="16" /></span>
            <div class="dev-main">
              <div class="dev-name">{{ d.name }}
                <span v-if="d.online" class="badge">{{ t('online') }}</span>
                <span v-if="d.locked" class="badge bad">{{ t('lockedDev') }}</span>
              </div>
              <div class="hint">{{ d.read_only ? t('readOnlyDev') : d.admin ? t('adminDev') : t('controlDev') }} · {{ viewing(d.id)?.text ?? t('lastUsed', { t: formatTime(d.last_seen) }) }}<template v-if="d.last_ip"> · {{ d.last_ip }}</template></div>
            </div>
            <div class="dev-actions">
              <button v-if="d.locked" class="btn small" @click="unlockDevice(d.id)">{{ t('unlockDevice') }}</button>
              <button v-if="viewing(d.id)" class="btn small" @click="disconnect(viewing(d.id)!.id)">{{ t('disconnect') }}</button>
              <button class="btn small" @click="setReadOnly(d.id, !d.read_only)">{{ d.read_only ? t('setControl') : t('setReadOnly') }}</button>
              <button class="btn small" @click="setAdmin(d.id, d.name, !d.admin)">{{ d.admin ? t('unsetAdmin') : t('setAdmin') }}</button>
              <button class="btn small danger" @click="removeDevice(d.id, d.name)">{{ t('removeDevice') }}</button>
            </div>
          </div>
        </section>

        <section class="card">
          <h2>{{ t('auditTitle') }}</h2>
          <p v-if="!view.audit.length" class="hint">{{ t('noAudit') }}</p>
          <div class="audit scroll">
            <div v-for="e in view.audit" :key="e.seq" class="log" :class="{ bad: ['pair_fail', 'unlock_fail', 'locked', 'error'].includes(e.kind) }">
              <span class="mono time">{{ formatTime(e.ts) }}</span>
              <span class="who">{{ e.device ?? '—' }}</span>
              <span class="what">{{ e.detail }}<template v-if="e.ip"> · {{ e.ip }}</template></span>
            </div>
          </div>
        </section>

        <section class="card warn-card">
          <h2><Icon name="warn" />{{ t('remoteOnlyTerm') }}</h2>
          <p>{{ t('remoteOnlyTermText') }}</p>
        </section>
      </div>
    </div>
  </div>
</template>

<style scoped>
.intro { display: flex; align-items: center; justify-content: space-between; gap: 24px; margin: -12px 0 20px; }
.intro p { margin: 0; color: var(--text-2); font-size: 14px; }
.master { display: flex; align-items: center; gap: 12px; font-size: 13px; color: var(--text-2); white-space: nowrap; }
.grid { display: grid; grid-template-columns: minmax(0, 1.25fr) minmax(0, 1fr); gap: 24px; align-items: start; }
.col { display: flex; flex-direction: column; gap: 20px; min-width: 0; }
.opts { display: grid; grid-template-columns: repeat(3, minmax(0, 1fr)); gap: 8px; margin-bottom: 14px; }
.opt { display: flex; gap: 10px; align-items: flex-start; padding: 10px 12px; border-radius: 8px; border: 1px solid var(--border-2); cursor: pointer; }
.opt.on { border-color: var(--accent); background: #16201a; }
.opt input { margin-top: 3px; accent-color: var(--accent); }
.opt b { display: block; font-size: 13.5px; font-weight: 500; }
.opt small { display: block; font-size: 12px; color: var(--text-2); line-height: 1.45; }
.two { display: grid; grid-template-columns: minmax(0, 1fr) minmax(0, 1fr); gap: 16px; margin-bottom: 6px; }
.field small { display: block; margin-top: 5px; }
.top { margin-top: 10px; }
.short { max-width: 160px; }
.select { width: auto; min-width: 150px; height: 32px; }
.warn-line { display: flex; align-items: center; gap: 8px; color: var(--amber); font-size: 13px; margin: 10px 0 0; }
.status-line { margin-top: 16px; display: flex; align-items: center; gap: 10px; flex-wrap: wrap; padding: 10px 12px; border-radius: 8px; background: var(--panel-3); border: 1px solid var(--border); font-size: 13px; }
.status-line .dot { width: 8px; height: 8px; border-radius: 50%; background: var(--muted); }
.status-line.online { background: #0f1a13; border-color: #1f3a28; }
.status-line.online .dot { background: var(--accent); }
.status-line.starting .dot { background: var(--amber); }
.status-line.error { background: #22140f; border-color: #4a2a20; }
.status-line.error .dot { background: var(--red); }
.status-line .url { color: #86efac; font-size: 12.5px; word-break: break-all; }
.status-line .msg { color: var(--text-2); font-size: 12.5px; }
.grow { flex: 1; }
.toggle { display: flex; align-items: center; justify-content: space-between; gap: 16px; padding: 12px 0; border-top: 1px solid #222831; font-size: 13.5px; }
.toggle.first { border-top: 0; padding-top: 0; }
.row-actions { display: flex; align-items: center; gap: 10px; }
.pin-form { display: grid; grid-template-columns: 1fr 1fr auto; gap: 8px; padding: 0 0 12px; }
.save-row { display: flex; justify-content: flex-end; gap: 10px; }
h2 { display: flex; align-items: center; justify-content: space-between; gap: 8px; }
.pairing { display: flex; gap: 18px; align-items: center; }
.qr { flex-shrink: 0; width: 170px; height: 170px; padding: 8px; background: #fff; border-radius: 8px; line-height: 0; }
.qr :deep(svg) { width: 100%; height: 100%; }
.code { font-size: 26px; font-weight: 600; letter-spacing: .08em; color: var(--accent); margin: 8px 0 6px; }
.pairing ul { margin: 12px 0 0; padding-left: 18px; font-size: 12.5px; color: var(--text-2); line-height: 1.7; }
.dev { display: grid; grid-template-columns: 32px minmax(0, 1fr); gap: 4px 12px; align-items: center; padding: 11px 0; border-top: 1px solid #222831; }
.dev:first-of-type { border-top: 0; }
.dev-icon { width: 32px; height: 32px; border-radius: 8px; background: var(--panel-2); color: var(--text-2); display: flex; align-items: center; justify-content: center; }
.dev-icon.on { background: var(--accent-soft); color: var(--accent); }
.dev-name { font-size: 13.5px; font-weight: 500; display: flex; align-items: center; gap: 8px; }
.badge.bad { background: #3a1f1c; color: var(--del-ink); }
.dev-actions { grid-column: 2; display: flex; gap: 6px; flex-wrap: wrap; margin-top: 4px; }
.audit { max-height: 260px; overflow-y: auto; }
.log { display: grid; grid-template-columns: 82px 110px minmax(0, 1fr); gap: 10px; font-size: 12.5px; padding: 5px 0; }
.log .time { color: var(--muted); }
.log .who { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.log .what { color: var(--text-2); }
.log.bad .time, .log.bad .who, .log.bad .what { color: var(--amber); }
.warn-card { border-color: #5a4520; background: #1c1810; }
.warn-card h2 { justify-content: flex-start; color: var(--amber); }
.warn-card p { margin: 0; font-size: 13px; line-height: 1.7; color: #d9cbb0; }
@media (max-width: 1200px) { .grid { grid-template-columns: minmax(0, 1fr); } .opts { grid-template-columns: repeat(2, minmax(0, 1fr)); } }
</style>
