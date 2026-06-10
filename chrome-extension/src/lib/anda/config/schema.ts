import type { Json } from './api'

export type JsonObject = { [key: string]: Json }

export type FieldKind =
  | 'text'
  | 'secret'
  | 'number'
  | 'boolean'
  | 'select'
  | 'string-list'
  | 'object'

export interface FieldSchema {
  key: string
  label: string
  kind: FieldKind
  options?: string[]
  placeholder?: string
  nullable?: boolean
  fields?: FieldSchema[]
}

export const runtimeFields: FieldSchema[] = [
  { key: 'addr', label: 'Gateway address', kind: 'text', placeholder: '127.0.0.1:8042' },
  {
    key: 'log_level',
    label: 'Log level',
    kind: 'select',
    options: ['error', 'warn', 'info', 'debug']
  },
  { key: 'https_proxy', label: 'HTTPS proxy', kind: 'text', nullable: true },
  { key: 'workspaces', label: 'Extra workspaces', kind: 'string-list' }
]

export const userFields: FieldSchema[] = [
  { key: 'id', label: 'User id', kind: 'text', nullable: true },
  { key: 'pubkey', label: 'Ed25519 public key', kind: 'secret' }
]

export const modelProviderFields: FieldSchema[] = [
  { key: 'family', label: 'Family', kind: 'select', options: ['anthropic', 'openai', 'gemini'] },
  { key: 'model', label: 'Model', kind: 'text' },
  { key: 'api_base', label: 'API base', kind: 'text' },
  { key: 'api_key', label: 'API key', kind: 'secret' },
  { key: 'effort', label: 'Effort', kind: 'select', options: ['minimal', 'low', 'medium', 'high'] },
  { key: 'context_window', label: 'Context window', kind: 'number' },
  { key: 'max_output', label: 'Max output', kind: 'number' },
  { key: 'labels', label: 'Labels', kind: 'string-list' },
  { key: 'stream', label: 'Stream', kind: 'boolean' },
  { key: 'disabled', label: 'Disabled', kind: 'boolean' },
  { key: 'bearer_auth', label: 'Bearer auth', kind: 'boolean' }
]

export const ttsFields: FieldSchema[] = [
  { key: 'enabled', label: 'Enabled', kind: 'boolean' },
  {
    key: 'default_provider',
    label: 'Default provider',
    kind: 'select',
    options: ['edge', 'openai', 'google', 'stepfun']
  },
  {
    key: 'default_format',
    label: 'Default format',
    kind: 'select',
    options: ['mp3', 'opus', 'wav']
  },
  { key: 'max_text_length', label: 'Max text length', kind: 'number' }
]

export const ttsProviderSchemas: Record<string, FieldSchema[]> = {
  edge: [
    { key: 'binary_path', label: 'Binary path', kind: 'text' },
    { key: 'voice', label: 'Voice', kind: 'text' }
  ],
  openai: [
    { key: 'api_key', label: 'API key', kind: 'secret' },
    { key: 'model', label: 'Model', kind: 'text' },
    { key: 'speed', label: 'Speed', kind: 'number' },
    { key: 'voice', label: 'Voice', kind: 'text' }
  ],
  google: [
    { key: 'api_key', label: 'API key', kind: 'secret' },
    { key: 'language_code', label: 'Language code', kind: 'text' },
    { key: 'voice', label: 'Voice', kind: 'text' }
  ],
  stepfun: [
    { key: 'api_key', label: 'API key', kind: 'secret' },
    { key: 'api_url', label: 'API URL', kind: 'text' },
    { key: 'model', label: 'Model', kind: 'text' },
    { key: 'voice', label: 'Voice', kind: 'text' },
    { key: 'speed', label: 'Speed', kind: 'number' },
    { key: 'volume', label: 'Volume', kind: 'number' },
    { key: 'instruction', label: 'Instruction', kind: 'text', nullable: true },
    { key: 'sample_rate', label: 'Sample rate', kind: 'number' },
    { key: 'markdown_filter', label: 'Markdown filter', kind: 'boolean', nullable: true },
    {
      key: 'pronunciation_map',
      label: 'Pronunciation map',
      kind: 'object',
      fields: [{ key: 'tone', label: 'Tone replacements', kind: 'string-list' }]
    }
  ]
}

