import type { DaemonApi } from './daemon'
import type {
  Json,
  McpChange,
  McpEntry,
  McpEventsView,
  McpImportRequest,
  McpImportScan,
  McpImportSource,
  McpReceipt,
  McpRegistryInput,
  McpRegistryPackage,
  McpRegistryPage,
  McpRegistryServer,
  McpResourceAttachment,
  McpResourceListing,
  McpSecretView,
  McpServerDetail,
  McpServerView,
  McpSignIn,
  McpSnapshot,
  McpTestReport,
  McpToolDiff,
  McpToolView,
  McpTrigger,
  McpTriggerChange,
  McpTriggerDetail
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

  /** Reads the other MCP clients' configuration on this computer; nothing is changed. */
  importScan(sources: McpImportSource[] = [], workspaces: string[] = []): Promise<McpImportScan> {
    return this.#call('mcp_import_scan', { sources, workspaces })
  }

  /** Imports servers a scan found, by their keys; the daemon reads their files again. */
  async import(request: McpImportRequest): Promise<McpReceipt> {
    const receipt = await this.#call<McpReceipt>('mcp_import', { ...request })
    this.notifyChanged()
    return receipt
  }

  /** One page of the MCP Registry's servers, searched by name. */
  registrySearch(query: string, cursor?: string): Promise<McpRegistryPage> {
    return this.#call('mcp_registry_search', cursor ? { query, cursor } : { query })
  }

  /** A server's event types (MCP Events) and the automations that run on them. */
  events(id: string): Promise<McpEventsView> {
    return this.#call('mcp_events_list', { id })
  }

  /** The automations, or a server's only. */
  triggers(serverId?: string): Promise<McpTrigger[]> {
    return this.#call('mcp_triggers_list', serverId ? { server_id: serverId } : {})
  }

  /** One automation with its latest runs and events. */
  trigger(id: number): Promise<McpTriggerDetail> {
    return this.#call('mcp_trigger_get', { id })
  }

  /** Creates, changes, pauses, resumes or deletes an automation. */
  async applyTrigger(change: McpTriggerChange): Promise<McpTriggerDetail | { deleted: number }> {
    const result = await this.#call<McpTriggerDetail | { deleted: number }>('mcp_trigger_apply', {
      change
    })
    this.notifyChanged()
    return result
  }

  /** The resources of a connected server, or of each. */
  async resources(id?: string): Promise<McpResourceListing[]> {
    const result = await this.#call<{ servers: McpResourceListing[] }>(
      'mcp_resources',
      id ? { id } : {}
    )
    return result.servers
  }

  /** Reads a resource to attach to a message: each of its contents as a file. */
  async readResource(id: string, uri: string): Promise<McpResourceAttachment[]> {
    const result = await this.#call<{ attachments: McpResourceAttachment[] }>('mcp_resource_read', {
      id,
      uri
    })
    return result.attachments
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

/** The clients an import reads; product names read the same in every language. */
export const IMPORT_SOURCE_LABELS: Record<McpImportSource, string> = {
  claude_desktop: 'Claude Desktop',
  claude_code: 'Claude Code',
  cursor: 'Cursor',
  vscode: 'VS Code',
  windsurf: 'Windsurf',
  codex: 'Codex'
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
 * `servers`), or a single entry, which takes `fallbackId`. Other clients'
 * spellings become Anda's: see {@link fromOtherClient}.
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
        servers.push(fromOtherClient({ ...entry, id: id.trim() } as McpEntry))
      }
    }
  }
  if (servers.length) {
    return { servers }
  }
  if (
    typeof root.command === 'string' ||
    typeof root.url === 'string' ||
    typeof root.serverUrl === 'string'
  ) {
    const id = (typeof root.id === 'string' && root.id.trim()) || fallbackId.trim()
    return id
      ? { servers: [fromOtherClient({ ...root, id } as McpEntry)] }
      : { servers: [], error: 'needs_id' }
  }
  return { servers: [], error: 'no_servers' }
}

/**
 * An entry as another client writes it, in Anda's spelling: Windsurf's
 * `serverUrl` is `url`, Copilot's `local` type is `stdio`, VS Code's
 * `${env:NAME}` is `${NAME}` and its `${input:id}` a secret to set,
 * `${secret:ID}`. The daemon's import translates the same way.
 */
