<script setup lang="ts">
import { onMounted, ref } from 'vue'
import { t } from '../i18n'

defineProps<{ title: string; confirmText?: string; danger?: boolean; disabled?: boolean }>()
const emit = defineEmits<{ confirm: []; cancel: [] }>()
const dialog = ref<HTMLDialogElement>()

onMounted(() => {
  dialog.value?.showModal()
  dialog.value?.querySelector<HTMLInputElement>('input')?.focus()
})
</script>

<template>
  <dialog ref="dialog" class="modal" @cancel.prevent="emit('cancel')" @click.self="emit('cancel')">
    <form method="dialog" @submit.prevent="!disabled && emit('confirm')">
      <h2>{{ title }}</h2>
      <div class="body"><slot /></div>
      <div class="actions">
        <button type="button" class="btn" @click="emit('cancel')">{{ t('cancel') }}</button>
        <button type="submit" class="btn" :class="danger ? 'danger' : 'primary'" :disabled="disabled">
          {{ confirmText ?? t('confirm') }}
        </button>
      </div>
    </form>
  </dialog>
</template>

<style scoped>
.modal { border: 1px solid var(--border-2); border-radius: 12px; background: var(--panel); color: var(--text); padding: 0; width: min(460px, calc(100vw - 32px)); }
.modal::backdrop { background: rgba(0, 0, 0, .55); }
form { padding: 22px; display: flex; flex-direction: column; gap: 16px; }
h2 { margin: 0; font-size: 17px; font-weight: 600; }
.body { display: flex; flex-direction: column; gap: 14px; font-size: 14px; color: var(--text-2); }
.actions { display: flex; justify-content: flex-end; gap: 10px; }
</style>
