import { computed, ref } from 'vue'

type Dict = Record<string, string>

const zh: Dict = {
  newSession: '新建会话', saveSnapshot: '保存快照', restoreAll: '恢复全部', sync: '同步', settings: '设置',
  search: '搜索会话或命令…', running: '运行中', saved: '已保存', templates: '模板', none: '（无）',
  windows: '窗口', panes: '面板', autosaveOn: '自动保存已开启', autosaveOff: '自动保存已关闭',
  autosaveEvery: '每 {n} 分钟保存一次快照', session: '会话', directory: '目录', created: '创建于',
  window: '窗口', layout: '当前窗口', command: '命令', history: '快照历史', viewHistory: '查看历史',
  saveAsTemplate: '另存为模板', noSession: '选择左侧的会话，或新建一个会话开始工作',
  noSessionTitle: '还没有打开会话', restore: '恢复', delete: '删除', rename: '重命名', close: '关闭',
  cancel: '取消', confirm: '确定', name: '名称', cwdOptional: '工作目录（可选）', fromTemplate: '从模板创建',
  blank: '空白会话', builtin: '内置', system: '系统', sessionsRunning: '运行中 {n} 个会话',
  lastPush: '上次同步', notSynced: '未配置远程仓库', back: '返回工作区', diffBase: '对比上一版本',
  restoreThis: '恢复这个版本', conflictTitle: '同名会话正在运行时', modeRename: '恢复为新会话',
  modeReplace: '替换正在运行的会话', modeSkip: '跳过', runCommands: '恢复后自动运行启动命令',
  steps: '恢复步骤', restoreSession: '恢复会话', localHost: '本机', otherHosts: '其他电脑',
  git: 'Git 仓库', savePolicy: '保存策略', tmuxConf: 'tmux', general: '通用', about: '关于',
  remote: '仓库地址', branch: '分支', auth: '认证方式', authNone: '无', authKey: 'SSH 密钥', authAgent: 'ssh-agent',
  authToken: 'HTTPS Token', keyPath: '私钥文件', passphrase: '私钥密码（可选）', username: '用户名', token: 'Token',
  savedSecret: '已保存，留空保持不变', testConn: '测试连接', author: '提交作者', email: '邮箱',
  autoCommit: '保存快照时自动提交', autoCommitHint: '每次保存生成一个提交', autoPush: '自动推送',
  autoPushHint: '提交后推送到远程，推送前先拉取', syncConf: '同步 tmux.conf', syncConfHint: '把 ~/.tmux.conf 纳入版本管理',
  repoDir: '本地快照目录', repoDirHint: '留空使用默认位置', saveSettings: '保存设置', discard: '放弃更改',
  autosaveMinutes: '自动保存间隔（分钟，0 为关闭）', scrollback: '保存终端滚动缓冲区',
  scrollbackHint: '会先过滤 Token、密码和私钥，但仍可能包含敏感输出', scrollbackLines: '保存行数',
  exclude: '不保存的会话（逗号分隔）', useSystemTmux: '使用系统 tmux 而不是内置 tmux',
  socket: 'tmux socket 名称', socketHint: '与你自己的 tmux 隔离，互不影响', language: '界面语言',
  privacyTitle: '隐私提示', privacy: '默认只保存布局、目录和启动命令，不保存终端输出。开启滚动缓冲区前，请确认仓库是私有的。',
  repoStructure: '仓库结构', components: '内置组件', gitEngine: 'Git 引擎', dataDir: '数据目录',
  version: '版本', settingsSaved: '设置已保存', connOk: '连接成功，可以同步',
  savedOk: '已保存：{list}', savedNone: '没有变化，无需保存', pushed: '，已推送', syncOk: '同步完成',
  restoredOk: '已恢复为会话 {name}', confirmKill: '关闭会话 {name}？会话中的程序会被结束。',
  confirmDelete: '删除快照 {name}？可以从 Git 历史中找回。', confirmDeleteTpl: '删除模板 {name}？',
  templateName: '模板名称', templateSaved: '已保存模板 {name}', newWindow: '新建窗口', noHistory: '还没有保存过这个会话',
  unpushed: '未推送', pushedTag: '已推送', selectCommit: '选择左侧的版本查看变化', current: '当前文件',
  fromHost: '来自 {host}', paletteHint: '回车执行', switchTo: '切换到 {name}', pullPush: '拉取并推送',
  noChanges: '与上一版本相同', loading: '加载中…', firstSave: '首次保存',
}

