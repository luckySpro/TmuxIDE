<script setup lang="ts">
import { onMounted, ref, watch } from 'vue'
import UpdatePanel from './UpdatePanel.vue'
import { t } from '../i18n'

// 低于最低支持版本时必须升级：不能用 Esc 或点击背景关闭
const dialog = ref<HTMLDialogElement>()
onMounted(() => dialog.value?.showModal())
watch(dialog, (d) => d && !d.open && d.showModal())
</script>

<template>
  <dialog ref="dialog" class="gate" @cancel.prevent>
    <h2>{{ t('updGateTitle') }}</h2>
    <p class="lead">{{ t('updGateText') }}</p>
    <UpdatePanel />
  </dialog>
</template>

<style scoped>
.gate { width: min(520px, calc(100vw - 32px)); padding: 22px 24px; border-radius: 12px; border: 1px solid var(--border); background: var(--panel); color: var(--text); }
.gate::backdrop { background: rgba(0, 0, 0, .6); }
h2 { margin: 0 0 6px; font-size: 17px; }
.lead { margin: 0 0 16px; font-size: 13px; color: var(--text-2); }
</style>
