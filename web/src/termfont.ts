import './termfont.css'

/** 终端字体：正文用 JetBrains Mono，图标依次找用户装的 Nerd Font，最后用内置的符号字体 */
export const TERM_FONT =
  "'JetBrains Mono', 'MesloLGS NF', 'JetBrainsMono Nerd Font', 'Hack Nerd Font', 'FiraCode Nerd Font', 'Symbols Nerd Font Mono', ui-monospace, Menlo, monospace"

/** 字体加载完成后重绘终端，避免先画出的图标停留在方框 */
export const termFontReady: Promise<unknown> = Promise.all([
  document.fonts.load("13px 'Symbols Nerd Font Mono'", ''),
  document.fonts.load("13px 'JetBrains Mono'"),
]).catch(() => undefined)
