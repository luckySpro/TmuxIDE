<script setup lang="ts">
import { api, remoteAdmin } from '../api'
import { lang, languages, t } from '../i18n'
import { state } from '../store'

function openRemote() {
  state.settingsSection = 'remote'
  state.view = 'settings'
}

async function switchLang() {
  const next = languages[(languages.findIndex((l) => l.id === lang.value) + 1) % languages.length].id
  lang.value = next
  try {
    const s = await api.get<{ config: { ui: { language: string } } }>('/settings')
    s.config.ui.language = next
    await api.put('/settings', s)
  } catch { /* 语言切换失败只影响下次启动 */ }
}
</script>

<template>
  <footer class="status">
    <span class="item"><span class="dot on" />tmux {{ state.info?.tmux.version ?? '…' }}（{{ state.info?.tmux.bundled ? t('builtin') : t('system') }}）</span>
    <span>{{ t('sessionsRunning', { n: state.sessions.length }) }}</span>
    <span v-if="state.git?.has_remote" class="mono">{{ state.git.branch }} ↑{{ state.git.ahead }} ↓{{ state.git.behind }}</span>
    <span v-else>{{ t('notSynced') }}</span>
    <span class="grow" />
    <a v-if="remoteAdmin" class="lang remote online" href="/" :title="t('backToTerminals')"><span class="dot" />{{ t('remoteAdminBar') }}</a>
    <button v-else-if="state.remote && state.remote.state !== 'off'" class="lang remote" :class="state.remote.state" @click="openRemote">
      <span class="dot" />{{ t('remoteBar') }}<template v-if="state.remote.terminals"> · {{ t('remoteBarTerminals', { n: state.remote.terminals }) }}</template>
    </button>
    <span>{{ state.autosaveMinutes ? t('autosaveEvery', { n: state.autosaveMinutes }) : t('autosaveOff') }}</span>
    <span>{{ state.info?.host }}</span>
    <button class="lang" @click="switchLang">{{ languages.find((l) => l.id === lang)?.label }}</button>
  </footer>
</template>

<style scoped>
.status { height: 28px; flex-shrink: 0; display: flex; align-items: center; gap: 18px; padding: 0 14px; background: var(--panel); border-top: 1px solid var(--border); font-size: 12px; color: var(--text-2); white-space: nowrap; overflow: hidden; }
.item { display: flex; align-items: center; gap: 6px; }
.item .dot { width: 6px; height: 6px; }
.grow { flex: 1; }
.lang { background: none; border: 0; color: var(--text-2); cursor: pointer; font-size: 12px; padding: 0; }
.lang:hover { color: var(--text); }
.remote { text-decoration: none; display: flex; align-items: center; gap: 6px; }
.remote .dot { width: 6px; height: 6px; background: var(--amber); }
.remote.online .dot { background: var(--accent); }
.remote.error .dot { background: var(--red); }
</style>