export const transcriptionFields: FieldSchema[] = [
  { key: 'enabled', label: 'Enabled', kind: 'boolean' },
  {
    key: 'default_provider',
    label: 'Default provider',
    kind: 'select',
    options: ['groq', 'openai', 'google', 'stepfun', 'local_whisper']
  },
  { key: 'initial_prompt', label: 'Initial prompt', kind: 'text', nullable: true },
  { key: 'max_duration_secs', label: 'Max duration seconds', kind: 'number' },
  { key: 'transcribe_non_ptt_audio', label: 'Transcribe non-PTT audio', kind: 'boolean' }
]

export const transcriptionProviderSchemas: Record<string, FieldSchema[]> = {
  groq: [
    { key: 'api_key', label: 'API key', kind: 'secret' },
    { key: 'api_url', label: 'API URL', kind: 'text' },
    { key: 'model', label: 'Model', kind: 'text' },
    { key: 'language', label: 'Language', kind: 'text', nullable: true },
    { key: 'language_code', label: 'Language code', kind: 'text' }
  ],
  openai: [
    { key: 'api_key', label: 'API key', kind: 'secret' },
    { key: 'model', label: 'Model', kind: 'text' }
  ],
  google: [
    { key: 'api_key', label: 'API key', kind: 'secret' },
    { key: 'language_code', label: 'Language code', kind: 'text' }
  ],
  stepfun: [
    { key: 'api_key', label: 'API key', kind: 'secret' },
    { key: 'api_url', label: 'API URL', kind: 'text' },
    { key: 'model', label: 'Model', kind: 'text' },
    { key: 'language', label: 'Language', kind: 'text' },
    { key: 'hotwords', label: 'Hotwords', kind: 'string-list' },
    { key: 'prompt', label: 'Prompt', kind: 'text', nullable: true },
    { key: 'enable_itn', label: 'Enable ITN', kind: 'boolean' },
    { key: 'pcm_codec', label: 'PCM codec', kind: 'text' },
    { key: 'pcm_rate', label: 'PCM rate', kind: 'number' },
    { key: 'pcm_bits', label: 'PCM bits', kind: 'number' },
    { key: 'pcm_channel', label: 'PCM channels', kind: 'number' }
  ],
  local_whisper: [
    { key: 'url', label: 'URL', kind: 'text' },
    { key: 'bearer_token', label: 'Bearer token', kind: 'secret', nullable: true },
    { key: 'max_audio_bytes', label: 'Max audio bytes', kind: 'number' },
    { key: 'timeout_secs', label: 'Timeout seconds', kind: 'number' }
  ]
}

export const channelSchemas: Record<string, FieldSchema[]> = {
  telegram: [
    { key: 'id', label: 'ID', kind: 'text', nullable: true },
    { key: 'user', label: 'User binding', kind: 'text', nullable: true },
    { key: 'bot_token', label: 'Bot token', kind: 'secret' },
    { key: 'username', label: 'Username', kind: 'text', nullable: true },
    { key: 'allowed_users', label: 'Allowed users', kind: 'string-list' },
    { key: 'allow_external_users', label: 'Allow external users', kind: 'boolean' },
    { key: 'mention_only', label: 'Mention only', kind: 'boolean' },
    { key: 'ack_reactions', label: 'ACK reactions', kind: 'boolean' }
  ],
  wechat: [
    { key: 'id', label: 'ID', kind: 'text', nullable: true },
    { key: 'user', label: 'User binding', kind: 'text', nullable: true },
    { key: 'bot_token', label: 'Bot token', kind: 'secret' },
    { key: 'username', label: 'Username', kind: 'text', nullable: true },
    { key: 'allowed_users', label: 'Allowed users', kind: 'string-list' },
    { key: 'allow_external_users', label: 'Allow external users', kind: 'boolean' },
    { key: 'route_tag', label: 'Route tag', kind: 'number', nullable: true }
  ],
  discord: [
    { key: 'id', label: 'ID', kind: 'text', nullable: true },
    { key: 'user', label: 'User binding', kind: 'text', nullable: true },
    { key: 'bot_token', label: 'Bot token', kind: 'secret' },
    { key: 'username', label: 'Username', kind: 'text', nullable: true },
    { key: 'guild_id', label: 'Guild ID', kind: 'text', nullable: true },
    { key: 'allowed_users', label: 'Allowed users', kind: 'string-list' },
    { key: 'allow_external_users', label: 'Allow external users', kind: 'boolean' },
    { key: 'listen_to_bots', label: 'Listen to bots', kind: 'boolean' },
    { key: 'mention_only', label: 'Mention only', kind: 'boolean' },
    { key: 'ack_reactions', label: 'ACK reactions', kind: 'boolean' }
  ],
  lark: [
    { key: 'id', label: 'ID', kind: 'text', nullable: true },
    { key: 'user', label: 'User binding', kind: 'text', nullable: true },
    { key: 'app_id', label: 'App ID', kind: 'text' },
    { key: 'app_secret', label: 'App secret', kind: 'secret' },
    { key: 'username', label: 'Username', kind: 'text', nullable: true },
    { key: 'verification_token', label: 'Verification token', kind: 'secret', nullable: true },
    { key: 'port', label: 'Webhook port', kind: 'number', nullable: true },
    { key: 'allowed_users', label: 'Allowed users', kind: 'string-list' },
    { key: 'allow_external_users', label: 'Allow external users', kind: 'boolean' },
    { key: 'mention_only', label: 'Mention only', kind: 'boolean' },
    { key: 'platform', label: 'Platform', kind: 'select', options: ['lark', 'feishu'] },
    {
      key: 'receive_mode',
      label: 'Receive mode',
      kind: 'select',
      options: ['websocket', 'webhook']
    },
    { key: 'ack_reactions', label: 'ACK reactions', kind: 'boolean' }
  ]
}

