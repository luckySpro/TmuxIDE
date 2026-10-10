<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import Icon from '../components/Icon.vue'
import { api, enc, type CommitInfo, type DiffLine, type RestoreResult, type RestoreStep } from '../api'
import { formatTime, t } from '../i18n'
import { act, state } from '../store'

const target = computed(() => state.historyTarget)
const commits = ref<CommitInfo[]>([])
const selected = ref<string | null>(null)
const diff = ref<DiffLine[]>([])
const steps = ref<RestoreStep[]>([])
const mode = ref<'rename' | 'replace' | 'skip'>('rename')
const runCommands = ref(true)

const idx = computed(() => commits.value.findIndex((c) => c.id === selected.value))
const base = computed(() => (idx.value >= 0 ? commits.value[idx.value + 1] : undefined))
const running = computed(() => state.sessions.some((s) => s.name === target.value?.name))
const adds = computed(() => diff.value.filter((l) => l.tag === '+').length)
const dels = computed(() => diff.value.filter((l) => l.tag === '-').length)

async function load() {
  if (!target.value) return
  commits.value = await api.get<CommitInfo[]>(`/history/${enc(target.value.host)}/${enc(target.value.name)}`).catch(() => [])
  selected.value = commits.value[0]?.id ?? null
}
watch(target, load, { immediate: true })

watch([selected, runCommands], async () => {
  const tg = target.value
  if (!tg || !selected.value) { diff.value = []; steps.value = []; return }
  const path = `${enc(tg.host)}/${enc(tg.name)}`
  diff.value = base.value
    ? await api.get<DiffLine[]>(`/diff/${path}?from=${base.value.id}&to=${selected.value}`).catch(() => [])
    : (await api.get<string>(`/saved/${path}?rev=${selected.value}`).catch(() => ''))
        .split('\n').filter((l, i, a) => l || i < a.length - 1)
        .map((text, i) => ({ tag: '+', old_no: null, new_no: i + 1, text }) as DiffLine)
  steps.value = await api.post<RestoreStep[]>('/restore/preview', {
    host: tg.host, name: tg.name, rev: selected.value, run_commands: runCommands.value,
  }).catch(() => [])
})

function describe(s: RestoreStep): string {
  switch (s.kind) {
    case 'new_session': return `新建会话 ${s.session}，目录 ${s.cwd}`
    case 'new_window': return `创建窗口 ${s.window}`
    case 'split_pane': return `分屏（${s.cwd}）`
    case 'select_layout': return '按保存的布局排列面板'
    case 'run_command': return `运行 ${s.cmd}`
    default: return ''
  }
}
const visibleSteps = computed(() => steps.value.map(describe).filter(Boolean))

function restore() {
  const tg = target.value
  if (!tg) return
  act(() => api.post<RestoreResult>('/restore', { host: tg.host, name: tg.name, rev: selected.value, mode: mode.value, run_commands: runCommands.value }), (r) => {
    if (r.skipped) return t('modeSkip')
    state.selected = r.session
    state.view = 'workspace'
    return t('restoredOk', { name: r.session }) + (r.report?.warnings.length ? ` · ${r.report.warnings.join('；')}` : '')
  })
}
</script>

<template>
  <div class="history">
    <nav class="commits scroll" :aria-label="t('history')">
      <button class="back" @click="state.view = 'workspace'"><Icon name="back" />{{ t('back') }}</button>
      <div class="head">
        <div class="mono name">{{ target?.name }}</div>
        <div class="muted small">{{ target?.host }}</div>
      </div>
      <div v-if="!commits.length" class="muted small pad">{{ t('noHistory') }}</div>
      <button v-for="(c, i) in commits" :key="c.id" class="commit" :class="{ active: c.id === selected }" @click="selected = c.id">
        <span class="dot" :class="i === 0 ? 'on' : 'off'" />
        <span class="body">
          <span class="summary">{{ c.summary.split(' @ ')[0] }}</span>
          <span class="mono muted small">{{ c.short }} · {{ formatTime(c.time) }}</span>
        </span>
      </button>
    </nav>

    <main class="diff-area">
      <div class="diff-head">
        <template v-if="base"><span class="tag mono blue">{{ base.short }}</span><span>{{ t('diffBase') }}</span></template>
        <template v-else-if="selected"><span>{{ t('firstSave') }}</span></template>
        <span v-if="selected" class="tag mono green">{{ selected.slice(0, 7) }}</span>
        <span class="grow" />
        <span class="mono path">hosts/{{ target?.host }}/sessions/<b>{{ target?.name }}.toml</b></span>
        <span class="add-n">+{{ adds }}</span><span class="del-n">−{{ dels }}</span>
      </div>
      <div class="diff mono scroll">
        <div v-if="!selected" class="muted pad">{{ t('selectCommit') }}</div>
        <div v-else-if="base && !adds && !dels" class="muted pad">{{ t('noChanges') }}</div>
        <div v-for="(l, i) in diff" :key="i" class="line" :class="{ add: l.tag === '+', del: l.tag === '-' }">
          <span class="no">{{ l.new_no ?? l.old_no }}</span><span class="sign">{{ l.tag }}</span><span class="text">{{ l.text }}</span>
        </div>
      </div>
    </main>

    <aside class="restore">
      <div>
        <h2>{{ t('restoreThis') }}</h2>
        <p class="mono muted small">{{ selected?.slice(0, 7) }}</p>
      </div>
      <fieldset>
        <legend>{{ t('conflictTitle') }}<template v-if="running"> · <span class="warn">{{ target?.name }} {{ t('running') }}</span></template></legend>
        <label class="opt" :class="{ on: mode === 'rename' }"><input v-model="mode" type="radio" value="rename" />{{ t('modeRename') }}</label>
        <label class="opt" :class="{ on: mode === 'replace' }"><input v-model="mode" type="radio" value="replace" />{{ t('modeReplace') }}</label>
        <label class="opt" :class="{ on: mode === 'skip' }"><input v-model="mode" type="radio" value="skip" />{{ t('modeSkip') }}</label>
      </fieldset>
      <label class="check"><input v-model="runCommands" type="checkbox" />{{ t('runCommands') }}</label>
      <div class="steps">
        <div class="section-title">{{ t('steps') }}</div>
        <ol>
          <li v-for="(s, i) in visibleSteps" :key="i"><span class="n" :class="{ first: i === 0 }">{{ i + 1 }}</span><span>{{ s }}</span></li>
        </ol>
      </div>
      <div class="grow" />
      <button class="btn primary big" :disabled="!selected || state.busy" @click="restore">{{ t('restoreSession') }}</button>
    </aside>
  </div>
