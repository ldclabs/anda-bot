import { decodeQuotedString } from '../client/conversations'
import type { ChatToolCall, Json } from '../client/types'

/**
 * How a tool call reads in the transcript: a one-line row (kind, name, the
 * argument that identifies the call, status) that expands into its input and
 * output. Pure functions only, so the transcript component just renders.
 */

export type ToolCallStatus = 'running' | 'ok' | 'error'
export type ToolKind = 'shell' | 'file' | 'memory' | 'web' | 'agent' | 'tool'

export interface ToolDetailSection {
  kind: 'input' | 'output' | 'stderr'
  text: string
  meta?: string
}

export interface RuntimeNotice {
  kind: string
  body: string
}

/** Arguments that best identify a call, most specific first. */
const SUMMARY_KEYS = [
  'command',
  'cmd',
  'path',
  'file_path',
  'paths',
  'url',
  'query',
  'pattern',
  'q',
  'prompt',
  'task',
  'goal',
  'name',
  'title',
  'text',
  'message',
  'id'
]
const PATH_KEYS = new Set(['path', 'file_path', 'paths'])
const SUMMARY_MAX_CHARS = 240
const DETAIL_MAX_CHARS = 50_000

export function toolKind(name: string): ToolKind {
  const lower = name.toLowerCase()
  if (/shell|exec|command|terminal/.test(lower)) return 'shell'
  if (/memory|recall|brain|kip/.test(lower)) return 'memory'
  if (/file|read|write|edit|dir|glob|grep/.test(lower)) return 'file'
  if (/browser|web|fetch|url|http|search|page/.test(lower)) return 'web'
  if (/agent|skill|task/.test(lower)) return 'agent'
  return 'tool'
}

export function toolCallStatus(tool: ChatToolCall): ToolCallStatus {
  if (tool.output === undefined) return 'running'
  return toolOutputFailed(parseJsonString(tool.output)) ? 'error' : 'ok'
}

function toolOutputFailed(output: Json | undefined): boolean {
  if (!isRecord(output)) return false
  const error = output.error
  if ((typeof error === 'string' && error.trim()) || isRecord(error)) return true
  if (typeof output.exit_code === 'number' && output.exit_code !== 0) return true
  return output.is_error === true || output.isError === true || output.success === false
}

/** The single line shown next to the tool name, e.g. a shell command or a path. */
export function toolCallSummary(tool: ChatToolCall): string {
  const args = parseJsonString(tool.args)
  if (args === undefined || args === null) return ''
  if (!isRecord(args)) return oneLine(typeof args === 'string' ? args : JSON.stringify(args))

  for (const key of SUMMARY_KEYS) {
    const value = summaryValue(args[key], PATH_KEYS.has(key))
    if (value) return value
  }
  for (const value of Object.values(args)) {
    const text = summaryValue(value, false)
    if (text) return text
  }
  return oneLine(JSON.stringify(args))
}

function summaryValue(value: Json | undefined, isPath: boolean): string {
  if (typeof value === 'string' && value.trim()) {
    return isPath ? shortenPath(oneLine(value)) : oneLine(value)
  }
  if (Array.isArray(value)) {
    const items = value.filter((item): item is string => typeof item === 'string' && !!item.trim())
    if (items.length)
      return oneLine(items.map((item) => (isPath ? shortenPath(item) : item)).join(', '))
  }
  return ''
}

/** Keeps the tail of a long path, which is the part that tells calls apart. */
function shortenPath(path: string): string {
  if (path.length <= 48) return path
  const segments = path.split('/').filter(Boolean)
  return segments.length > 3 ? `…/${segments.slice(-3).join('/')}` : path
}

function oneLine(text: string): string {
  const line = text.replace(/\s+/g, ' ').trim()
  return line.length > SUMMARY_MAX_CHARS ? `${line.slice(0, SUMMARY_MAX_CHARS)}…` : line
}

