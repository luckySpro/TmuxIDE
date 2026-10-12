const zh = {
  connectTo: '连接到 {host}',
  pairIntro: '这台设备还没有配对。在电脑上打开“设置 → 远程访问 → 配对新设备”，扫码或输入配对码。',
  pairFromQr: '配对码已从二维码中读取，确认设备名称后连接。',
  code: '配对码', deviceName: '设备名称', deviceNameHint: '会显示在电脑的“已配对设备”里',
  pairBtn: '配对并连接', pairNote1: '配对码 5 分钟内有效，只能用一次。配对后这台设备获得独立凭证，电脑上可随时移除。',
  pinTitle: '输入 PIN', pinIntro: '为了安全，每次打开都需要输入在电脑上设置的 PIN。', unlock: '解锁',
  lockedTitle: '设备已锁定', lockedIntro: 'PIN 连续输错次数过多。请在电脑的“设置 → 远程访问 → 已配对设备”中解除锁定。',
  retry: '重试', live: '实时同步', offline: '连接中断，正在重连…', lock: '锁定', forget: '取消配对',
  confirmForget: '取消这台设备的配对？之后需要重新扫码。', noSessions: '电脑上没有运行中的会话',
  windows: '{n} 个窗口', usedOnDesktop: '电脑正在使用', panes: '{n} 面板', footer: '列表随电脑上的变化自动更新 · {device}',
  control: '可控制', readonly: '只读', keepSize: '保持电脑尺寸', fitPhone: '适配手机尺寸',
  fitHint: '适配手机会改变电脑上这个窗口的大小', inputPlaceholder: '输入命令，回车发送', send: '发送',
  interrupt: '^C 中断', clear: '清屏', keyboard: '键盘', reconnecting: '终端已断开，正在重连…',
  ended: '会话已结束', back: '返回终端列表', readonlyNote: '这台设备是只读的，只能查看',
  error: '出错了',
}
type Key = keyof typeof zh
const en: Record<Key, string> = {
  connectTo: 'Connect to {host}',
  pairIntro: 'This device is not paired yet. On your computer open Settings → Remote access → Pair new device, then scan the QR code or enter the code.',
  pairFromQr: 'Pairing code read from the QR code. Confirm the device name to connect.',
  code: 'Pairing code', deviceName: 'Device name', deviceNameHint: 'Shown in “Paired devices” on your computer',
  pairBtn: 'Pair and connect', pairNote1: 'Codes are valid for 5 minutes and can be used once. This device gets its own credential, which you can revoke on your computer at any time.',
  pinTitle: 'Enter PIN', pinIntro: 'For safety, enter the PIN set on your computer each time you open this page.', unlock: 'Unlock',
  lockedTitle: 'Device locked', lockedIntro: 'Too many wrong PINs. Unlock this device on your computer under Settings → Remote access → Paired devices.',
  retry: 'Retry', live: 'Live', offline: 'Disconnected, reconnecting…', lock: 'Lock', forget: 'Unpair',
  confirmForget: 'Unpair this device? You will need to scan the code again.', noSessions: 'No running sessions on the computer',
  windows: '{n} windows', usedOnDesktop: 'In use on computer', panes: '{n} panes', footer: 'Updates live · {device}',
  control: 'Control', readonly: 'Read-only', keepSize: 'Keep computer size', fitPhone: 'Fit to phone',
  fitHint: 'Fitting resizes this window on your computer too', inputPlaceholder: 'Type a command, Enter to send', send: 'Send',
  interrupt: '^C', clear: 'Clear', keyboard: 'Keyboard', reconnecting: 'Terminal disconnected, reconnecting…',
  ended: 'Session ended', back: 'Back to terminals', readonlyNote: 'This device is read-only',
  error: 'Error',
}

const dict: Record<Key, string> = navigator.language.toLowerCase().startsWith('zh') ? zh : en

export function t(key: Key, vars: Record<string, string | number> = {}) {
  return dict[key].replace(/\{(\w+)\}/g, (_, k) => String(vars[k] ?? ''))
}