export function normalizeConfigDraft(value: Json): JsonObject {
  const draft = cloneJson(asObject(value))
  ensureObject(draft, 'model')
  ensureObject(draft, 'tts')
  ensureObject(draft, 'transcription')
  ensureObject(draft, 'channels')
  ensureArray(draft, 'users')
  ensureArray(draft, 'workspaces')
  ensureArray(getObject(draft, 'model'), 'providers')

  const channels = getObject(draft, 'channels')
  for (const channel of Object.keys(channelSchemas)) {
    ensureArray(channels, channel)
  }

  return draft
}

export function asObject(value: Json | undefined): JsonObject {
  if (value && typeof value === 'object' && !Array.isArray(value)) {
    return value as JsonObject
  }
  return {}
}

export function getObject(target: JsonObject, key: string): JsonObject {
  const value = target[key]
  if (value && typeof value === 'object' && !Array.isArray(value)) {
    return value as JsonObject
  }
  target[key] = {}
  return target[key] as JsonObject
}

export function optionalObject(target: JsonObject, key: string): JsonObject | null {
  const value = target[key]
  return value && typeof value === 'object' && !Array.isArray(value) ? (value as JsonObject) : null
}

export function ensureObject(target: JsonObject, key: string): JsonObject {
  return getObject(target, key)
}

export function ensureArray(target: JsonObject, key: string): Json[] {
  if (!Array.isArray(target[key])) {
    target[key] = []
  }
  return target[key] as Json[]
}

export function objectArray(target: JsonObject, key: string): JsonObject[] {
  return ensureArray(target, key).filter(
    (item): item is JsonObject => Boolean(item) && typeof item === 'object' && !Array.isArray(item)
  )
}

export function stringValue(target: JsonObject, key: string): string {
  const value = target[key]
  return value == null ? '' : String(value)
}

export function numberValue(target: JsonObject, key: string): string {
  const value = target[key]
  return typeof value === 'number' && Number.isFinite(value) ? String(value) : ''
}

export function booleanValue(target: JsonObject, key: string): boolean {
  return target[key] === true
}

export function stringListValue(target: JsonObject, key: string): string {
  const value = target[key]
  return Array.isArray(value) ? value.map((item) => String(item ?? '')).join('\n') : ''
}

export function setStringValue(
  target: JsonObject,
  key: string,
  value: string,
  nullable = false
): void {
  target[key] = nullable && !value.trim() ? null : value
}

export function setNumberValue(
  target: JsonObject,
  key: string,
  value: string,
  nullable = false
): void {
  const trimmed = value.trim()
  if (!trimmed) {
    target[key] = nullable ? null : 0
    return
  }
  const number = Number(trimmed)
  target[key] = Number.isFinite(number) ? number : target[key]
}

export function setStringListValue(target: JsonObject, key: string, value: string): void {
  target[key] = value
    .split(/\r?\n/)
    .map((item) => item.trim())
    .filter(Boolean)
}

