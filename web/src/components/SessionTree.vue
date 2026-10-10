<script setup lang="ts">
import { computed, ref } from 'vue'
import Icon from './Icon.vue'
import Modal from './Modal.vue'
import NewSessionDialog from './NewSessionDialog.vue'
import { api, enc, type RestoreResult, type SavedEntry, type SessionInfo } from '../api'
import { formatTime, t } from '../i18n'
import { act, state } from '../store'

const localSaved = computed(() => state.saved.filter((s) => s.local))
const otherSaved = computed(() => state.saved.filter((s) => !s.local))

type Pending =
  | { kind: 'kill'; name: string }
  | { kind: 'rename'; name: string; to: string }
  | { kind: 'delete'; entry: SavedEntry }
  | { kind: 'deleteTpl'; name: string }
const pending = ref<Pending | null>(null)
const fromTemplate = ref<string | null>(null)

const paneCount = (s: SessionInfo) => s.windows.reduce((n, w) => n + w.panes.length, 0)

function restore(e: SavedEntry) {
  act(() => api.post<RestoreResult>('/restore', { host: e.host, name: e.name, mode: 'rename', run_commands: true }), (r) => {
    state.selected = r.session
    return t('restoredOk', { name: r.session }) + (r.report?.warnings.length ? ` · ${r.report.warnings.join('；')}` : '')
  })
}

function openHistory(host: string, name: string) {
  state.historyTarget = { host, name }
  state.view = 'history'
}

async function confirm() {
  const p = pending.value
  if (!p) return
  pending.value = null
  if (p.kind === 'kill') await act(() => api.del(`/sessions/${enc(p.name)}`))
  if (p.kind === 'rename') {
    const r = await act(() => api.post(`/sessions/${enc(p.name)}/rename`, { to: p.to }))
    if (r !== undefined && state.selected === p.name) state.selected = p.to
  }
  if (p.kind === 'delete') await act(() => api.del(`/saved/${enc(p.entry.host)}/${enc(p.entry.name)}`))
  if (p.kind === 'deleteTpl') await act(() => api.del(`/templates/${enc(p.name)}`))
}
</script>

