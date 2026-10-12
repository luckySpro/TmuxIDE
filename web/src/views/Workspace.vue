<script setup lang="ts">
import { computed, nextTick, ref } from 'vue'
import NewSessionDialog from '../components/NewSessionDialog.vue'
import OutputHistory from '../components/OutputHistory.vue'
import SessionTree from '../components/SessionTree.vue'
import TerminalPane from '../components/TerminalPane.vue'
import Inspector from '../components/Inspector.vue'
import Icon from '../components/Icon.vue'
import { api, enc } from '../api'
import { t } from '../i18n'
import { act, currentSession, groupNames, refreshSessions, setGroupFilter, state } from '../store'

const session = computed(currentSession)
const groups = computed(groupNames)
const showNew = ref(false)
const historyOf = ref<{ pane: string; title: string } | null>(null)

function openHistory() {
  const w = session.value?.windows.find((x) => x.active) ?? session.value?.windows[0]
  const p = w?.panes.find((x) => x.active) ?? w?.panes[0]
  if (session.value && w && p) historyOf.value = { pane: p.id, title: `${session.value.name} › ${w.index}:${w.name}` }
}
/** 选项卡里的会话：按分组筛选，分组相同的排在一起 */
const tabSessions = computed(() => {
  const f = state.groupFilter && groups.value.includes(state.groupFilter) ? state.groupFilter : ''
  return state.sessions
    .filter((s) => !f || s.group === f)
    .slice()
    .sort((a, b) => (a.group || '\uffff').localeCompare(b.group || '\uffff') || a.name.localeCompare(b.name))
})

/** 切换会话选项卡后把键盘焦点交给终端 */
function openTab(name: string) {
  state.selected = name
  nextTick(() => document.querySelector<HTMLTextAreaElement>('.center .xterm-helper-textarea')?.focus())
}

function pickGroup(g: string) {
  setGroupFilter(g)
  if (g && session.value?.group !== g) {
    const first = state.sessions.find((s) => s.group === g)
    if (first) state.selected = first.name
  }
}

async function selectWindow(id: string) {
  try {
    await api.post(`/windows/${enc(id)}/select`)
    await refreshSessions()
  } catch { /* 窗口可能刚被关闭 */ }
}

function newWindow() {
  if (session.value) act(() => api.post(`/sessions/${enc(session.value!.name)}/windows`, {}))
}
</script>

<template>
  <div class="workspace">
    <SessionTree />
    <main class="center">
      <div v-if="state.sessions.length" class="stabs">
        <div v-if="groups.length" class="groups" role="radiogroup" :aria-label="t('groupName')">
          <button role="radio" :aria-checked="!state.groupFilter" class="chip" :class="{ on: !state.groupFilter || !groups.includes(state.groupFilter) }" @click="pickGroup('')">{{ t('allGroups') }}</button>
          <button v-for="g in groups" :key="g" role="radio" :aria-checked="state.groupFilter === g" class="chip" :class="{ on: state.groupFilter === g }" @click="pickGroup(g)">{{ g }}</button>
        </div>
        <div class="stab-list" role="tablist" :aria-label="t('session')">
          <button v-for="s in tabSessions" :key="s.id" role="tab" :aria-selected="s.name === state.selected" class="stab mono" :class="{ active: s.name === state.selected }"
            :title="s.group ? `${s.group} / ${s.name}` : s.name" @click="openTab(s.name)">
            <span v-if="s.group && !state.groupFilter" class="g">{{ s.group }}</span>{{ s.name }}
          </button>
          <button class="stab add" :aria-label="t('newSession')" :title="t('newSession')" @click="showNew = true"><Icon name="plus" :size="14" /></button>
        </div>
      </div>
      <template v-if="session">
        <div class="tabs" role="tablist">
          <span class="crumb mono">{{ session.name }} ▸</span>
          <button v-for="w in session.windows" :key="w.id" role="tab" :aria-selected="w.active" class="tab mono" :class="{ active: w.active }"
            @click="selectWindow(w.id)">{{ w.index }}:{{ w.name }}</button>
          <button class="tab add" :aria-label="t('newWindow')" :title="t('newWindow')" @click="newWindow"><Icon name="plus" :size="14" /></button>
          <span class="grow" />
          <button class="hist-btn" :title="t('outputHistoryHint')" @click="openHistory"><Icon name="history" :size="14" />{{ t('outputHistory') }}</button>
        </div>
        <!-- 切换会话选项卡时保留终端连接，切回来立即可用 -->
        <KeepAlive :max="8">
          <TerminalPane :key="session.name" :session="session.name" />
        </KeepAlive>
      </template>
      <div v-else class="empty">
        <span class="logo mono">&gt;_</span>
        <h2>{{ t('noSessionTitle') }}</h2>
        <p>{{ t('noSession') }}</p>
      </div>
    </main>
    <Inspector v-if="session" :session="session" />
    <NewSessionDialog v-if="showNew" @close="showNew = false" />
    <OutputHistory v-if="historyOf" :pane="historyOf.pane" :title="historyOf.title" @close="historyOf = null" />
  </div>
