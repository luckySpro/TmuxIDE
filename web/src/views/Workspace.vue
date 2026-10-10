<script setup lang="ts">
import { computed } from 'vue'
import SessionTree from '../components/SessionTree.vue'
import TerminalPane from '../components/TerminalPane.vue'
import Inspector from '../components/Inspector.vue'
import Icon from '../components/Icon.vue'
import { api, enc } from '../api'
import { t } from '../i18n'
import { act, currentSession, refreshSessions, state } from '../store'

const session = computed(currentSession)

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
      <template v-if="session">
        <div class="tabs" role="tablist">
          <span class="crumb mono">{{ session.name }} ▸</span>
          <button v-for="w in session.windows" :key="w.id" role="tab" :aria-selected="w.active" class="tab mono" :class="{ active: w.active }"
            @click="selectWindow(w.id)">{{ w.index }}:{{ w.name }}</button>
          <button class="tab add" :aria-label="t('newWindow')" :title="t('newWindow')" @click="newWindow"><Icon name="plus" :size="14" /></button>
        </div>
        <TerminalPane :key="session.name" :session="session.name" />
      </template>
      <div v-else class="empty">
        <span class="logo mono">&gt;_</span>
        <h2>{{ t('noSessionTitle') }}</h2>
        <p>{{ t('noSession') }}</p>
      </div>
    </main>
    <Inspector v-if="session" :session="session" />
  </div>
</template>

<style scoped>
.workspace { flex: 1; display: flex; min-height: 0; }
.center { flex: 1; min-width: 0; display: flex; flex-direction: column; background: var(--bg); }
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