const en: Dict = {
  newSession: 'New session', saveSnapshot: 'Save snapshot', restoreAll: 'Restore all', sync: 'Sync', settings: 'Settings',
  search: 'Search sessions or commands…', running: 'Running', saved: 'Saved', templates: 'Templates', none: '(none)',
  windows: 'windows', panes: 'panes', autosaveOn: 'Autosave on', autosaveOff: 'Autosave off',
  autosaveEvery: 'Snapshot every {n} min', session: 'Session', directory: 'Directory', created: 'Created',
  window: 'Window', layout: 'Current window', command: 'Command', history: 'Snapshot history', viewHistory: 'History',
  saveAsTemplate: 'Save as template', noSession: 'Pick a session on the left or create a new one',
  noSessionTitle: 'No session open', restore: 'Restore', delete: 'Delete', rename: 'Rename', close: 'Close',
  cancel: 'Cancel', confirm: 'OK', name: 'Name', cwdOptional: 'Working directory (optional)', fromTemplate: 'From template',
  blank: 'Blank session', builtin: 'bundled', system: 'system', sessionsRunning: '{n} sessions running',
  lastPush: 'Last sync', notSynced: 'No remote configured', back: 'Back to workspace', diffBase: 'Compared with previous',
  restoreThis: 'Restore this version', conflictTitle: 'If a session with this name is running', modeRename: 'Restore as a new session',
  modeReplace: 'Replace the running session', modeSkip: 'Skip', runCommands: 'Run startup commands after restore',
  steps: 'Restore steps', restoreSession: 'Restore session', localHost: 'This computer', otherHosts: 'Other computers',
  git: 'Git repository', savePolicy: 'Saving', tmuxConf: 'tmux', general: 'General', about: 'About',
  remote: 'Remote URL', branch: 'Branch', auth: 'Authentication', authNone: 'None', authKey: 'SSH key', authAgent: 'ssh-agent',
  authToken: 'HTTPS token', keyPath: 'Private key', passphrase: 'Key passphrase (optional)', username: 'Username', token: 'Token',
  savedSecret: 'Saved; leave empty to keep', testConn: 'Test connection', author: 'Commit author', email: 'Email',
  autoCommit: 'Commit on save', autoCommitHint: 'Every save creates a commit', autoPush: 'Auto push',
  autoPushHint: 'Push after commit, pulling first', syncConf: 'Sync tmux.conf', syncConfHint: 'Version ~/.tmux.conf too',
  repoDir: 'Local snapshot folder', repoDirHint: 'Empty uses the default', saveSettings: 'Save settings', discard: 'Discard',
  autosaveMinutes: 'Autosave interval (minutes, 0 = off)', scrollback: 'Save scrollback',
  scrollbackHint: 'Tokens, passwords and keys are filtered, but output may still be sensitive', scrollbackLines: 'Lines',
  exclude: 'Sessions not to save (comma separated)', useSystemTmux: 'Use system tmux instead of the bundled one',
  socket: 'tmux socket name', socketHint: 'Keeps TmuxIDE separate from your own tmux', language: 'Language',
  privacyTitle: 'Privacy', privacy: 'Only layout, directories and startup commands are saved by default, not terminal output. Make sure the repository is private before enabling scrollback.',
  repoStructure: 'Repository layout', components: 'Bundled components', gitEngine: 'Git engine', dataDir: 'Data folder',
  version: 'Version', settingsSaved: 'Settings saved', connOk: 'Connected, ready to sync',
  savedOk: 'Saved: {list}', savedNone: 'Nothing changed', pushed: ', pushed', syncOk: 'Synced',
  restoredOk: 'Restored as {name}', confirmKill: 'Close session {name}? Programs in it will exit.',
  confirmDelete: 'Delete snapshot {name}? It stays in Git history.', confirmDeleteTpl: 'Delete template {name}?',
  templateName: 'Template name', templateSaved: 'Template {name} saved', newWindow: 'New window', noHistory: 'This session has not been saved yet',
  unpushed: 'not pushed', pushedTag: 'pushed', selectCommit: 'Pick a version to see what changed', current: 'Working copy',
  fromHost: 'from {host}', paletteHint: 'Enter to run', switchTo: 'Switch to {name}', pullPush: 'Pull and push',
  noChanges: 'Same as previous version', loading: 'Loading…', firstSave: 'First save',
}

const dicts: Record<string, Dict> = { 'zh-CN': zh, en }

export const lang = ref<string>('zh-CN')

export function t(key: string, vars: Record<string, string | number> = {}): string {
  const s = dicts[lang.value]?.[key] ?? zh[key] ?? key
  return s.replace(/\{(\w+)\}/g, (_, k) => String(vars[k] ?? ''))
}

export const languages = [
  { id: 'zh-CN', label: '简体中文' },
  { id: 'en', label: 'English' },
]

export const isZh = computed(() => lang.value.startsWith('zh'))

export function formatTime(ts: number | string): string {
  const d = typeof ts === 'number' ? new Date(ts * 1000) : new Date(ts)
  if (isNaN(d.getTime())) return String(ts)
  const now = new Date()
  const pad = (n: number) => String(n).padStart(2, '0')
  const hm = `${pad(d.getHours())}:${pad(d.getMinutes())}`
  if (d.toDateString() === now.toDateString()) return hm
  return `${pad(d.getMonth() + 1)}-${pad(d.getDate())} ${hm}`
}
