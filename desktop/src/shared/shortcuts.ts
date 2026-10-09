/**
 * Menu commands the main process forwards to the renderer, and the keys that
 * run them. One table feeds the native menu's accelerators and the shortcut
 * hints in tooltips, so the two cannot disagree.
 *
 * Panel commands use ⌘⇧/Ctrl+Shift: a bare Ctrl+letter belongs to the shell
 * in the embedded terminal on Windows and Linux (Ctrl+B, Ctrl+F, Ctrl+[).
 */
export type MenuAction =
  | 'new-chat'
  | 'settings'
  | 'updates'
  | 'search'
  | 'toggle-sidebar'
  | 'panel:resources'
  | 'panel:changes'
  | 'panel:terminal'
  | 'panel:browser'
  | 'back'
  | 'forward'
  | 'previous-chat'
  | 'next-chat'
  | 'find'
  | 'approve'
  | 'deny'

type Keys = { mac: string; other: string }
const same = (keys: string): Keys => ({ mac: keys, other: keys })

export const shortcuts: Partial<Record<MenuAction, Keys>> = {
  'new-chat': same('CmdOrCtrl+N'),
  settings: same('CmdOrCtrl+,'),
  search: same('CmdOrCtrl+K'),
  'toggle-sidebar': same('CmdOrCtrl+Shift+S'),
  'panel:resources': same('CmdOrCtrl+Shift+A'),
  'panel:changes': same('CmdOrCtrl+Shift+G'),
  'panel:terminal': same('CmdOrCtrl+Shift+T'),
  'panel:browser': same('CmdOrCtrl+Shift+B'),
  back: { mac: 'Cmd+[', other: 'Alt+Left' },
  forward: { mac: 'Cmd+]', other: 'Alt+Right' },
  'previous-chat': same('Ctrl+Shift+Tab'),
  'next-chat': same('Ctrl+Tab'),
  find: { mac: 'Cmd+F', other: 'Ctrl+Shift+F' },
  approve: same('CmdOrCtrl+Shift+Enter'),
  deny: same('CmdOrCtrl+Shift+Backspace')
}

/** The Electron accelerator for a command on this platform. */
export function accelerator(action: MenuAction, platform: string): string | undefined {
  const keys = shortcuts[action]
  return keys && (platform === 'darwin' ? keys.mac : keys.other)
}

const macSymbols: Record<string, string> = {
  CmdOrCtrl: '⌘',
  Cmd: '⌘',
  Ctrl: '⌃',
  Alt: '⌥',
  Shift: '⇧',
  Enter: '↩',
  Backspace: '⌫',
  Tab: '⇥',
  Left: '←',
  Right: '→'
}

/** How a command's keys read in a tooltip: `⌘⇧T` on macOS, `Ctrl+Shift+T` elsewhere. */
export function shortcutLabel(action: MenuAction, platform: string): string {
  const keys = accelerator(action, platform)
  if (!keys) return ''
  const parts = keys.split('+')
  return platform === 'darwin'
    ? parts.map((part) => macSymbols[part] ?? part).join('')
    : parts.map((part) => (part === 'CmdOrCtrl' ? 'Ctrl' : part)).join('+')
}
