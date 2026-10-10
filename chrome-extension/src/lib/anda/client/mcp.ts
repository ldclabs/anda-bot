import type { DaemonApi } from './daemon'
import type {
  Json,
  McpChange,
  McpEntry,
  McpReceipt,
  McpSecretView,
  McpServerDetail,
  McpSignIn,
  McpSnapshot,
  McpTestReport,
  McpToolDiff
} from './types'

/** A refusal from the daemon's MCP API, with its stable code (`not_found`, `revision_conflict`, …). */
export class McpApiError extends Error {
  constructor(
    readonly code: string,
    message: string
  ) {
    super(message)
    this.name = 'McpApiError'
  }
}

type McpReply<Result> = { result: Result } | { error: { code?: string; message?: string } }

/**
 * The owner's MCP servers, as the MCP page uses them: the daemon's `mcp_*`
 * methods. Every change fires `mcp-changed`, so open views refresh.
 *
 * Reads answer with an empty snapshot when no token is configured, so the
 * page can render before the daemon connects.
 */
export class McpApi extends EventTarget {
  #daemon: DaemonApi

  constructor(daemon: DaemonApi) {
    super()
    this.#daemon = daemon
  }

  async list(): Promise<McpSnapshot> {
    if (!this.#daemon.authorized) {
      return emptySnapshot()
    }
    return this.#call<McpSnapshot>('mcp_list')
  }

  get(id: string): Promise<McpServerDetail> {
    return this.#call('mcp_get', { id })
  }

  toolDiff(id: string, tool: string): Promise<McpToolDiff> {
    return this.#call('mcp_tool_diff', { id, tool })
  }

  /** The stored secrets, by name only. */
  secrets(): Promise<McpSecretView[]> {
    return this.#call('mcp_secrets')
  }

  /**
   * Tries an entry without saving it. `secrets` are values for its
   * `${secret:NAME}` references that are not stored yet, used for this test only.
   */
  test(server: McpEntry, secrets: Record<string, string> = {}): Promise<McpTestReport> {
    return this.#call('mcp_test', { server, secrets })
  }

  async apply(change: McpChange, expectedRevision?: string): Promise<McpReceipt> {
    const receipt = await this.#call<McpReceipt>('mcp_apply', {
      change,
      ...(expectedRevision ? { expected_revision: expectedRevision } : {})
    })
    this.notifyChanged()
    return receipt
  }

  /** Connects a server again, or without an id every failed one. */
  async reconnect(id?: string): Promise<McpReceipt> {
    const receipt = await this.#call<McpReceipt>('mcp_reconnect', id ? { id } : {})
    this.notifyChanged()
    return receipt
  }

  /** Starts an OAuth sign-in; the caller opens `authorization_url` when one comes back. */
  async signIn(request: { id?: string; url?: string; reauthorize?: boolean }): Promise<McpSignIn> {
    const result = await this.#call<McpSignIn>('mcp_sign_in', request)
    this.notifyChanged()
    return result
  }

  async signOut(id: string): Promise<void> {
    await this.#call('mcp_sign_out', { id })
    this.notifyChanged()
  }

  /** Applies mcp.json as it is on disk. */
  async reload(): Promise<McpReceipt> {
    const receipt = await this.#call<McpReceipt>('mcp_reload')
    this.notifyChanged()
    return receipt
  }

  notifyChanged(): void {
    this.dispatchEvent(new Event('mcp-changed'))
  }

  async #call<Result>(method: string, params: Record<string, unknown> = {}): Promise<Result> {
    const reply = await this.#daemon.rpc<McpReply<Result>>(method, [params])
    if (!reply || typeof reply !== 'object') {
      throw new McpApiError('failed', `${method} returned nothing`)
    }
    if ('error' in reply && reply.error) {
      throw new McpApiError(reply.error.code || 'failed', reply.error.message || `${method} failed`)
    }
    return (reply as { result: Result }).result
  }
}

export function emptySnapshot(): McpSnapshot {
  return {
    config_path: '',
    revision: '',
    config_changed_on_disk: false,
    running: false,
    servers: []
  }
}

/** Servers found in pasted configuration, or why none were. */
export interface ParsedMcpConfig {
  servers: McpEntry[]
  error?: 'invalid_json' | 'no_servers' | 'needs_id'
}

/**
 * Reads configuration pasted from another MCP client: a whole file
 * (`mcpServers` as Claude, Cursor and Anda write it, or VS Code's
 * `servers`), or a single entry, which takes `fallbackId`.
 */
export function parseMcpConfig(text: string, fallbackId = ''): ParsedMcpConfig {
  let root: unknown
  try {
    root = JSON.parse(text)
  } catch {
    return { servers: [], error: 'invalid_json' }
  }
  if (!isObject(root)) {
    return { servers: [], error: 'no_servers' }
  }
  const servers: McpEntry[] = []
  for (const key of ['mcpServers', 'servers']) {
    const entries = root[key]
    if (!isObject(entries)) continue
    for (const [id, entry] of Object.entries(entries)) {
      if (isObject(entry) && id.trim()) {
        servers.push({ ...entry, id: id.trim() } as McpEntry)
      }
    }
  }
  if (servers.length) {
    return { servers }
  }
  if (typeof root.command === 'string' || typeof root.url === 'string') {
    const id = (typeof root.id === 'string' && root.id.trim()) || fallbackId.trim()
    return id ? { servers: [{ ...root, id } as McpEntry] } : { servers: [], error: 'needs_id' }
  }
  return { servers: [], error: 'no_servers' }
}