export function fromOtherClient(entry: McpEntry): McpEntry {
  const next: McpEntry = { ...entry }
  if (typeof next.serverUrl === 'string' && next.url === undefined) {
    next.url = next.serverUrl
    delete next.serverUrl
  }
  if (next.type === 'local') next.type = 'stdio'
  if (next.type === 'streamableHttp') next.type = 'http'
  const text = (value: Json): Json =>
    typeof value === 'string'
      ? value
          .replace(/\$\{env:([A-Za-z_][A-Za-z0-9_]*)(:-[^}]*)?\}/g, '${$1$2}')
          .replace(/\$\{input:([^}]+)\}/g, (_, id: string) => `\${secret:${secretName(id)}}`)
      : value
  for (const key of ['command', 'url', 'cwd', 'bearer_token']) {
    if (typeof next[key] === 'string') next[key] = text(next[key] as Json)
  }
  if (Array.isArray(next.args)) next.args = next.args.map(text)
  for (const key of ['env', 'headers']) {
    const values = next[key]
    if (isObject(values)) {
      next[key] = Object.fromEntries(
        Object.entries(values).map(([name, value]) => [name, text(value as Json)])
      )
    }
  }
  return next
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
 * Moves the plaintext credentials of an entry into secrets: each value
 * becomes `${secret:<ID>_<KEY>}` and the returned `secrets` hold the values
 * to store. Credentials are header values, the bearer token, and env values
 * whose names say they are keys or tokens (or URLs with a password); other
 * env values, such as paths, stay readable. A value that refers to a
 * variable or secret stays too: its secret would hold the reference, which
 * is never expanded. `taken` names are not reused.
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
  const convert = (key: string, value: Json, credential: boolean): Json => {
    if (typeof value !== 'string' || !credential || !isPlaintext(value)) return value
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
        Object.entries(values).map(([key, value]) => [
          key,
          convert(
            key,
            value as Json,
            field === 'headers' || isCredential(key, typeof value === 'string' ? value : '')
          )
        ])
      )
    }
  }
  if (typeof entry.bearer_token === 'string') {
    next.bearer_token = convert('TOKEN', entry.bearer_token, true)
  }
  return { entry: next, secrets }
}

/** Whether an environment variable holds a credential, as the daemon's import decides it. */
export function isCredential(name: string, value: string): boolean {
  const words = name.toLowerCase().split(/[_.-]/)
  if (words.some((word) => CREDENTIAL_WORDS.has(word))) return true
  try {
    return Boolean(new URL(value).password)
  } catch {
    return false
  }
}

const CREDENTIAL_WORDS = new Set([
  'token',
  'tokens',
  'secret',
  'secrets',
  'password',
  'passwd',
  'pwd',
  'pass',
  'credential',
  'credentials',
  'key',
  'keys',
  'apikey',
  'auth',
  'authorization',
  'bearer',
  'cookie',
  'pat',
  'jwt'
])

