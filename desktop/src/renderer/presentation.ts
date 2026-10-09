import { shortcuts, type MenuAction } from '../shared/shortcuts'

/**
 * The part of a configured model name people recognize: `openai:gpt-5.1`
 * reads `gpt-5.1`, and a ChatGPT plan's `chatgpt:<account>:<model>` its model.
 */
export function modelLabel(name: string): string {
  const last = name.split(':').at(-1)?.trim()
  return last || name
}

export type DiffLineKind = 'add' | 'del' | 'hunk' | 'meta' | 'context'

/** How a unified-diff line is coloured. */
export function diffLineKind(line: string): DiffLineKind {
  if (
    /^(diff --git |index |--- |\+\+\+ |new file mode|deleted file mode|similarity index|rename (from|to) |Binary files )/.test(
      line
    )
  )
    return 'meta'
  if (line.startsWith('@@')) return 'hunk'
  if (line.startsWith('+')) return 'add'
  if (line.startsWith('-')) return 'del'
  return 'context'
}

/** `950`, `12.3k`, `4.1M`. */
export function formatTokens(count: number): string {
  if (count < 1000) return String(count)
  if (count < 1_000_000) return `${(count / 1000).toFixed(count < 10_000 ? 1 : 0)}k`
  return `${(count / 1_000_000).toFixed(1)}M`
}

/** Share of the context window in use, 0–100; undefined when the window is unknown. */
export function contextPercent(tokens: number, window: number): number | undefined {
  if (!(window > 0)) return undefined
  return Math.min(100, Math.round((tokens / window) * 100))
}

/** Share of input tokens read from the prompt cache, 0–100. */
export function cacheHitPercent(input: number, cached: number): number {
  return input > 0 ? Math.min(100, Math.round((cached / input) * 100)) : 0
}

/** `0:07`, `12:30`, `1:02:05`. */
export function formatElapsed(ms: number): string {
  const total = Math.max(0, Math.floor(ms / 1000))
  const hours = Math.floor(total / 3600)
  const minutes = Math.floor((total % 3600) / 60)
  const seconds = String(total % 60).padStart(2, '0')
  return hours ? `${hours}:${String(minutes).padStart(2, '0')}:${seconds}` : `${minutes}:${seconds}`
}

/** Whether a key press is a renderer-handled shortcut from the shared table. */
export function matchesShortcut(
  event: KeyboardEvent,
  action: MenuAction,
  platform: string
): boolean {
  const keys = shortcuts[action]
  if (!keys) return false
  const parts = (platform === 'darwin' ? keys.mac : keys.other).split('+')
  const key = parts.at(-1)!
  const wants = new Set(parts.slice(0, -1))
  const primary = platform === 'darwin' ? event.metaKey : event.ctrlKey
  const command = wants.has('CmdOrCtrl') || wants.has(platform === 'darwin' ? 'Cmd' : 'Ctrl')
  if (command !== primary) return false
  if (platform === 'darwin' && wants.has('Ctrl') !== event.ctrlKey) return false
  if (platform !== 'darwin' && event.metaKey) return false
  if (wants.has('Shift') !== event.shiftKey || wants.has('Alt') !== event.altKey) return false
  return event.key.toLowerCase() === key.toLowerCase()
}
