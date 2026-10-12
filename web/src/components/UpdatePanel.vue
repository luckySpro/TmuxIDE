<script setup lang="ts">
import { computed } from 'vue'
import Icon from './Icon.vue'
import { remoteAdmin } from '../api'
import { formatTime, t } from '../i18n'
import { state, updateAction } from '../store'

const u = computed(() => state.update)
const pct = computed(() => (u.value && u.value.total ? Math.floor((u.value.downloaded / u.value.total) * 100) : 0))
const mb = (n: number) => (n / 1048576).toFixed(1)
</script>

<template>
  <div v-if="u" class="upd">
    <div class="line">
      <span class="dot" :class="u.state" />
      <div class="what">
        <template v-if="!u.enabled">{{ t('updDisabled') }}</template>
        <template v-else-if="u.state === 'checking'">{{ t('updChecking') }}</template>
        <template v-else-if="u.state === 'downloading'">{{ t('updDownloading', { v: u.latest ?? '' }) }} {{ u.total ? `${pct}% · ${mb(u.downloaded)} / ${mb(u.total)} MB` : '' }}</template>
        <template v-else-if="u.state === 'ready'">{{ u.can_restart ? t('updReady', { v: u.latest ?? '' }) : t('updReadyCli', { v: u.latest ?? '' }) }}</template>
        <template v-else-if="u.state === 'available'">{{ u.required ? t('updRequired', { v: u.latest ?? '' }) : t('updAvailable', { v: u.latest ?? '' }) }}</template>
        <template v-else-if="u.state === 'uptodate'">{{ t('updLatest') }}</template>
        <template v-else-if="u.state === 'error'">{{ t('updFailed') }}</template>
        <template v-else>{{ t('updIdle') }}</template>
        <div class="hint">{{ t('updCurrent', { v: u.current }) }}<template v-if="u.checked_at"> · {{ t('updCheckedAt', { time: formatTime(u.checked_at) }) }}</template></div>
      </div>
      <template v-if="u.enabled && !remoteAdmin">
        <button v-if="u.state === 'ready' && u.can_restart" class="btn primary small" @click="updateAction('restart')">{{ t('updRestart') }}</button>
        <button v-else-if="u.state === 'available' && !u.manual_reason" class="btn primary small" @click="updateAction('install')"><Icon name="save" :size="13" />{{ t('updInstall') }}</button>
        <button v-if="['idle', 'uptodate', 'error', 'available'].includes(u.state)" class="btn small" @click="updateAction('check')">{{ t('updCheck') }}</button>
      </template>
    </div>
    <div v-if="u.state === 'downloading'" class="bar"><div :style="{ width: `${pct}%` }" /></div>
    <p v-if="u.error" class="err">{{ u.error }}</p>
    <p v-if="u.manual_reason && u.state === 'available'" class="hint">{{ u.manual_reason }}</p>
    <p v-if="remoteAdmin && ['available', 'ready'].includes(u.state)" class="hint">{{ t('updRemoteOnly') }}</p>
    <a v-if="u.release_url && ['available', 'error', 'ready'].includes(u.state)" class="rel" :href="u.release_url" target="_blank" rel="noreferrer">{{ t('updReleasePage') }}</a>
    <pre v-if="u.notes && ['available', 'downloading', 'ready'].includes(u.state)" class="notes">{{ u.notes }}</pre>
  </div>
</template>

<style scoped>
.upd { display: flex; flex-direction: column; gap: 8px; }
.line { display: flex; align-items: center; gap: 10px; }
.what { flex: 1; min-width: 0; font-size: 13.5px; }
.dot { width: 8px; height: 8px; border-radius: 50%; background: var(--muted); flex-shrink: 0; }
.dot.uptodate { background: var(--accent); }
.dot.available, .dot.downloading, .dot.ready { background: var(--amber); }
.dot.error { background: var(--red); }
.hint { font-size: 12px; color: var(--muted); margin: 2px 0 0; }
.btn.small { display: inline-flex; align-items: center; gap: 5px; }
.bar { height: 6px; border-radius: 3px; background: var(--panel-2); overflow: hidden; }
.bar div { height: 100%; background: var(--accent); transition: width .3s; }
.err { margin: 0; font-size: 12.5px; color: var(--red); word-break: break-word; }
.rel { font-size: 12.5px; color: #86efac; }
.notes { margin: 0; max-height: 180px; overflow: auto; padding: 10px 12px; border-radius: 8px; background: var(--panel-2); border: 1px solid var(--border); font-size: 12px; white-space: pre-wrap; color: var(--text-2); }
</style>