/** Input and output as plain text blocks; shell results show their streams, not the envelope. */
export function toolDetailSections(tool: ChatToolCall): ToolDetailSection[] {
  const sections: ToolDetailSection[] = []
  const args = parseJsonString(tool.args)
  if (args !== undefined && args !== null && !isEmptyRecord(args)) {
    if (isRecord(args) && typeof args.command === 'string') {
      const cwd = typeof args.cwd === 'string' ? args.cwd : undefined
      sections.push({ kind: 'input', text: capped(args.command.trim()), meta: cwd })
    } else {
      sections.push({ kind: 'input', text: capped(jsonText(args)) })
    }
  }

  if (tool.output === undefined) return sections
  const output = parseJsonString(tool.output)
  if (isRecord(output) && ('stdout' in output || 'stderr' in output)) {
    const stdout = typeof output.stdout === 'string' ? output.stdout.trimEnd() : ''
    const stderr = typeof output.stderr === 'string' ? output.stderr.trimEnd() : ''
    const meta = shellOutputMeta(output)
    if (stdout || !stderr) sections.push({ kind: 'output', text: capped(stdout), meta })
    if (stderr)
      sections.push({ kind: 'stderr', text: capped(stderr), meta: stdout ? undefined : meta })
    return sections
  }
  if (isRecord(output) && typeof output.error === 'string' && Object.keys(output).length === 1) {
    sections.push({ kind: 'output', text: capped(output.error.trim()) })
    return sections
  }
  sections.push({ kind: 'output', text: capped(jsonText(output ?? null)) })
  return sections
}

function shellOutputMeta(output: Record<string, Json>): string | undefined {
  const parts: string[] = []
  if (typeof output.exit_code === 'number' && output.exit_code !== 0) {
    parts.push(`exit ${output.exit_code}`)
  }
  if (typeof output.state === 'string' && output.state && output.state !== 'exited') {
    parts.push(output.state)
  }
  return parts.join(' · ') || undefined
}

/**
 * Splits runtime-injected text (`[$system: kind="…"]` + preamble + quoted
 * body, possibly several joined by `---`) into its notices.
 */
export function runtimeNotices(text: string): RuntimeNotice[] {
  const headers = [...text.matchAll(/\[\$system:\s*kind=("(?:\\.|[^"\\])*")\]/g)]
  if (!headers.length) {
    return text.trim() ? [{ kind: '', body: text.trim() }] : []
  }
  return headers.map((header, index) => {
    const start = (header.index ?? 0) + header[0].length
    const end = headers[index + 1]?.index ?? text.length
    const segment = text
      .slice(start, end)
      .trim()
      .replace(/\n+-{3}$/, '')
      .trim()
    const preambleEnd = segment.indexOf('\n\n')
    const body = preambleEnd >= 0 ? segment.slice(preambleEnd + 2) : segment
    return { kind: decodeQuotedString(header[1]), body: decodeQuotedString(body) }
  })
}

export function firstLine(text: string): string {
  const line = text.trim().split('\n', 1)[0] || ''
  return oneLine(line.replace(/^[#>*\-\s]+/, ''))
}

function parseJsonString(value: Json | undefined): Json | undefined {
  if (typeof value !== 'string') return value
  const trimmed = value.trim()
  if (!/^[[{]/.test(trimmed)) return value
  try {
    return JSON.parse(trimmed) as Json
  } catch {
    return value
  }
}

function jsonText(value: Json): string {
  return typeof value === 'string' ? value : JSON.stringify(value, null, 2)
}

function capped(text: string): string {
  return text.length > DETAIL_MAX_CHARS ? `${text.slice(0, DETAIL_MAX_CHARS)}\n…` : text
}

function isRecord(value: Json | undefined): value is Record<string, Json> {
  return Boolean(value) && typeof value === 'object' && !Array.isArray(value)
}

function isEmptyRecord(value: Json): boolean {
  return isRecord(value) && Object.keys(value).length === 0
}
