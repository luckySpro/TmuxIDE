<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import Icon from '../components/Icon.vue'
import RemoteSettings from '../components/RemoteSettings.vue'
import UpdatePanel from '../components/UpdatePanel.vue'
import { api, remoteAdmin, type SettingsView } from '../api'
import { lang, languages, t } from '../i18n'
import { act, state, toast } from '../store'

type Section = 'git' | 'save' | 'tmux' | 'remote' | 'general' | 'about'
const section = ref<Section>(state.settingsSection === 'remote' && !remoteAdmin ? 'remote' : state.settingsSection === 'about' ? 'about' : 'git')
const view = ref<SettingsView | null>(null)
const original = ref('')
const token = ref('')
const passphrase = ref('')
const testResult = ref<{ ok: boolean; text: string } | null>(null)
const exclude = ref('')

const sections: { id: Section; label: string }[] = [
  { id: 'git', label: 'git' }, { id: 'save', label: 'savePolicy' }, { id: 'tmux', label: 'tmuxConf' },
  { id: 'remote', label: 'remoteAccess' }, { id: 'general', label: 'general' }, { id: 'about', label: 'about' },
].filter((s) => !(remoteAdmin && s.id === 'remote')) as { id: Section; label: string }[]

async function load() {
  const v = await api.get<SettingsView>('/settings')
  view.value = v
  exclude.value = v.config.save.exclude.join(', ')
  original.value = JSON.stringify(v.config)
  token.value = ''
  passphrase.value = ''
}
onMounted(() => load().catch((e) => toast(String(e), 'error')))

const dirty = computed(() =>
  !!view.value && (JSON.stringify({ ...view.value.config, save: { ...view.value.config.save, exclude: parseExclude() } }) !== original.value || !!token.value || !!passphrase.value))

function parseExclude() {
  return exclude.value.split(/[,，\s]+/).map((s) => s.trim()).filter(Boolean)
}

async function save() {
  if (!view.value) return
  const body: SettingsView = {
    ...view.value,
    config: { ...view.value.config, save: { ...view.value.config.save, exclude: parseExclude() } },
    token: token.value || null,
    passphrase: passphrase.value || null,
  }
  const r = await act(() => api.put<SettingsView>('/settings', body), () => t('settingsSaved'))
  if (r) {
    state.autosaveMinutes = r.config.save.autosave_minutes
    lang.value = r.config.ui.language
    state.info = await api.get('/info')
    await load()
  }
}

async function test() {
  if (dirty.value) await save()
  testResult.value = null
  try {
    await api.post('/git/test')
    testResult.value = { ok: true, text: t('connOk') }
  } catch (e) {
    testResult.value = { ok: false, text: e instanceof Error ? e.message : String(e) }
  }
}

const authOptions = [
  { id: 'ssh-key', label: 'authKey', hint: '~/.ssh' },
  { id: 'https-token', label: 'authToken', hint: 'Keychain' },
  { id: 'ssh-agent', label: 'authAgent', hint: 'SSH_AUTH_SOCK' },
  { id: 'none', label: 'authNone', hint: 'file:// / public' },
]
</script>

