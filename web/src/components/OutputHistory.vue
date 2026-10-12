<script setup lang="ts">
import { nextTick, onMounted, ref } from 'vue'
import Icon from './Icon.vue'
import { api, enc } from '../api'
import { t } from '../i18n'
import { copyText } from '../store'

// 面板的输出历史（tmux 滚动缓冲区）：全屏程序（vim、less 等）的画面不在其中
const props = defineProps<{ pane: string; title: string }>()
const emit = defineEmits<{ close: [] }>()
const dialog = ref<HTMLDialogElement>()
const body = ref<HTMLPreElement>()
const text = ref('')
const lines = ref(5000)
const loading = ref(false)
const error = ref('')

async function load() {
  loading.value = true
  error.value = ''
  try {
    const r = await api.get<{ text: string }>(`/panes/${enc(props.pane)}/history?lines=${lines.value}`)
    text.value = r.text
    await nextTick()
    if (body.value) body.value.scrollTop = body.value.scrollHeight
  } catch (e) {
    error.value = e instanceof Error ? e.message : String(e)
  } finally {
    loading.value = false
  }
}

onMounted(() => {
  dialog.value?.showModal()
  load()
})
</script>

<template>
  <dialog ref="dialog" class="hist" @cancel.prevent="emit('close')" @click.self="emit('close')">
    <header>
      <h2><Icon name="history" :size="15" />{{ t('outputHistory') }} <span class="mono sub">{{ title }}</span></h2>
      <select v-model.number="lines" class="input select" :aria-label="t('historyLines')" @change="load">
        <option :value="5000">{{ t('lastLines', { n: 5000 }) }}</option>
        <option :value="20000">{{ t('lastLines', { n: 20000 }) }}</option>
        <option :value="50000">{{ t('lastLines', { n: 50000 }) }}</option>
      </select>
      <button class="btn small" :disabled="loading" @click="load"><Icon name="restore" :size="13" />{{ t('refresh') }}</button>
      <button class="btn small" :disabled="!text" @click="copyText(text)"><Icon name="copy" :size="13" />{{ t('copy') }}</button>
      <button class="icon-btn" :aria-label="t('close')" @click="emit('close')"><Icon name="close" :size="15" /></button>
    </header>
    <p v-if="error" class="err">{{ error }}</p>
    <pre ref="body" class="mono body">{{ text || (loading ? '…' : t('historyEmpty')) }}</pre>
    <p class="hint">{{ t('historyHint') }}</p>
  </dialog>
</template>

<style scoped>
.hist { width: min(1100px, calc(100vw - 48px)); height: min(80vh, 900px); padding: 0; border-radius: 12px; border: 1px solid var(--border); background: var(--panel); color: var(--text); }
.hist[open] { display: flex; flex-direction: column; }
.hist::backdrop { background: rgba(0, 0, 0, .55); }
header { display: flex; align-items: center; gap: 8px; padding: 12px 16px; border-bottom: 1px solid var(--border); }
h2 { flex: 1; margin: 0; font-size: 15px; display: flex; align-items: center; gap: 8px; min-width: 0; }
.sub { font-size: 12px; color: var(--muted); font-weight: 400; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.btn.small { display: inline-flex; align-items: center; gap: 5px; }
.select { width: auto; height: 30px; font-size: 12.5px; }
.body { flex: 1; margin: 0; padding: 12px 16px; overflow: auto; background: var(--bg); font-size: 12.5px; line-height: 1.45; white-space: pre-wrap; word-break: break-all; color: #c9d1d9; user-select: text; }
.err { margin: 8px 16px 0; color: var(--red); font-size: 12.5px; }
.hint { margin: 0; padding: 8px 16px; font-size: 12px; color: var(--muted); border-top: 1px solid var(--border); }
</style>