</template>

<style scoped>
.workspace { flex: 1; display: flex; min-height: 0; }
.center { flex: 1; min-width: 0; display: flex; flex-direction: column; background: var(--bg); }
.grow { flex: 1; }
.hist-btn { align-self: center; display: inline-flex; align-items: center; gap: 6px; height: 28px; padding: 0 10px; border-radius: 6px; border: 1px solid var(--border); background: var(--panel-2); color: var(--text-2); font-size: 12.5px; cursor: pointer; white-space: nowrap; }
.hist-btn:hover { color: var(--text); }
.stabs { flex-shrink: 0; display: flex; align-items: center; gap: 10px; min-height: 38px; padding: 0 12px; background: var(--panel); border-bottom: 1px solid var(--border); }
.groups { display: flex; gap: 4px; flex-shrink: 0; padding-right: 10px; border-right: 1px solid var(--border); }
.chip { height: 24px; padding: 0 10px; border-radius: 12px; border: 1px solid var(--border); background: none; color: var(--text-2); font-size: 12px; cursor: pointer; white-space: nowrap; }
.chip:hover { color: var(--text); }
.chip.on { border-color: var(--accent); background: var(--accent-soft); color: #fff; }
.stab-list { display: flex; gap: 4px; overflow-x: auto; min-width: 0; padding: 6px 0; scrollbar-width: thin; }
.stab { height: 26px; padding: 0 12px; border-radius: 6px; border: 1px solid var(--border); background: var(--panel-2); color: var(--text-2); font-size: 12.5px; cursor: pointer; white-space: nowrap; display: inline-flex; align-items: center; gap: 6px; }
.stab:hover { color: var(--text); }
.stab.active { border-color: var(--accent); color: #fff; background: var(--accent-soft); }
.stab .g { font-size: 11px; color: var(--muted); }
.stab.active .g { color: #86efac; }
.stab.add { padding: 0 8px; }
.tabs { height: 40px; flex-shrink: 0; display: flex; align-items: flex-end; gap: 2px; padding: 0 12px; background: var(--panel-3); border-bottom: 1px solid var(--border); overflow-x: auto; }
.crumb { align-self: center; font-size: 12px; color: var(--muted); margin-right: 8px; white-space: nowrap; }
.tab { height: 34px; padding: 0 16px; border: 1px solid transparent; border-bottom: 0; border-radius: 6px 6px 0 0; background: none; color: var(--text-2); font-size: 12.5px; cursor: pointer; white-space: nowrap; }
.tab:hover { color: var(--text); }
.tab.active { background: var(--bg); border-color: var(--border); color: #fff; box-shadow: inset 0 2px 0 var(--accent); }
.tab.add { padding: 0 10px; display: inline-flex; align-items: center; }
.empty { flex: 1; display: flex; flex-direction: column; align-items: center; justify-content: center; color: var(--text-2); gap: 4px; }
.empty h2 { margin: 12px 0 0; font-size: 18px; color: var(--text); font-weight: 600; }
.empty p { margin: 0; }
.logo { width: 48px; height: 48px; border-radius: 12px; background: var(--accent-soft); color: var(--accent); display: flex; align-items: center; justify-content: center; font-size: 18px; font-weight: 600; }
</style>