<template>
  <div class="settings">
    <nav class="nav" :aria-label="t('settings')">
      <button class="back" @click="state.view = 'workspace'"><Icon name="back" />{{ t('back') }}</button>
      <button v-for="s in sections" :key="s.id" class="item" :class="{ active: section === s.id }" @click="section = s.id">{{ t(s.label) }}</button>
    </nav>
    <div v-if="view" class="content scroll">
      <div class="header">
        <h1>{{ t(sections.find((s) => s.id === section)!.label) }}</h1>
        <div v-if="section !== 'remote'" class="actions">
          <button class="btn" :disabled="!dirty" @click="load">{{ t('discard') }}</button>
          <button class="btn primary" :disabled="!dirty || state.busy" @click="save">{{ t('saveSettings') }}</button>
        </div>
      </div>

      <div v-if="section === 'git'" class="grid">
        <div class="col">
          <section class="card">
            <h2>{{ t('remote') }}</h2>
            <div class="two">
              <label class="field"><span>{{ t('remote') }}</span><input v-model="view.config.git.remote" class="input" placeholder="git@github.com:you/tmux-sessions.git" /></label>
              <label class="field narrow"><span>{{ t('branch') }}</span><input v-model="view.config.git.branch" class="input" /></label>
            </div>
            <div class="field-block">
              <span class="label">{{ t('auth') }}</span>
              <div class="opts">
                <label v-for="o in authOptions" :key="o.id" class="opt" :class="{ on: view.config.git.auth === o.id }">
                  <input v-model="view.config.git.auth" type="radio" :value="o.id" />
                  <span><b>{{ t(o.label) }}</b><small>{{ o.hint }}</small></span>
                </label>
              </div>
            </div>
            <div v-if="view.config.git.auth === 'ssh-key'" class="two">
              <label class="field"><span>{{ t('keyPath') }}</span><input v-model="view.config.git.ssh_key" class="input" /></label>
              <label class="field"><span>{{ t('passphrase') }}</span><input v-model="passphrase" type="password" class="input" :placeholder="view.has_passphrase ? t('savedSecret') : ''" autocomplete="off" /></label>
            </div>
            <div v-if="view.config.git.auth === 'https-token'" class="two">
              <label class="field narrow"><span>{{ t('username') }}</span><input v-model="view.config.git.username" class="input" placeholder="git" /></label>
              <label class="field"><span>{{ t('token') }}</span><input v-model="token" type="password" class="input" :placeholder="view.has_token ? t('savedSecret') : 'ghp_…'" autocomplete="off" /></label>
            </div>
            <div class="test">
              <button class="btn" :disabled="!view.config.git.remote" @click="test"><Icon name="link" />{{ t('testConn') }}</button>
              <span v-if="testResult" :class="testResult.ok ? 'ok' : 'bad'"><Icon :name="testResult.ok ? 'check' : 'warn'" />{{ testResult.text }}</span>
            </div>
          </section>
          <section class="card">
            <h2>{{ t('author') }}</h2>
            <div class="two">
              <label class="field"><span>{{ t('author') }}</span><input v-model="view.config.git.author_name" class="input" /></label>
              <label class="field"><span>{{ t('email') }}</span><input v-model="view.config.git.author_email" class="input" /></label>
            </div>
            <div class="toggle"><div><div>{{ t('autoCommit') }}</div><div class="hint">{{ t('autoCommitHint') }}</div></div>
              <button class="switch" role="switch" :aria-checked="view.config.git.auto_commit" :aria-label="t('autoCommit')" @click="view.config.git.auto_commit = !view.config.git.auto_commit" /></div>
            <div class="toggle"><div><div>{{ t('autoPush') }}</div><div class="hint">{{ t('autoPushHint') }}</div></div>
              <button class="switch" role="switch" :aria-checked="view.config.git.auto_push" :aria-label="t('autoPush')" @click="view.config.git.auto_push = !view.config.git.auto_push" /></div>
            <div class="toggle"><div><div>{{ t('syncConf') }}</div><div class="hint">{{ t('syncConfHint') }}</div></div>
              <button class="switch" role="switch" :aria-checked="view.config.git.sync_tmux_conf" :aria-label="t('syncConf')" @click="view.config.git.sync_tmux_conf = !view.config.git.sync_tmux_conf" /></div>
            <label class="field top"><span>{{ t('repoDir') }}</span><input v-model="view.config.git.repo_dir" class="input" :placeholder="state.info?.repo_dir" /><div class="hint">{{ t('repoDirHint') }}</div></label>
          </section>
        </div>
        <div class="col">
          <section class="card">
            <h2>{{ t('repoStructure') }}</h2>
            <pre class="tree mono">{{ `tmux-sessions/
├─ hosts/
│  ├─ ${state.info?.host ?? 'this-mac'}/   ←
│  │  └─ sessions/*.toml
│  └─ other-host/
├─ templates/*.toml
├─ tmux/tmux.conf
└─ tmuxide.toml` }}</pre>
          </section>
          <section class="card warn-card">
            <h2><Icon name="warn" />{{ t('privacyTitle') }}</h2>
            <p>{{ t('privacy') }}</p>
          </section>
          <section class="card">
            <h2>{{ t('components') }}</h2>
            <dl>
              <dt>{{ t('gitEngine') }}</dt><dd>{{ state.info?.git_engine }}</dd>
              <dt>tmux</dt><dd>{{ state.info?.tmux.version }}（{{ state.info?.tmux.bundled ? t('builtin') : t('system') }}）</dd>
            </dl>
          </section>
        </div>
      </div>

      <RemoteSettings v-else-if="section === 'remote'" />

      <div v-else-if="section === 'save'" class="narrow-col">
        <section class="card">
          <label class="field"><span>{{ t('autosaveMinutes') }}</span><input v-model.number="view.config.save.autosave_minutes" type="number" min="0" max="1440" class="input short" /></label>
          <label class="field top"><span>{{ t('exclude') }}</span><input v-model="exclude" class="input" placeholder="scratch, tmp" /></label>
          <div class="toggle"><div><div>{{ t('scrollback') }}</div><div class="hint">{{ t('scrollbackHint') }}</div></div>
            <button class="switch" role="switch" :aria-checked="view.config.save.scrollback" :aria-label="t('scrollback')" @click="view.config.save.scrollback = !view.config.save.scrollback" /></div>
          <label v-if="view.config.save.scrollback" class="field top"><span>{{ t('scrollbackLines') }}</span><input v-model.number="view.config.save.scrollback_lines" type="number" min="100" max="100000" class="input short" /></label>
        </section>
      </div>

      <div v-else-if="section === 'tmux'" class="narrow-col">
        <section class="card">
          <label class="field"><span>{{ t('socket') }}</span><input v-model="view.config.tmux.socket" class="input short" /><div class="hint">{{ t('socketHint') }}</div></label>
          <div class="toggle"><div>{{ t('useSystemTmux') }}</div>
            <button class="switch" role="switch" :aria-checked="view.config.tmux.use_system" :aria-label="t('useSystemTmux')" @click="view.config.tmux.use_system = !view.config.tmux.use_system" /></div>
          <p class="hint">tmux {{ state.info?.tmux.version }} · <span class="mono">{{ state.info?.tmux.path }}</span></p>
        </section>
      </div>

      <div v-else-if="section === 'general'" class="narrow-col">
        <section class="card">
          <label class="field"><span>{{ t('language') }}</span>
            <select v-model="view.config.ui.language" class="input short">
              <option v-for="l in languages" :key="l.id" :value="l.id">{{ l.label }}</option>
            </select>
          </label>
          <div class="toggle"><div><div>{{ t('updAuto') }}</div><div class="hint">{{ t('updAutoHint') }}</div></div>
            <button class="switch" role="switch" :aria-checked="view.config.ui.check_updates" :aria-label="t('updAuto')" @click="view.config.ui.check_updates = !view.config.ui.check_updates" /></div>
        </section>
      </div>

      <div v-else class="narrow-col">
        <section class="card">
          <h2>TmuxIDE</h2>
          <dl>
            <dt>{{ t('version') }}</dt><dd>{{ state.info?.version }} · {{ state.info?.os }}/{{ state.info?.arch }}</dd>
            <dt>tmux</dt><dd>{{ state.info?.tmux.version }}（{{ state.info?.tmux.bundled ? t('builtin') : t('system') }}）</dd>
            <dt>{{ t('gitEngine') }}</dt><dd>{{ state.info?.git_engine }}</dd>
            <dt>{{ t('dataDir') }}</dt><dd class="mono">{{ state.info?.data_dir }}</dd>
            <dt>{{ t('repoDir') }}</dt><dd class="mono">{{ state.info?.repo_dir }}</dd>
          </dl>
          <h2 class="sub">{{ t('updTitle') }}</h2>
          <UpdatePanel />
          <p class="hint">© M&amp;J Group · 473276@qq.com · MIT License<br />tmux (ISC) · libevent (BSD-3) · ncurses (MIT) · utf8proc (MIT) · libgit2 (GPLv2 with linking exception) · OpenSSL (Apache-2.0) · libssh2 (BSD-3) · cloudflared (Apache-2.0) · xterm.js (MIT) · Vue (MIT)</p>
        </section>
      </div>
    </div>
  </div>