<template>
  <nav class="tree scroll" :aria-label="t('session')">
    <div class="sec section-title">{{ t('running') }} <span class="count on">{{ state.sessions.length }}</span></div>
    <div v-if="!state.sessions.length" class="empty">{{ t('none') }}</div>
    <div v-for="s in state.sessions" :key="s.id" class="row" :class="{ selected: s.name === state.selected }">
      <button class="row-btn" @click="state.selected = s.name; state.view = 'workspace'">
        <span class="dot on" /><span class="mono name">{{ s.name }}</span>
        <span class="meta">{{ s.windows.length }} {{ t('windows') }} · {{ paneCount(s) }} {{ t('panes') }}</span>
      </button>
      <span class="actions">
        <button class="icon-btn" :aria-label="t('rename')" :title="t('rename')" @click="pending = { kind: 'rename', name: s.name, to: s.name }"><Icon name="edit" :size="13" /></button>
        <button class="icon-btn" :aria-label="t('close')" :title="t('close')" @click="pending = { kind: 'kill', name: s.name }"><Icon name="power" :size="13" /></button>
      </span>
    </div>

    <div class="sec section-title">{{ t('saved') }} <span class="count">{{ state.saved.length }}</span></div>
    <div v-if="!state.saved.length" class="empty">{{ t('none') }}</div>
    <template v-for="group in [localSaved, otherSaved]" :key="group === localSaved ? 'l' : 'o'">
      <div v-for="e in group" :key="e.host + '/' + e.name" class="row">
        <button class="row-btn" :title="t('viewHistory')" @click="openHistory(e.host, e.name)">
          <span class="dot" :class="e.running ? 'on' : 'off'" /><span class="mono name">{{ e.name }}</span>
          <span class="meta">{{ e.local ? formatTime(e.saved_at) : t('fromHost', { host: e.host }) }}</span>
        </button>
        <span class="actions">
          <button class="icon-btn" :aria-label="t('restore')" :title="t('restore')" @click="restore(e)"><Icon name="restore" :size="13" /></button>
          <button class="icon-btn" :aria-label="t('delete')" :title="t('delete')" @click="pending = { kind: 'delete', entry: e }"><Icon name="trash" :size="13" /></button>
        </span>
      </div>
    </template>

    <div class="sec section-title">{{ t('templates') }} <span class="count">{{ state.templates.length }}</span></div>
    <div v-if="!state.templates.length" class="empty">{{ t('none') }}</div>
    <div v-for="tp in state.templates" :key="tp.name" class="row">
      <button class="row-btn" :title="t('newSession')" @click="fromTemplate = tp.name">
        <Icon name="layers" :size="14" class="tpl" /><span class="mono name">{{ tp.name }}</span>
        <span class="meta">{{ tp.windows.join(' + ') }}</span>
      </button>
      <span class="actions">
        <button class="icon-btn" :aria-label="t('delete')" :title="t('delete')" @click="pending = { kind: 'deleteTpl', name: tp.name }"><Icon name="trash" :size="13" /></button>
      </span>
    </div>

    <div class="grow" />
    <div class="autosave">
      <div class="title"><span class="dot" :class="state.autosaveMinutes ? 'on' : 'off'" />{{ state.autosaveMinutes ? t('autosaveOn') : t('autosaveOff') }}</div>
      <span v-if="state.autosaveMinutes">{{ t('autosaveEvery', { n: state.autosaveMinutes }) }}</span>
    </div>

    <Modal v-if="pending?.kind === 'kill'" :title="t('close')" danger :confirm-text="t('close')" @confirm="confirm" @cancel="pending = null">
      {{ t('confirmKill', { name: pending.name }) }}
    </Modal>
    <Modal v-if="pending?.kind === 'rename'" :title="t('rename')" @confirm="confirm" @cancel="pending = null">
      <label class="field"><span>{{ t('name') }}</span><input v-model="pending.to" class="input" /></label>
    </Modal>
    <Modal v-if="pending?.kind === 'delete'" :title="t('delete')" danger :confirm-text="t('delete')" @confirm="confirm" @cancel="pending = null">
      {{ t('confirmDelete', { name: `${pending.entry.host}/${pending.entry.name}` }) }}
    </Modal>
    <Modal v-if="pending?.kind === 'deleteTpl'" :title="t('delete')" danger :confirm-text="t('delete')" @confirm="confirm" @cancel="pending = null">
      {{ t('confirmDeleteTpl', { name: pending.name }) }}
    </Modal>
    <NewSessionDialog v-if="fromTemplate" :template="fromTemplate" @close="fromTemplate = null" />
  </nav>
</template>

<style scoped>
.tree { width: 260px; flex-shrink: 0; background: var(--panel); border-right: 1px solid var(--border); display: flex; flex-direction: column; padding-bottom: 12px; }
.sec { padding: 14px 16px 6px; display: flex; align-items: center; justify-content: space-between; }
.count { letter-spacing: 0; font-weight: 500; }
.count.on { color: var(--accent); }
.empty { padding: 4px 16px 8px; font-size: 12.5px; color: var(--muted); }
.row { position: relative; display: flex; align-items: center; }
.row:hover { background: var(--panel-2); }
.row.selected { background: var(--accent-soft); box-shadow: inset 2px 0 0 var(--accent); }
.row-btn { flex: 1; min-width: 0; display: flex; align-items: center; gap: 10px; height: 34px; padding: 0 16px; border: 0; background: none; text-align: left; cursor: pointer; color: #d5dae0; font-size: 13.5px; }
.row.selected .row-btn { color: #fff; }
.name { white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
.meta { margin-left: auto; font-size: 11.5px; color: var(--muted); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; max-width: 120px; }
.actions { position: absolute; right: 8px; display: none; gap: 2px; background: var(--panel-2); }
.row.selected .actions { background: var(--accent-soft); }
.row:hover .actions, .row:focus-within .actions { display: flex; }
.tpl { color: var(--blue); flex-shrink: 0; }
.grow { flex: 1; min-height: 12px; }
.autosave { margin: 0 12px; padding: 12px; border-radius: 8px; background: var(--panel-2); border: 1px solid var(--border); font-size: 12px; color: var(--text-2); }
.autosave .title { display: flex; align-items: center; gap: 6px; color: var(--text); font-weight: 500; margin-bottom: 2px; }
.autosave .dot { width: 6px; height: 6px; }
</style>
