import { toolCallStatus } from '$lib/anda/chat/tool-view'
import type { ChatMessage, Json } from '$lib/anda/client/types'

/** A file the agent changed during one turn, with the lines its edits touched. */
export interface EditedFile {
  path: string
  additions: number
  /** Unknown for a whole-file write, which does not say what it replaced. */
  deletions: number
}

function args(value: Json | undefined): Record<string, Json> | null {
  let parsed = value
  if (typeof parsed === 'string') {
    try {
      parsed = JSON.parse(parsed) as Json
    } catch {
      return null
    }
  }
  return parsed && typeof parsed === 'object' && !Array.isArray(parsed) ? parsed : null
}

function lines(text: string): string[] {
  if (!text) return []
  const split = text.split('\n')
  if (text.endsWith('\n')) split.pop()
  return split
}

/** Lines an exact-string edit removes and adds, without the context they share. */
function editStats(before: string, after: string): { additions: number; deletions: number } {
  const old = lines(before)
  const next = lines(after)
  let start = 0
  while (start < old.length && start < next.length && old[start] === next[start]) start++
  let end = 0
  while (
    end < old.length - start &&
    end < next.length - start &&
    old[old.length - 1 - end] === next[next.length - 1 - end]
  )
    end++
  return { additions: next.length - start - end, deletions: old.length - start - end }
}

/**
 * The files a turn's successful `edit_file` and `write_file` calls changed,
 * in the order they were first touched.
 */
export function editedFiles(messages: ChatMessage[]): EditedFile[] {
  const files = new Map<string, EditedFile>()
  for (const message of messages) {
    for (const tool of message.tools || []) {
      if (tool.name !== 'edit_file' && tool.name !== 'write_file') continue
      if (toolCallStatus(tool) !== 'ok') continue
      const input = args(tool.args)
      const path = typeof input?.path === 'string' ? input.path.trim() : ''
      if (!path) continue
      const stats =
        tool.name === 'edit_file'
          ? editStats(String(input?.old_string ?? ''), String(input?.new_string ?? ''))
          : { additions: lines(String(input?.content ?? '')).length, deletions: 0 }
      const file = files.get(path) || { path, additions: 0, deletions: 0 }
      file.additions += stats.additions
      file.deletions += stats.deletions
      files.set(path, file)
    }
  }
  return [...files.values()]
}

/** A path relative to the workspace when it lies inside it, with `/` separators. */
export function workspaceRelative(path: string, workspace: string | undefined): string {
  const normalized = path.replace(/\\/g, '/')
  if (!workspace) return normalized
  const root = workspace.replace(/\\/g, '/').replace(/\/+$/, '')
  return normalized.startsWith(`${root}/`) ? normalized.slice(root.length + 1) : normalized
}

export interface FileTarget {
  path: string
  line?: number
}

// Bare file names count only with an extension people put in prose.
const KNOWN_EXTENSIONS = new Set(
  (
    'c cc cfg conf cpp cs css csv go h hpp html ini java js json jsx kt lock log md mdx mjs ' +
    'cjs mts cts php py rb rs scss sh sql svelte swift toml ts tsx txt vue xml yaml yml zsh'
  ).split(' ')
)
const LINE_SUFFIX = /(?::(\d+)(?::\d+)?|#L(\d+)(?:-L?\d+)?)$/

/**
 * A file named by inline code or a link in the transcript: `src/app.ts`,
 * `./a/b.rs:42`, `C:\repo\x.ts`, `README.md`. Anything with a scheme other
 * than `file:` (a web link), spaces, or call syntax is not one.
 */
export function fileTarget(raw: string): FileTarget | null {
  let text = raw.trim()
  if (!text || text.length > 1024) return null
  if (text.startsWith('file://')) {
    try {
      text = decodeURIComponent(new URL(text).pathname).replace(/^\/([A-Za-z]:\/)/, '$1')
    } catch {
      return null
    }
  } else if (/^[a-z][a-z0-9+.-]*:/i.test(text) && !/^[A-Za-z]:[\\/]/.test(text)) return null
  const suffix = LINE_SUFFIX.exec(text)
  const line = suffix ? Number(suffix[1] || suffix[2]) : undefined
  if (suffix) text = text.slice(0, suffix.index)
  if (!/^[\w@.+~:\\/-]+$/.test(text) || text.endsWith('/') || text.endsWith('\\')) return null
  const name = text.split(/[\\/]/).at(-1) || ''
  const extension = /\.([A-Za-z][A-Za-z0-9]{0,9})$/.exec(name)?.[1]?.toLowerCase()
  const nested = /[\\/]/.test(text)
  if (
    nested ? !name || name === '.' || name === '..' : !extension || !KNOWN_EXTENSIONS.has(extension)
  )
    return null
  if (nested && !extension && !/^\.?[\w-]+$/.test(name)) return null
  return line ? { path: text, line } : { path: text }
}

/** The transcript as Markdown for the clipboard: who said what, in order. */
export function conversationMarkdown(
  title: string,
  messages: ChatMessage[],
  names: { user: string; assistant: string }
): string {
  const parts = [`# ${title}`]
  for (const message of messages) {
    const text = message.text.trim()
    if (!text) continue
    if (message.role === 'user') parts.push(`**${names.user}:**\n\n${text}`)
    else if (message.role === 'assistant') parts.push(`**${names.assistant}:**\n\n${text}`)
  }
  return `${parts.join('\n\n---\n\n')}\n`
}
