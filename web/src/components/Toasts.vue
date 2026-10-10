<script setup lang="ts">
import { state } from '../store'
import Icon from './Icon.vue'
</script>

<template>
  <div class="toasts" role="status">
    <div v-for="m in state.toasts" :key="m.id" class="toast" :class="m.kind">
      <Icon :name="m.kind === 'ok' ? 'check' : 'warn'" />
      <span>{{ m.text }}</span>
    </div>
  </div>
</template>

<style scoped>
.toasts { position: fixed; right: 16px; bottom: 44px; display: flex; flex-direction: column; gap: 8px; z-index: 50; max-width: min(460px, calc(100vw - 32px)); }
.toast { display: flex; gap: 10px; align-items: flex-start; padding: 10px 14px; border-radius: 8px; background: var(--panel-2); border: 1px solid var(--border-2); font-size: 13px; box-shadow: 0 8px 24px rgba(0,0,0,.4); }
.toast.ok svg { color: var(--accent); flex-shrink: 0; margin-top: 2px; }
.toast.error { border-color: #5c2b2b; }
.toast.error svg { color: var(--red); flex-shrink: 0; margin-top: 2px; }
</style>