/** An entry for a remote server; `headers` holds one `Name: value` per line. */
export function remoteEntry(id: string, url: string, headers: string): McpEntry {
  const entry: McpEntry = { id: id.trim(), type: 'http', url: url.trim() }
  const parsed = keyValueLines(headers, ':')
  if (Object.keys(parsed).length) entry.headers = parsed
  return entry
}

/** An entry for a local command; `env` holds one `KEY=value` per line. */
export function localEntry(id: string, commandLine: string, env: string): McpEntry {
  const [command = '', ...args] = splitCommandLine(commandLine)
  const entry: McpEntry = { id: id.trim(), command }
  if (args.length) entry.args = args
  const parsed = keyValueLines(env, '=')
  if (Object.keys(parsed).length) entry.env = parsed
  return entry
}

/** Splits a command line on spaces, keeping quoted parts whole. Nothing is expanded. */
export function splitCommandLine(line: string): string[] {
  const parts: string[] = []
  let current = ''
  let quote = ''
  let started = false
  for (const ch of line.trim()) {
    if (quote) {
      if (ch === quote) quote = ''
      else current += ch
    } else if (ch === '"' || ch === "'") {
      quote = ch
      started = true
    } else if (/\s/.test(ch)) {
      if (started) parts.push(current)
      current = ''
      started = false
    } else {
      current += ch
      started = true
    }
  }
  if (started) parts.push(current)
  return parts
}

/** A short id for a new server: the meaningful part of its host or package name. */
export function suggestServerId(source: string): string {
  const text = source.trim()
  let name = ''
  try {
    const url = new URL(text)
    if (url.protocol === 'http:' || url.protocol === 'https:') {
      const labels = url.hostname.split('.').filter(Boolean)
      name =
        labels.find((label, index) => index < labels.length - 1 && !genericHost(label)) ||
        labels[0] ||
        ''
    }
  } catch {
    const args = splitCommandLine(text).filter((arg) => !arg.startsWith('-'))
    const last = args[args.length - 1] || ''
    name = (last.split('/').pop() || '').replace(/@[^@]*$/, '')
  }
  const id = name
    .toLowerCase()
    .replace(/^mcp[-_]|[-_]mcp(?:[-_]server)?$|[-_]server$/g, '')
    .replace(/[^a-z0-9_-]+/g, '_')
    .replace(/^_+|_+$/g, '')
  return id || 'mcp'
}

/** The names `${secret:NAME}` references in a value. */
export function secretReferences(value: string): string[] {
  const names = new Set<string>()
  for (const match of value.matchAll(/\$\{secret:([A-Za-z_][A-Za-z0-9_]*)(?::-[^}]*)?\}/g)) {
    names.add(match[1])
  }
  return [...names]
}

/**
 * Moves the plaintext credentials of an entry (header and env values, and a
 * bearer token) into secrets: each value becomes `${secret:<ID>_<KEY>}` and
 * the returned `secrets` hold the values to store. A value that only
 * references variables or secrets is left as it is, and `taken` names are
 * not reused.
 */
export function moveCredentialsToSecrets(
  entry: McpEntry,
  taken: ReadonlySet<string> = new Set()
): {
  entry: McpEntry
  secrets: Record<string, string>
} {
  const secrets: Record<string, string> = {}
  const next: McpEntry = { ...entry }
  const prefix = secretName(entry.id)
  const convert = (key: string, value: Json): Json => {
    if (typeof value !== 'string' || !value.trim() || onlyReferences(value)) return value
    let name = secretName(`${prefix}_${key}`)
    for (let n = 2; name in secrets || taken.has(name); n += 1) {
      name = secretName(`${prefix}_${key}_${n}`)
    }
    secrets[name] = value
    return `\${secret:${name}}`
  }
  for (const field of ['headers', 'env', 'environment']) {
    const values = entry[field]
    if (isObject(values)) {
      next[field] = Object.fromEntries(
        Object.entries(values).map(([key, value]) => [key, convert(key, value as Json)])
      )
    }
  }
  if (typeof entry.bearer_token === 'string') {
    next.bearer_token = convert('TOKEN', entry.bearer_token)
  }
  return { entry: next, secrets }
}

/** A secret name built from free text: letters, digits and `_`, upper case. */
export function secretName(text: string): string {
  const name = text
    .toUpperCase()
    .replace(/[^A-Z0-9_]+/g, '_')
    .replace(/^_+|_+$/g, '')
  return /^[A-Z_]/.test(name) ? name : `_${name}`
}

function onlyReferences(value: string): boolean {
  return /^(\$\{[^}]+\}|\$[A-Za-z_][A-Za-z0-9_]*)+$/.test(value.trim())
}

function genericHost(label: string): boolean {
  return ['www', 'api', 'mcp', 'app', 'server'].includes(label.toLowerCase())
}

function keyValueLines(text: string, separator: string): Record<string, string> {
  const values: Record<string, string> = {}
  for (const line of text.split(/\r?\n/)) {
    const at = line.indexOf(separator)
    if (at <= 0) continue
    const key = line.slice(0, at).trim()
    if (key) values[key] = line.slice(at + 1).trim()
  }
  return values
}

function isObject(value: unknown): value is Record<string, unknown> {
  return Boolean(value) && typeof value === 'object' && !Array.isArray(value)
}
