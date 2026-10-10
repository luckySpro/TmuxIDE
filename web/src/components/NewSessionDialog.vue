<script setup lang="ts">
import { ref } from 'vue'
import Modal from './Modal.vue'
import { api } from '../api'
import { t } from '../i18n'
import { act, state } from '../store'

const props = defineProps<{ template?: string }>()
const emit = defineEmits<{ close: [] }>()
const name = ref(props.template ? `${props.template}-1` : '')
const cwd = ref('')
const template = ref(props.template ?? '')
const valid = () => /^[\p{L}\p{N}_-]{1,64}$/u.test(name.value) && !name.value.startsWith('-')

async function create() {
  const r = await act(() => api.post<string>('/sessions', { name: name.value, cwd: cwd.value || null, template: template.value || null }))
  if (r) {
    state.selected = r
    state.view = 'workspace'
    emit('close')
  }
}
</script>

<template>
  <Modal :title="t('newSession')" :confirm-text="t('newSession')" :disabled="!valid() || state.busy" @confirm="create" @cancel="emit('close')">
    <label class="field"><span>{{ t('name') }}</span><input v-model="name" class="input" placeholder="dev" autocomplete="off" /></label>
    <label class="field">
      <span>{{ t('fromTemplate') }}</span>
      <select v-model="template" class="input">
        <option value="">{{ t('blank') }}</option>
        <option v-for="tp in state.templates" :key="tp.name" :value="tp.name">{{ tp.name }} · {{ tp.windows.join(' / ') }}</option>
      </select>
    </label>
    <label v-if="!template" class="field"><span>{{ t('cwdOptional') }}</span><input v-model="cwd" class="input" placeholder="~/code/project" autocomplete="off" /></label>
  </Modal>
</template>
