<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref } from 'vue'
import Icon from './Icon.vue'
import NewSessionDialog from './NewSessionDialog.vue'
import { api, type RestoreResult, type SaveResult } from '../api'
import { t } from '../i18n'
import { act, state } from '../store'

const showNew = ref(false)
const query = ref('')
const open = ref(false)
const active = ref(0)
const input = ref<HTMLInputElement>()

function saveAll() {
  return act(() => api.post<SaveResult>('/save'), (r) =>
    r.saved.length ? t('savedOk', { list: r.saved.join(', ') }) + (r.pushed ? t('pushed') : '') + (r.push_error ? ` · ${r.push_error}` : '') : t('savedNone'))
}
function restoreAll() {
  return act(() => api.post<RestoreResult[]>('/restore-all'), (r) => {
    const done = r.filter((x) => !x.skipped).map((x) => x.session)
    return done.length ? t('restoredOk', { name: done.join(', ') }) : t('savedNone')
  })
}
function sync() {
  return act(() => api.post('/git/sync'), () => t('syncOk'))
}

interface Cmd { label: string; run: () => void }
const commands = computed<Cmd[]>(() => {
  const list: Cmd[] = [
    ...state.sessions.map((s) => ({ label: t('switchTo', { name: s.name }), run: () => { state.selected = s.name; state.view = 'workspace' } })),
    { label: t('newSession'), run: () => (showNew.value = true) },
    { label: t('saveSnapshot'), run: saveAll },
    { label: t('restoreAll'), run: restoreAll },
    { label: `${t('sync')} · ${t('pullPush')}`, run: sync },
    { label: t('settings'), run: () => (state.view = 'settings') },
  ]
  const q = query.value.trim().toLowerCase()
  return q ? list.filter((c) => c.label.toLowerCase().includes(q)) : list
})

function run(c?: Cmd) {
  if (!c) return
  c.run()
  query.value = ''
  open.value = false
  input.value?.blur()
}

function onKey(e: KeyboardEvent) {
  if (e.key === 'ArrowDown') { active.value = Math.min(active.value + 1, commands.value.length - 1); e.preventDefault() }
  else if (e.key === 'ArrowUp') { active.value = Math.max(active.value - 1, 0); e.preventDefault() }
  else if (e.key === 'Enter') run(commands.value[active.value])
  else if (e.key === 'Escape') { open.value = false; input.value?.blur() }
  else active.value = 0
}

// ⌘K / Ctrl+K 只在终端以外的地方或显式快捷键时打开命令面板
function globalKey(e: KeyboardEvent) {
  if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === 'k') {
    e.preventDefault()
    input.value?.focus()
  }
}
onMounted(() => window.addEventListener('keydown', globalKey, true))
onUnmounted(() => window.removeEventListener('keydown', globalKey, true))
</script>

<template>
  <header class="topbar">
    <div class="brand"><span class="logo mono">&gt;_</span>TmuxIDE</div>
    <div class="sep" />
    <button class="btn primary" :disabled="state.busy" @click="showNew = true"><Icon name="plus" />{{ t('newSession') }}</button>
    <button class="btn" :disabled="state.busy" @click="saveAll"><Icon name="save" />{{ t('saveSnapshot') }}</button>
    <button class="btn" :disabled="state.busy" @click="restoreAll"><Icon name="restore" />{{ t('restoreAll') }}</button>
    <button class="btn" :disabled="state.busy || !state.git?.has_remote" :title="state.git?.has_remote ? '' : t('notSynced')" @click="sync">
      <Icon name="sync" />{{ t('sync') }}
      <span v-if="state.git?.ahead" class="badge">↑{{ state.git.ahead }}</span>
      <span v-if="state.git?.behind" class="badge">↓{{ state.git.behind }}</span>
    </button>
    <div class="grow" />
    <div class="palette">
      <label class="search">
        <Icon name="search" />
        <input ref="input" v-model="query" type="text" :placeholder="t('search')" :aria-label="t('search')"
          @focus="open = true" @blur="open = false" @keydown="onKey" />
        <span class="kbd mono">⌘K</span>
      </label>
      <ul v-if="open && commands.length" class="menu" role="listbox">
        <li v-for="(c, i) in commands" :key="c.label" role="option" :aria-selected="i === active" :class="{ active: i === active }"
          @mousedown.prevent="run(c)">{{ c.label }}</li>
      </ul>
    </div>
    <button class="btn icon" :aria-label="t('settings')" :class="{ on: state.view === 'settings' }" @click="state.view = state.view === 'settings' ? 'workspace' : 'settings'">
      <Icon name="gear" :size="16" />
    </button>
    <NewSessionDialog v-if="showNew" @close="showNew = false" />
  </header>
</template>

<style scoped>
.topbar { height: 52px; flex-shrink: 0; display: flex; align-items: center; gap: 10px; padding: 0 16px; background: var(--panel); border-bottom: 1px solid var(--border); flex-wrap: nowrap; overflow: hidden; }
.brand { display: flex; align-items: center; gap: 8px; font-weight: 600; font-size: 15px; }
.logo { display: inline-flex; align-items: center; justify-content: center; width: 24px; height: 24px; border-radius: 6px; background: var(--accent); color: var(--accent-ink); font-size: 12px; font-weight: 600; }
.sep { width: 1px; height: 24px; background: var(--border); margin: 0 4px; }
.grow { flex: 1; }
.palette { position: relative; }
.search { display: flex; align-items: center; gap: 8px; width: 300px; height: 32px; padding: 0 10px; border-radius: 8px; background: var(--bg); border: 1px solid var(--border-2); color: var(--muted); }
.search input { flex: 1; min-width: 0; background: none; border: 0; outline: none; font-size: 13px; }
.kbd { font-size: 11px; padding: 1px 6px; border: 1px solid var(--border-2); border-radius: 4px; }
.menu { position: absolute; top: 38px; left: 0; right: 0; margin: 0; padding: 6px; list-style: none; background: var(--panel-2); border: 1px solid var(--border-2); border-radius: 8px; z-index: 40; max-height: 360px; overflow: auto; box-shadow: 0 12px 32px rgba(0,0,0,.5); }
.menu li { padding: 8px 10px; border-radius: 6px; font-size: 13px; cursor: pointer; }
.menu li.active { background: var(--accent-soft); color: #fff; }
.btn.on { border-color: var(--accent); }
@media (max-width: 1100px) { .search { width: 180px; } }
</style>