export function createModelProvider(): JsonObject {
  return {
    family: 'openai',
    model: '',
    api_base: '',
    api_key: '',
    effort: 'high',
    context_window: 128000,
    max_output: 32000,
    labels: [],
    stream: true,
    disabled: false,
    bearer_auth: false
  }
}

export function createUser(): JsonObject {
  return { id: '', pubkey: '' }
}

export function createTtsProvider(provider: string): JsonObject {
  switch (provider) {
    case 'edge':
      return { binary_path: 'edge-tts', voice: 'en-US-AriaNeural' }
    case 'openai':
      return { api_key: '', model: 'tts-1', speed: 1.0, voice: 'alloy' }
    case 'google':
      return { api_key: '', language_code: 'en-US', voice: 'en-US-Standard-A' }
    case 'stepfun':
      return {
        api_key: '',
        api_url: 'https://api.stepfun.com/v1/audio/speech',
        model: 'stepaudio-2.5-tts',
        voice: 'ruyananshi',
        speed: 1.0,
        volume: 1.0,
        instruction: null,
        sample_rate: 24000,
        markdown_filter: null,
        pronunciation_map: { tone: [] }
      }
    default:
      return {}
  }
}

export function createTranscriptionProvider(provider: string): JsonObject {
  switch (provider) {
    case 'groq':
      return {
        api_key: '',
        api_url: 'https://api.groq.com/openai/v1/audio/transcriptions',
        model: 'whisper-large-v3-turbo',
        language: null,
        language_code: 'en-US'
      }
    case 'openai':
      return { api_key: '', model: 'whisper-1' }
    case 'google':
      return { api_key: '', language_code: 'en-US' }
    case 'stepfun':
      return {
        api_key: '',
        api_url: 'https://api.stepfun.com/v1/audio/asr/sse',
        model: 'stepaudio-2.5-asr',
        language: 'zh',
        hotwords: [],
        prompt: null,
        enable_itn: true,
        pcm_codec: 'pcm_s16le',
        pcm_rate: 16000,
        pcm_bits: 16,
        pcm_channel: 1
      }
    case 'local_whisper':
      return {
        url: 'http://127.0.0.1:8001/v1/transcribe',
        bearer_token: null,
        max_audio_bytes: 26214400,
        timeout_secs: 300
      }
    default:
      return {}
  }
}

export function createChannel(channel: string): JsonObject {
  switch (channel) {
    case 'telegram':
      return {
        id: 'personal',
        user: null,
        bot_token: '',
        username: 'anda_bot',
        allowed_users: ['*'],
        allow_external_users: false,
        mention_only: false,
        ack_reactions: true
      }
    case 'wechat':
      return {
        id: 'personal',
        user: null,
        bot_token: '',
        username: 'anda-wechat',
        allowed_users: ['*'],
        allow_external_users: false,
        route_tag: null
      }
    case 'discord':
      return {
        id: 'server',
        user: null,
        bot_token: '',
        username: 'anda-discord',
        guild_id: null,
        allowed_users: ['*'],
        allow_external_users: false,
        listen_to_bots: false,
        mention_only: true,
        ack_reactions: true
      }
    case 'lark':
      return {
        id: 'work',
        user: null,
        app_id: '',
        app_secret: '',
        username: 'anda-lark',
        verification_token: null,
        port: null,
        allowed_users: ['*'],
        allow_external_users: false,
        mention_only: true,
        platform: 'lark',
        receive_mode: 'websocket',
        ack_reactions: true
      }
    default:
      return {}
  }
}

export function removeArrayItem(target: JsonObject, key: string, index: number): void {
  const items = ensureArray(target, key)
  items.splice(index, 1)
}

