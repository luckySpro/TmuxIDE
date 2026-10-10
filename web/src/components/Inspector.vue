<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import Modal from './Modal.vue'
import { api, enc, type CommitInfo, type SaveResult, type SessionInfo } from '../api'
import { formatTime, t } from '../i18n'
import { act, state } from '../store'

const props = defineProps<{ session: SessionInfo }>()
const commits = ref<CommitInfo[]>([])
const unpushed = ref(new Set<string>())
const tplName = ref<string | null>(null)

const win = computed(() => props.session.windows.find((w) => w.active) ?? props.session.windows[0])
const panes = computed(() => props.session.windows.reduce((n, w) => n + w.panes.length, 0))
const activePane = computed(() => win.value?.panes.find((p) => p.active) ?? win.value?.panes[0])
const totalWidth = computed(() => Math.max(1, ...(win.value?.panes.map((p) => p.width) ?? [1])))

async function loadHistory() {
  if (!state.info) return
  try {
    const [hist, log] = await Promise.all([
      api.get<CommitInfo[]>(`/history/${enc(state.info.host)}/${enc(props.session.name)}`),
      api.get<CommitInfo[]>('/git/log'),
    ])
    commits.value = hist
    // 全局日志中最新的 ahead 个提交尚未推送
    unpushed.value = new Set(log.slice(0, state.git?.ahead ?? 0).map((c) => c.id))
  } catch {
    commits.value = []
  }
}
watch(() => [props.session.name, state.saved, state.git, state.info], loadHistory, { immediate: true })

function save() {
  act(() => api.post<SaveResult>('/save', { sessions: [props.session.name] }), (r) =>
    r.saved.length ? t('savedOk', { list: r.saved.join(', ') }) + (r.pushed ? t('pushed') : '') : t('savedNone'))
}

function saveTemplate() {
  const name = tplName.value
  tplName.value = null
  if (name) act(() => api.post('/templates', { session: props.session.name, name }), () => t('templateSaved', { name }))
}

function openHistory() {
  if (!state.info) return
  state.historyTarget = { host: state.info.host, name: props.session.name }
  state.view = 'history'
}
</script>

<template>
  <aside class="inspector scroll" :aria-label="t('session')">
    <section>
      <div class="section-title">{{ t('session') }}</div>
      <div class="title mono">{{ session.name }}</div>
      <dl>
        <dt>{{ t('windows') }}</dt><dd>{{ session.windows.length }} · {{ panes }} {{ t('panes') }}</dd>
        <dt>{{ t('directory') }}</dt><dd class="mono ellipsis">{{ activePane?.cwd }}</dd>
        <dt>{{ t('created') }}</dt><dd>{{ formatTime(session.created) }}</dd>
      </dl>
    </section>
    <section v-if="win">
      <div class="section-title">{{ t('layout') }} · {{ win.name }}</div>
      <div class="layout">
        <div v-for="p in win.panes" :key="p.id" class="pane mono" :class="{ active: p.active }"
          :style="{ flexBasis: `${(p.width / totalWidth) * 100}%`, minHeight: `${Math.max(28, p.height)}px` }">{{ p.command }}</div>
      </div>
      <dl>
        <dt>{{ t('panes') }}</dt><dd class="mono">{{ activePane?.id }} · {{ activePane?.width }}×{{ activePane?.height }}</dd>
        <dt>{{ t('command') }}</dt><dd class="mono ellipsis">{{ activePane?.command }}</dd>
        <dt>PID</dt><dd class="mono">{{ activePane?.pid }}</dd>
      </dl>
    </section>
    <section class="history">
      <div class="section-title row">{{ t('history') }}<span class="branch">Git · {{ state.git?.branch }}</span></div>
      <div v-if="!commits.length" class="muted small">{{ t('noHistory') }}</div>
      <ul>
        <li v-for="(c, i) in commits.slice(0, 6)" :key="c.id">
          <span class="dot" :class="i === 0 ? 'on' : 'off'" />
          <div>
            <div class="summary">{{ formatTime(c.time) }} · {{ c.summary.split(' @ ')[0] }}</div>
            <div class="mono muted small">{{ c.short }}<template v-if="state.git?.has_remote"> · {{ unpushed.has(c.id) ? t('unpushed') : t('pushedTag') }}</template></div>
          </div>
        </li>
      </ul>
      <div class="grow" />
      <div class="buttons">
        <button class="btn" @click="save">{{ t('saveSnapshot') }}</button>
        <button class="btn" :disabled="!commits.length" @click="openHistory">{{ t('viewHistory') }}</button>
      </div>
      <button class="btn wide" @click="tplName = session.name">{{ t('saveAsTemplate') }}</button>
    </section>
    <Modal v-if="tplName !== null" :title="t('saveAsTemplate')" @confirm="saveTemplate" @cancel="tplName = null">
      <label class="field"><span>{{ t('templateName') }}</span><input v-model="tplName" class="input" /></label>
    </Modal>
  </aside>
</template>

<style scoped>
.inspector { width: 300px; flex-shrink: 0; background: var(--panel); border-left: 1px solid var(--border); display: flex; flex-direction: column; }
section { padding: 16px; border-bottom: 1px solid var(--border); }
.section-title { margin-bottom: 10px; }
.section-title.row { display: flex; justify-content: space-between; }
.branch { letter-spacing: 0; font-weight: 400; text-transform: none; }
.title { font-size: 18px; font-weight: 600; margin-bottom: 12px; }
dl { display: grid; grid-template-columns: 72px minmax(0, 1fr); row-gap: 8px; margin: 0; font-size: 12.5px; }
dt { color: var(--muted); }
dd { margin: 0; }
.ellipsis { white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
.layout { display: flex; flex-wrap: wrap; gap: 4px; margin-bottom: 12px; max-height: 120px; overflow: hidden; }
.pane { flex-grow: 1; flex-shrink: 1; max-height: 56px; border: 1px solid #343c47; border-radius: 4px; font-size: 11px; color: var(--text-2); display: flex; align-items: center; justify-content: center; overflow: hidden; padding: 0 4px; }
.pane.active { border: 1.5px solid var(--accent); background: var(--accent-soft); color: #86efac; }
.history { flex: 1; display: flex; flex-direction: column; gap: 10px; border-bottom: 0; }
ul { list-style: none; margin: 0; padding: 0; display: flex; flex-direction: column; gap: 10px; }
li { display: flex; gap: 10px; align-items: flex-start; }
li .dot { margin-top: 6px; }
.summary { font-size: 13px; }
.small { font-size: 11.5px; }
.grow { flex: 1; }
.buttons { display: flex; gap: 8px; }
.buttons .btn { flex: 1; }
.wide { width: 100%; }
</style>