/** A non-empty value that refers to no variable or secret. */
function isPlaintext(value: string): boolean {
  return Boolean(value.trim()) && !/\$(\{|[A-Za-z_])/.test(value)
}

/** A secret name built from free text: letters, digits and `_`, upper case. */
export function secretName(text: string): string {
  const name = text
    .toUpperCase()
    .replace(/[^A-Z0-9_]+/g, '_')
    .replace(/^_+|_+$/g, '')
  return /^[A-Z_]/.test(name) ? name : `_${name}`
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

/**
 * One way to install a Registry server: one of its remote endpoints, or one
 * of its packages run on this computer. `unsupported` says why a choice
 * cannot be installed here.
 */
export interface RegistryChoice {
  kind: 'remote' | 'package'
  index: number
  /** The registry type, or `remote`. */
  type: string
  /** The URL or package name. */
  target: string
  unsupported?: 'sse' | 'transport' | 'package'
  fields: RegistryField[]
}

/** A value the install form asks for. */
export interface RegistryField {
  key: string
  /** The header, variable, environment variable or argument it fills. */
  name: string
  description?: string
  required: boolean
  secret: boolean
  default?: string
  choices?: string[]
}

type Slot = { input: McpRegistryInput; name: string; key: string }

/** The ways a Registry server can be installed, remotes first: they run nothing here. */
export function registryChoices(server: McpRegistryServer): RegistryChoice[] {
  const remotes = (server.remotes || []).map((remote, index): RegistryChoice => {
    const unsupported =
      remote.type === 'sse' ? 'sse' : remote.type === 'streamable-http' ? undefined : 'transport'
    const fields = [
      ...templateFields(remote.url, remote.variables, 'url', false, true),
      ...(remote.headers || []).flatMap((header) =>
        inputFields({ input: header, name: header.name || '', key: `header:${header.name}` })
      )
    ]
    return { kind: 'remote', index, type: 'remote', target: remote.url, unsupported, fields }
  })
  const packages = (server.packages || []).map((pkg, index): RegistryChoice => {
    const unsupported = !PACKAGE_RUNNERS[pkg.registryType]
      ? 'package'
      : (pkg.transport?.type || 'stdio') !== 'stdio'
        ? 'transport'
        : undefined
    const fields = packageSlots(pkg).flatMap(inputFields)
    return {
      kind: 'package',
      index,
      type: pkg.registryType,
      target: pkg.identifier,
      unsupported,
      fields
    }
  })
  return [...remotes, ...packages]
}

/** The choice installed unless the owner picks another: the first remote, else the first package. */
export function defaultRegistryChoice(choices: RegistryChoice[]): RegistryChoice | undefined {
  return choices.find((choice) => !choice.unsupported)
}

/** A short id for a Registry server: the meaningful part of its name. */
export function registryServerId(name: string): string {
  const [namespace = '', path = ''] = name.split('/', 2)
  const part = (text: string) =>
    text
      .toLowerCase()
      .replace(/^mcp[-_](?:server[-_])?|[-_]mcp(?:[-_]server)?$|[-_]server$/g, '')
      .replace(/[^a-z0-9_-]+/g, '-')
      .replace(/^[-_]+|[-_]+$/g, '')
  const id = part(path)
  if (id && !['mcp', 'server'].includes(id)) return id
  const labels = namespace
    .split('.')
    .filter((label) => !['com', 'io', 'ai', 'org', 'net', 'dev', 'github'].includes(label))
  return part(labels[labels.length - 1] || '') || 'mcp'
}

/**
 * The mcp.json entry for a Registry server, and the secrets to store with
 * it: what the owner typed for a secret field goes to the secret store, and
 * the entry refers to it as `${secret:<ID>_<NAME>}`, avoiding the `taken`
 * names. A local server runs without the daemon's whole environment.
 * `missing` names required fields left empty, and then nothing is built.
 */
export function registryEntry(
  server: McpRegistryServer,
  choice: RegistryChoice,
  id: string,
  values: Record<string, string>,
  taken: ReadonlySet<string> = new Set()
): { entry?: McpEntry; secrets: Record<string, string>; missing: string[] } {
  const secrets: Record<string, string> = {}
  const missing: string[] = []
  const prefix = secretName(id)
  const fields = new Map(choice.fields.map((field) => [field.key, field]))
  /** The text a field fills in: its value, or a reference to its secret. */
  const fill = (key: string): string => {
    const field = fields.get(key)
    if (!field) return ''
    const value = (values[key] ?? '').trim() || field.default || ''
    if (!value) {
      if (field.required) missing.push(field.name)
      return ''
    }
    if (!field.secret) return value
    let name = secretName(`${prefix}_${field.name}`)
    for (let n = 2; taken.has(name) || (name in secrets && secrets[name] !== value); n += 1) {
      name = secretName(`${prefix}_${field.name}_${n}`)
    }
    secrets[name] = value
    return `\${secret:${name}}`
  }
  /** An input's text: its fixed value with its placeholders filled, or the field for it. */
  const text = (slot: Slot): string => {
    const value = slot.input.value
    if (value === undefined) return fill(slot.key)
    return value.replace(PLACEHOLDER, (_, variable: string) => fill(`${slot.key}:${variable}`))
  }

  let entry: McpEntry
  if (choice.kind === 'remote') {
    const remote = (server.remotes || [])[choice.index]
    const url = remote.url.replace(PLACEHOLDER, (_, variable: string) => fill(`url:${variable}`))
    entry = { id, type: 'http', url }
    const headers: Record<string, string> = {}
    for (const header of remote.headers || []) {
      if (!header.name) continue
      const value = text({ input: header, name: header.name, key: `header:${header.name}` })
      if (value) headers[header.name] = value
    }
    if (Object.keys(headers).length) entry.headers = headers
  } else {
    const pkg = (server.packages || [])[choice.index]
    const runner = PACKAGE_RUNNERS[pkg.registryType]
    const slots = packageSlots(pkg)
    const env: Record<string, string> = {}
    const argsOf = (group: 'runtime' | 'package') =>
      slots
        .filter((slot) => slot.key.startsWith(`arg:${group}:`))
        .flatMap((slot) => {
          const value = text(slot)
          if (!value) return []
          if (slot.input.type !== 'named' || !slot.input.name) return [value]
          const flag = slot.input.name.startsWith('-') ? slot.input.name : `--${slot.input.name}`
          if (slot.input.format === 'boolean') return value === 'true' ? [flag] : []
          return [flag, value]
        })
    for (const slot of slots.filter((slot) => slot.key.startsWith('env:'))) {
      const value = text(slot)
      if (value) env[slot.name] = value
    }
    const { command, args } = runner(pkg, argsOf('runtime'), argsOf('package'), Object.keys(env))
    entry = { id, command, args, inherit_env: false }
    if (Object.keys(env).length) entry.env = env
  }
  return missing.length ? { secrets: {}, missing } : { entry, secrets, missing }
}

const PLACEHOLDER = /\{([A-Za-z0-9_.-]+)\}/g

/** The inputs of a package, keyed as its form fields are. */
function packageSlots(pkg: McpRegistryPackage): Slot[] {
  const args = (group: 'runtime' | 'package', inputs: McpRegistryInput[] = []) =>
    inputs.map((input, index) => ({
      input,
      name: input.name || input.valueHint || `${group} argument ${index + 1}`,
      key: `arg:${group}:${index}`
    }))
  return [
    ...args('runtime', pkg.runtimeArguments),
    ...args('package', pkg.packageArguments),
    ...(pkg.environmentVariables || []).map((input) => ({
      input,
      name: input.name || '',
      key: `env:${input.name}`
    }))
  ]
}

/** The fields an input needs: one for itself, or one per placeholder in its fixed value. */
function inputFields(slot: Slot): RegistryField[] {
  const { input } = slot
  if (input.value !== undefined) {
    return templateFields(
      input.value,
      input.variables,
      slot.key,
      Boolean(input.isSecret),
      Boolean(input.isRequired)
    )
  }
  return [
    {
      key: slot.key,
      name: slot.name,
      description: input.description,
      required: Boolean(input.isRequired),
      secret: Boolean(input.isSecret),
      default: input.default,
      choices: input.choices
    }
  ]
}

function templateFields(
  template: string,
  variables: Record<string, McpRegistryInput> | undefined,
  key: string,
  secret: boolean,
  required: boolean
): RegistryField[] {
  const names = [...new Set([...template.matchAll(PLACEHOLDER)].map((match) => match[1]))]
  return names.map((name) => {
    const variable = variables?.[name]
    return {
      key: `${key}:${name}`,
      name,
      description: variable?.description,
      required: variable ? Boolean(variable.isRequired) || !variable.default : required,
      secret: variable ? Boolean(variable.isSecret) : secret,
      default: variable?.default,
      choices: variable?.choices
    }
  })
}

type Runner = (
  pkg: McpRegistryPackage,
  runtimeArgs: string[],
  packageArgs: string[],
  env: string[]
) => { command: string; args: string[] }

/** How each kind of package runs. MCPB bundles and the rest are installed by hand. */
const PACKAGE_RUNNERS: Record<string, Runner> = {
  npm: (pkg, runtimeArgs, packageArgs) => {
    const command = pkg.runtimeHint || 'npx'
    const spec = pkg.version ? `${pkg.identifier}@${pkg.version}` : pkg.identifier
    return {
      command,
      args: [...(command === 'npx' ? ['-y'] : []), ...runtimeArgs, spec, ...packageArgs]
    }
  },
  pypi: (pkg, runtimeArgs, packageArgs) => {
    const command = pkg.runtimeHint || 'uvx'
    const spec =
      pkg.version && pkg.version !== 'latest' ? `${pkg.identifier}==${pkg.version}` : pkg.identifier
    return { command, args: [...runtimeArgs, spec, ...packageArgs] }
  },
  oci: (pkg, runtimeArgs, packageArgs, env) => {
    const tagged = /:[^/]+$/.test(pkg.identifier) || !pkg.version
    const image = tagged ? pkg.identifier : `${pkg.identifier}:${pkg.version}`
    // The container sees only the variables passed on with -e.
    const passed = env.flatMap((name) => ['-e', name])
    return {
      command: 'docker',
      args: ['run', '-i', '--rm', ...passed, ...runtimeArgs, image, ...packageArgs]
    }
  },
  nuget: (pkg, runtimeArgs, packageArgs) => {
    const command = pkg.runtimeHint || 'dnx'
    const spec = pkg.version ? `${pkg.identifier}@${pkg.version}` : pkg.identifier
    return {
      command,
      args: [...runtimeArgs, spec, '--yes', ...(packageArgs.length ? ['--', ...packageArgs] : [])]
    }
  }
}

/** A subscription argument the automation form asks for. */
export interface EventArgumentField {
  name: string
  type: 'string' | 'number' | 'integer' | 'boolean'
  required: boolean
  description?: string
  /** The values an `enum` allows. */
  options?: string[]
}

const FIELD_TYPES = ['string', 'number', 'integer', 'boolean'] as const

/**
 * The fields of an event's argument schema, for a form: each a flat property
 * of a string, number, integer or boolean type, or a string enum. `null` when
 * the schema is anything else, and the arguments are written as JSON instead.
 */
export function eventArgumentFields(schema: Json): EventArgumentField[] | null {
  if (!schema || typeof schema !== 'object' || Array.isArray(schema)) return null
  const object = schema as Record<string, Json>
  if (object.type !== undefined && object.type !== 'object') return null
  const properties = object.properties ?? {}
  if (!properties || typeof properties !== 'object' || Array.isArray(properties)) return null
  const required = new Set(Array.isArray(object.required) ? object.required.map(String) : [])
  const fields: EventArgumentField[] = []
  for (const [name, value] of Object.entries(properties as Record<string, Json>)) {
    if (!value || typeof value !== 'object' || Array.isArray(value)) return null
    const property = value as Record<string, Json>
    const type = property.type
    const options = Array.isArray(property.enum) ? property.enum : undefined
    if (options && !options.every((option) => typeof option === 'string')) return null
    if (options && type !== undefined && type !== 'string') return null
    if (!options && !FIELD_TYPES.includes(type as (typeof FIELD_TYPES)[number])) return null
    fields.push({
      name,
      type: options ? 'string' : (type as EventArgumentField['type']),
      required: required.has(name),
      ...(typeof property.description === 'string' ? { description: property.description } : {}),
      ...(options ? { options: options as string[] } : {})
    })
  }
  return fields
}

/**
 * Arguments from the form's text values. Empty fields are left out; numbers
 * and booleans are parsed. Throws naming the field that is missing or wrong.
 */
export function eventArguments(
  fields: EventArgumentField[],
  values: Record<string, string>
): Record<string, Json> {
  const result: Record<string, Json> = {}
  for (const field of fields) {
    const text = (values[field.name] ?? '').trim()
    if (!text) {
      if (field.required) throw new Error(field.name)
      continue
    }
    if (field.type === 'boolean') {
      if (text !== 'true' && text !== 'false') throw new Error(field.name)
      result[field.name] = text === 'true'
    } else if (field.type === 'number' || field.type === 'integer') {
      const number = Number(text)
      if (!Number.isFinite(number) || (field.type === 'integer' && !Number.isInteger(number))) {
        throw new Error(field.name)
      }
      result[field.name] = number
    } else {
      result[field.name] = text
    }
  }
  return result
}

/**
 * The tools of a server that an event automation cannot use. Automations run
 * with nobody to answer an approval, so a tool that would ask is refused: it
 * runs only when set to Always allow, or when the server's `auto` lets a
 * reviewed read-only tool run unasked (the call gate's rule for a session that
 * asks on risk). A tool whose definition changed asks again either way.
 */
export function automationBlockedTools(
  server: Pick<McpServerView, 'approval'>,
  tools: McpToolView[]
): McpToolView[] {
  return tools.filter((tool) => {
    if (tool.hidden) return false
    const approval = tool.approval ?? server.approval
    const reviewed = (tool.review ?? 'trusted') === 'trusted'
    const readOnly = tool.annotations.read_only === true && tool.annotations.destructive !== true
    if (approval === 'ask') return true
    if (approval === 'allow') return !reviewed
    return !(reviewed && readOnly)
  })
}