export function renderConfigYaml(config: JsonObject, previousSource = ''): string {
  const comments = analyzeComments(previousSource)
  const emittedComments = new Set<number>()
  const lines: string[] = []

  writeCommentEntries(lines, comments.header, emittedComments)
  writeTopScalar(lines, comments, emittedComments, 'addr', config.addr ?? '127.0.0.1:8042')
  writeTopScalar(lines, comments, emittedComments, 'log_level', config.log_level ?? 'warn')
  writeTopScalar(lines, comments, emittedComments, 'https_proxy', config.https_proxy ?? null, true)
  writeTopList(lines, comments, emittedComments, 'workspaces', config.workspaces)
  writeTopObjectArray(lines, comments, emittedComments, 'users', config.users, userFields)

  writeTopSection(lines, comments, emittedComments, 'model')
  const model = asObject(config.model)
  lines.push('model:')
  writeScalarField(
    lines,
    2,
    { key: 'active', label: 'Active model', kind: 'text' },
    model.active ?? ''
  )
  writeObjectArray(lines, 2, 'providers', model.providers, modelProviderFields)
  lines.push('')

  writeTopSection(lines, comments, emittedComments, 'tts')
  const tts = asObject(config.tts)
  lines.push('tts:')
  writeFields(lines, 2, tts, ttsFields)
  for (const provider of Object.keys(ttsProviderSchemas)) {
    const value = optionalObject(tts, provider)
    if (value) {
      writeObject(lines, 2, provider, value, ttsProviderSchemas[provider])
    }
  }
  lines.push('')

  writeTopSection(lines, comments, emittedComments, 'transcription')
  const transcription = asObject(config.transcription)
  lines.push('transcription:')
  writeFields(lines, 2, transcription, transcriptionFields)
  for (const provider of Object.keys(transcriptionProviderSchemas)) {
    const value = optionalObject(transcription, provider)
    if (value) {
      writeObject(lines, 2, provider, value, transcriptionProviderSchemas[provider])
    }
  }
  lines.push('')

  writeTopSection(lines, comments, emittedComments, 'channels')
  const channels = asObject(config.channels)
  lines.push('channels:')
  for (const channel of Object.keys(channelSchemas)) {
    writeObjectArray(lines, 2, channel, channels[channel], channelSchemas[channel])
  }
  lines.push('')

  writeRemainingComments(lines, comments.all, emittedComments)

  return `${lines
    .join('\n')
    .replace(/\n{3,}/g, '\n\n')
    .trimEnd()}\n`
}

function writeTopScalar(
  lines: string[],
  comments: CommentIndex,
  emittedComments: Set<number>,
  key: string,
  value: Json,
  nullable = false
): void {
  writeTopSection(lines, comments, emittedComments, key)
  lines.push(`${key}: ${yamlScalar(value, nullable)}`.trimEnd())
  lines.push('')
}

function writeTopList(
  lines: string[],
  comments: CommentIndex,
  emittedComments: Set<number>,
  key: string,
  value: Json | undefined
): void {
  writeTopSection(lines, comments, emittedComments, key)
  writeStringList(lines, 0, key, Array.isArray(value) ? value : [])
  lines.push('')
}

function writeTopObjectArray(
  lines: string[],
  comments: CommentIndex,
  emittedComments: Set<number>,
  key: string,
  value: Json | undefined,
  fields: FieldSchema[]
): void {
  writeTopSection(lines, comments, emittedComments, key)
  writeObjectArray(lines, 0, key, value, fields)
  lines.push('')
}

function writeTopSection(
  lines: string[],
  comments: CommentIndex,
  emittedComments: Set<number>,
  key: string
): void {
  writeCommentEntries(
    lines,
    comments.leadingByKey.get(key) || comments.sectionByKey.get(key),
    emittedComments
  )
}

function writeFields(
  lines: string[],
  indent: number,
  target: JsonObject,
  fields: FieldSchema[]
): void {
  for (const field of fields) {
    writeScalarField(lines, indent, field, target[field.key])
  }
}

function writeObject(
  lines: string[],
  indent: number,
  key: string,
  value: JsonObject,
  fields: FieldSchema[]
): void {
  lines.push(`${spaces(indent)}${key}:`)
  writeFields(lines, indent + 2, value, fields)
}

function writeObjectArray(
  lines: string[],
  indent: number,
  key: string,
  value: Json | undefined,
  fields: FieldSchema[]
): void {
  const items = Array.isArray(value)
    ? value.filter(
        (item): item is JsonObject =>
          Boolean(item) && typeof item === 'object' && !Array.isArray(item)
      )
    : []
  if (items.length === 0) {
    lines.push(`${spaces(indent)}${key}: []`)
    return
  }

  lines.push(`${spaces(indent)}${key}:`)
  for (const item of items) {
    lines.push(
      `${spaces(indent + 2)}- ${fields[0].key}: ${yamlScalar(item[fields[0].key], fields[0].nullable)}`
    )
    for (const field of fields.slice(1)) {
      writeScalarField(lines, indent + 4, field, item[field.key])
    }
  }
}

