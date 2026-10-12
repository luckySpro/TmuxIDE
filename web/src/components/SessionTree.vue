<script setup lang="ts">
import { computed, ref } from 'vue'
import Icon from './Icon.vue'
import Modal from './Modal.vue'
import NewSessionDialog from './NewSessionDialog.vue'
import { api, enc, type RestoreResult, type SavedEntry, type SessionInfo } from '../api'
import { formatTime, t } from '../i18n'
import { act, groupNames, state } from '../store'

const localSaved = computed(() => state.saved.filter((s) => s.local))
const otherSaved = computed(() => state.saved.filter((s) => !s.local))

/** 运行中的会话按分组显示，未分组的放在最后 */
const grouped = computed(() => {
  const out: { group: string; sessions: SessionInfo[] }[] = []
  for (const g of groupNames()) out.push({ group: g, sessions: state.sessions.filter((s) => s.group === g) })
  const rest = state.sessions.filter((s) => !s.group)
  if (rest.length) out.push({ group: '', sessions: rest })
  return out
})
const collapsed = ref<Record<string, boolean>>({})

type Pending =
  | { kind: 'group'; name: string; group: string }
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
  if (p.kind === 'group') {
    const group = p.group.trim()
    await act(() => api.put(`/sessions/${enc(p.name)}/group`, { group }), () =>
      group ? t('groupSet', { name: p.name, group }) : t('groupCleared', { name: p.name }))
  }
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
    <template v-for="g in grouped" :key="g.group || '\u0000'">
    <button v-if="grouped.length > 1 || g.group" class="group" :aria-expanded="!collapsed[g.group]" @click="collapsed[g.group] = !collapsed[g.group]">
      <span class="caret" :class="{ closed: collapsed[g.group] }">▾</span><Icon name="folder" :size="13" />
      <span class="gname">{{ g.group || t('ungrouped') }}</span><span class="gcount">{{ g.sessions.length }}</span>
    </button>
    <div v-for="s in collapsed[g.group] ? [] : g.sessions" :key="s.id" class="row" :class="{ selected: s.name === state.selected, nested: grouped.length > 1 || g.group }">
      <button class="row-btn" @click="state.selected = s.name; state.view = 'workspace'">
        <span class="dot on" /><span class="mono name">{{ s.name }}</span>
        <span class="meta">{{ s.windows.length }} {{ t('windows') }} · {{ paneCount(s) }} {{ t('panes') }}</span>
      </button>
      <span class="actions">
        <button class="icon-btn" :aria-label="t('setGroup')" :title="t('setGroup')" @click="pending = { kind: 'group', name: s.name, group: s.group }"><Icon name="folder" :size="13" /></button>
        <button class="icon-btn" :aria-label="t('rename')" :title="t('rename')" @click="pending = { kind: 'rename', name: s.name, to: s.name }"><Icon name="edit" :size="13" /></button>
        <button class="icon-btn" :aria-label="t('close')" :title="t('close')" @click="pending = { kind: 'kill', name: s.name }"><Icon name="power" :size="13" /></button>
      </span>
    </div>
    </template>

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

    <Modal v-if="pending?.kind === 'group'" :title="`${t('setGroup')} · ${pending.name}`" @confirm="confirm" @cancel="pending = null">
      <label class="field"><span>{{ t('groupName') }}</span><input v-model="pending.group" class="input" list="tmx-groups" maxlength="32" /></label>
      <datalist id="tmx-groups"><option v-for="g in groupNames()" :key="g" :value="g" /></datalist>
      <div v-if="groupNames().length" class="pick">
        <button v-for="g in groupNames()" :key="g" type="button" class="chip" :class="{ on: pending.group === g }" @click="pending.group = g">{{ g }}</button>
        <button type="button" class="chip" :class="{ on: !pending.group }" @click="pending.group = ''">{{ t('ungrouped') }}</button>
      </div>
      <p class="hint">{{ t('groupHint') }}</p>
    </Modal>
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
.group { display: flex; align-items: center; gap: 6px; width: 100%; height: 28px; padding: 0 12px; border: 0; background: none; color: var(--text-2); font-size: 12.5px; cursor: pointer; text-align: left; }
.group:hover { color: var(--text); }
.caret { width: 10px; font-size: 10px; transition: transform .15s; }
.caret.closed { transform: rotate(-90deg); }
.gname { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.gcount { margin-left: auto; font-size: 11px; color: var(--muted); }
.row.nested .row-btn { padding-left: 30px; }
.pick { display: flex; flex-wrap: wrap; gap: 6px; margin-top: 10px; }
.chip { height: 26px; padding: 0 10px; border-radius: 13px; border: 1px solid var(--border); background: none; color: var(--text-2); font-size: 12px; cursor: pointer; }
.chip.on { border-color: var(--accent); background: var(--accent-soft); color: #fff; }
.hint { margin: 10px 0 0; font-size: 12px; color: var(--muted); }
.grow { flex: 1; min-height: 12px; }
.autosave { margin: 0 12px; padding: 12px; border-radius: 8px; background: var(--panel-2); border: 1px solid var(--border); font-size: 12px; color: var(--text-2); }
.autosave .title { display: flex; align-items: center; gap: 6px; color: var(--text); font-weight: 500; margin-bottom: 2px; }
.autosave .dot { width: 6px; height: 6px; }
</style>
