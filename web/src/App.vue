<script setup lang="ts">
import { onMounted } from 'vue'
import TopBar from './components/TopBar.vue'
import StatusBar from './components/StatusBar.vue'
import Toasts from './components/Toasts.vue'
import Workspace from './views/Workspace.vue'
import Settings from './views/Settings.vue'
import History from './views/History.vue'
import { init, state } from './store'
import { t } from './i18n'

onMounted(init)
</script>

<template>
  <div class="app">
    <TopBar />
    <div v-if="state.authError" class="auth-error">
      <strong>TmuxIDE</strong>
      <p>访问令牌无效或已过期，请从 TmuxIDE 应用或 <code>tmuxide serve</code> 输出的地址重新打开。</p>
    </div>
    <Workspace v-else-if="state.view === 'workspace'" />
    <Settings v-else-if="state.view === 'settings'" />
    <History v-else-if="state.view === 'history'" />
    <StatusBar />
    <Toasts />
    <span class="sr-only" aria-live="polite">{{ state.busy ? t('loading') : '' }}</span>
  </div>
</template>

<style scoped>
.app { height: 100%; display: flex; flex-direction: column; }
.auth-error { flex: 1; display: flex; flex-direction: column; align-items: center; justify-content: center; color: var(--text-2); }
.sr-only { position: absolute; width: 1px; height: 1px; overflow: hidden; clip: rect(0 0 0 0); }
</style>