function writeScalarField(
  lines: string[],
  indent: number,
  field: FieldSchema,
  value: Json | undefined
): void {
  if (field.kind === 'string-list') {
    writeStringList(lines, indent, field.key, Array.isArray(value) ? value : [])
    return
  }

  if (field.kind === 'object') {
    const objectValue = asObject(value)
    writeObject(lines, indent, field.key, objectValue, field.fields || [])
    return
  }

  lines.push(`${spaces(indent)}${field.key}: ${yamlScalar(value, field.nullable)}`.trimEnd())
}

function writeStringList(lines: string[], indent: number, key: string, value: Json[]): void {
  const items = value.map((item) => String(item ?? '').trim()).filter(Boolean)
  if (items.length === 0) {
    lines.push(`${spaces(indent)}${key}: []`)
    return
  }

  lines.push(`${spaces(indent)}${key}:`)
  for (const item of items) {
    lines.push(`${spaces(indent + 2)}- ${yamlScalar(item)}`)
  }
}

function yamlScalar(value: Json | undefined, nullable = false): string {
  if (value == null) {
    return nullable ? '' : 'null'
  }
  if (typeof value === 'boolean' || typeof value === 'number') {
    return String(value)
  }
  const text = String(value)
  if (text === '') {
    return nullable ? '' : '""'
  }
  if (
    /^[A-Za-z0-9_./:@+-]+$/.test(text) &&
    !['true', 'false', 'null', '~'].includes(text.toLowerCase())
  ) {
    return text
  }
  return JSON.stringify(text)
}

function spaces(count: number): string {
  return ' '.repeat(count)
}

interface CommentEntry {
  index: number
  line: string
}

interface CommentIndex {
  all: CommentEntry[]
  header: CommentEntry[]
  leadingByKey: Map<string, CommentEntry[]>
  sectionByKey: Map<string, CommentEntry[]>
}

function analyzeComments(source: string): CommentIndex {
  const lines = source.split(/\r?\n/)
  const all: CommentEntry[] = []
  const header: CommentEntry[] = []
  const leadingByKey = new Map<string, CommentEntry[]>()
  const sectionByKey = new Map<string, CommentEntry[]>()
  const topKeyByLine = new Map<number, string>()
  const topKeyPattern = /^([A-Za-z_][A-Za-z0-9_]*):(?:\s|$)/

  let currentKey = '__header'
  for (let index = 0; index < lines.length; index += 1) {
    const line = lines[index]
    const match = topKeyPattern.exec(line)
    if (match && !line.trimStart().startsWith('#')) {
      currentKey = match[1]
      topKeyByLine.set(index, currentKey)
      continue
    }
    if (!line.trimStart().startsWith('#')) {
      continue
    }

    const entry = { index, line }
    all.push(entry)
    if (currentKey === '__header') {
      header.push(entry)
    } else {
      const entries = sectionByKey.get(currentKey) || []
      entries.push(entry)
      sectionByKey.set(currentKey, entries)
    }
  }

  for (const [index, key] of topKeyByLine) {
    const entries: CommentEntry[] = []
    for (let scan = index - 1; scan >= 0; scan -= 1) {
      const line = lines[scan]
      if (line.trim() === '') {
        continue
      }
      if (!line.trimStart().startsWith('#')) {
        break
      }
      entries.unshift({ index: scan, line })
    }
    if (entries.length > 0) {
      leadingByKey.set(key, entries)
    }
  }

  return { all, header, leadingByKey, sectionByKey }
}

function writeCommentEntries(
  lines: string[],
  entries: CommentEntry[] | undefined,
  emittedComments: Set<number>
): void {
  if (!entries?.length) {
    return
  }
  for (const entry of entries) {
    if (emittedComments.has(entry.index)) {
      continue
    }
    lines.push(entry.line)
    emittedComments.add(entry.index)
  }
}

function writeRemainingComments(
  lines: string[],
  entries: CommentEntry[],
  emittedComments: Set<number>
): void {
  const remaining = entries.filter((entry) => !emittedComments.has(entry.index))
  if (remaining.length === 0) {
    return
  }
  lines.push('')
  for (const entry of remaining) {
    lines.push(entry.line)
    emittedComments.add(entry.index)
  }
}

function cloneJson<T extends Json>(value: T): T {
  return JSON.parse(JSON.stringify(value)) as T
}