</template>

<style scoped>
.history { flex: 1; display: flex; min-height: 0; }
.commits { width: 300px; flex-shrink: 0; background: var(--panel); border-right: 1px solid var(--border); display: flex; flex-direction: column; }
.back { display: flex; align-items: center; gap: 6px; padding: 12px 16px; background: none; border: 0; color: var(--text-2); cursor: pointer; font-size: 13px; }
.back:hover { color: var(--text); }
.head { padding: 4px 16px 12px; border-bottom: 1px solid var(--border); }
.name { font-size: 17px; font-weight: 600; }
.small { font-size: 11.5px; }
.pad { padding: 16px; }
.commit { display: flex; gap: 12px; padding: 12px 16px; border: 0; border-bottom: 1px solid #1e242c; background: none; text-align: left; cursor: pointer; color: var(--text); }
.commit:hover { background: var(--panel-2); }
.commit.active { background: #1c232b; box-shadow: inset 2px 0 0 var(--blue); }
.commit .dot { margin-top: 6px; }
.body { display: flex; flex-direction: column; min-width: 0; }
.summary { font-size: 13.5px; }
.diff-area { flex: 1; min-width: 0; display: flex; flex-direction: column; padding: 20px 24px; gap: 14px; }
.diff-head { display: flex; align-items: center; gap: 10px; font-size: 13px; color: var(--text-2); flex-wrap: wrap; }
.tag { padding: 3px 8px; border-radius: 4px; }
.tag.blue { background: #1c232b; color: var(--blue); }
.tag.green { background: var(--accent-soft); color: #86efac; }
.grow { flex: 1; }
.path b { color: var(--text); font-weight: 500; }
.add-n { color: #86efac; }
.del-n { color: var(--del-ink); }
.diff { flex: 1; min-height: 0; background: var(--panel-3); border: 1px solid var(--border); border-radius: 10px; padding: 10px 0; font-size: 12.5px; line-height: 1.75; color: #c9d1d9; }
.line { display: flex; padding: 0 14px; white-space: pre; }
.line.add { background: var(--add-bg); color: var(--add-ink); }
.line.del { background: var(--del-bg); color: var(--del-ink); }
.no { width: 36px; color: #4b5563; flex-shrink: 0; }
.sign { width: 16px; flex-shrink: 0; }
.restore { width: 340px; flex-shrink: 0; background: var(--panel); border-left: 1px solid var(--border); padding: 20px; display: flex; flex-direction: column; gap: 18px; overflow: auto; }
h2 { margin: 0 0 4px; font-size: 17px; font-weight: 600; }
p { margin: 0; }
fieldset { border: 0; padding: 0; margin: 0; display: flex; flex-direction: column; gap: 8px; }
legend { font-size: 13px; font-weight: 500; margin-bottom: 8px; padding: 0; }
.warn { color: var(--amber); }
.opt { display: flex; align-items: center; gap: 10px; padding: 10px 12px; border-radius: 8px; border: 1px solid var(--border-2); font-size: 13.5px; cursor: pointer; }
.opt.on { border-color: var(--accent); background: #16201a; }
input[type='radio'], input[type='checkbox'] { accent-color: var(--accent); }
.check { display: flex; align-items: center; gap: 10px; font-size: 13px; color: #c3cad3; }
.steps { padding: 14px; border-radius: 8px; background: var(--bg); border: 1px solid var(--border); }
ol { list-style: none; margin: 10px 0 0; padding: 0; display: flex; flex-direction: column; gap: 8px; }
ol li { display: flex; gap: 10px; font-size: 12.5px; line-height: 1.5; }
.n { width: 20px; height: 20px; border-radius: 50%; background: #232a33; color: var(--text-2); font-size: 11px; display: flex; align-items: center; justify-content: center; flex-shrink: 0; }
.n.first { background: var(--accent); color: var(--accent-ink); }
.big { height: 42px; font-size: 14px; }
</style>