</template>

<style scoped>
.card h2.sub { margin-top: 20px; font-size: 14px; }
.settings { flex: 1; display: flex; min-height: 0; }
.nav { width: 240px; flex-shrink: 0; padding: 8px 12px; background: var(--panel); border-right: 1px solid var(--border); display: flex; flex-direction: column; gap: 2px; }
.back { display: flex; align-items: center; gap: 6px; padding: 8px 4px 14px; background: none; border: 0; color: var(--text-2); cursor: pointer; font-size: 13px; }
.back:hover { color: var(--text); }
.item { height: 38px; padding: 0 14px; border-radius: 6px; border: 0; background: none; color: #c3cad3; font-size: 14px; font-weight: 500; text-align: left; cursor: pointer; }
.item:hover { background: var(--panel-2); }
.item.active { background: var(--accent-soft); color: #fff; box-shadow: inset 2px 0 0 var(--accent); }
.content { flex: 1; min-width: 0; padding: 28px 40px; }
.header { display: flex; align-items: center; justify-content: space-between; gap: 16px; margin-bottom: 24px; flex-wrap: wrap; }
h1 { margin: 0; font-size: 24px; font-weight: 600; }
.actions { display: flex; gap: 10px; }
.grid { display: grid; grid-template-columns: minmax(0, 1.4fr) minmax(0, 1fr); gap: 24px; align-items: start; }
.col { display: flex; flex-direction: column; gap: 20px; }
.narrow-col { max-width: 640px; display: flex; flex-direction: column; gap: 20px; }
.two { display: grid; grid-template-columns: minmax(0, 1fr) minmax(0, 1fr); gap: 16px; margin-bottom: 14px; }
.two:has(.narrow) { grid-template-columns: minmax(0, 1fr) 180px; }
.two:has(.narrow:first-child) { grid-template-columns: 180px minmax(0, 1fr); }
.field-block { margin-bottom: 14px; }
.label { display: block; font-size: 13px; font-weight: 500; margin-bottom: 6px; }
.opts { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 8px; }
.opt { display: flex; gap: 10px; align-items: flex-start; padding: 10px 12px; border-radius: 8px; border: 1px solid var(--border-2); cursor: pointer; }
.opt.on { border-color: var(--accent); background: #16201a; }
.opt input { margin-top: 3px; accent-color: var(--accent); }
.opt b { display: block; font-size: 13.5px; font-weight: 500; }
.opt small { display: block; font-size: 12px; color: var(--text-2); }
.test { display: flex; align-items: center; gap: 14px; flex-wrap: wrap; }
.test span { display: flex; align-items: center; gap: 6px; font-size: 13px; }
.ok { color: #86efac; }
.bad { color: var(--del-ink); }
.toggle { display: flex; align-items: center; justify-content: space-between; gap: 16px; padding: 12px 0; border-top: 1px solid #222831; font-size: 13.5px; }
.top { margin-top: 14px; }
.short { max-width: 220px; }
.tree { margin: 0; font-size: 12.5px; line-height: 1.8; color: #c9d1d9; background: var(--bg); border: 1px solid var(--border); border-radius: 8px; padding: 14px 16px; white-space: pre; overflow-x: auto; }
.warn-card { border-color: #5a4520; background: #1c1810; }
.warn-card h2 { display: flex; align-items: center; gap: 8px; color: var(--amber); }
.warn-card p { margin: 0; font-size: 13px; line-height: 1.7; color: #d9cbb0; }
dl { display: grid; grid-template-columns: 110px minmax(0, 1fr); row-gap: 10px; margin: 0 0 12px; font-size: 13px; }
dt { color: var(--muted); }
dd { margin: 0; word-break: break-all; }
@media (max-width: 1100px) { .grid { grid-template-columns: minmax(0, 1fr); } }
</style>
