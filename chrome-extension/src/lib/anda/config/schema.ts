import { getMessage } from '$lib/i18n'
import { Document, isMap, isNode, isScalar, isSeq, parseDocument } from 'yaml'
import type { Pair, YAMLMap, YAMLSeq } from 'yaml'
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
  {
    key: 'addr',
    label: getMessage('configFieldGatewayAddress') || 'Gateway address',
    kind: 'text',
    placeholder: '127.0.0.1:8042'
  },
  {
    key: 'log_level',
    label: getMessage('configFieldLogLevel') || 'Log level',
    kind: 'select',
    options: ['error', 'warn', 'info', 'debug']
  },
  {
    key: 'https_proxy',
    label: getMessage('configFieldHTTPSProxy') || 'HTTPS proxy',
    kind: 'text',
    nullable: true
  },
  {
    key: 'workspaces',
    label: getMessage('configFieldExtraWorkspaces') || 'Extra workspaces',
    kind: 'string-list'
  }
]

export const userFields: FieldSchema[] = [
  { key: 'id', label: getMessage('configFieldUserId') || 'User id', kind: 'text', nullable: true },
  {
    key: 'pubkey',
    label: getMessage('configFieldEd25519PublicKey') || 'Ed25519 public key',
    kind: 'secret'
  }
]

export const modelProviderFields: FieldSchema[] = [
  {
    key: 'family',
    label: getMessage('configFieldFamily') || 'Family',
    kind: 'select',
    options: ['anthropic', 'openai', 'gemini']
  },
  { key: 'model', label: getMessage('configFieldModel') || 'Model', kind: 'text' },
  { key: 'api_base', label: getMessage('configFieldAPIBase') || 'API base', kind: 'text' },
  { key: 'api_key', label: getMessage('configFieldAPIKey') || 'API key', kind: 'secret' },
  {
    key: 'effort',
    label: getMessage('configFieldEffort') || 'Effort',
    kind: 'select',
    options: ['minimal', 'low', 'medium', 'high']
  },
  {
    key: 'context_window',
    label: getMessage('configFieldContextWindow') || 'Context window',
    kind: 'number'
  },
  { key: 'max_output', label: getMessage('configFieldMaxOutput') || 'Max output', kind: 'number' },
  { key: 'labels', label: getMessage('configFieldLabels') || 'Labels', kind: 'string-list' },
  { key: 'stream', label: getMessage('configFieldStream') || 'Stream', kind: 'boolean' },
  { key: 'disabled', label: getMessage('configFieldDisabled') || 'Disabled', kind: 'boolean' },
  {
    key: 'bearer_auth',
    label: getMessage('configFieldBearerAuth') || 'Bearer auth',
    kind: 'boolean'
  }
]

export const ttsFields: FieldSchema[] = [
  { key: 'enabled', label: getMessage('configFieldEnabled') || 'Enabled', kind: 'boolean' },
  {
    key: 'default_provider',
    label: getMessage('configFieldDefaultProvider') || 'Default provider',
    kind: 'select',
    options: ['edge', 'openai', 'google', 'stepfun']
  },
  {
    key: 'default_format',
    label: getMessage('configFieldStepFunOutputFormat') || 'StepFun output format',
    kind: 'select',
    options: ['mp3', 'opus', 'wav', 'flac', 'pcm']
  },
  {
    key: 'max_text_length',
    label: getMessage('configFieldMaxTextLength') || 'Max text length',
    kind: 'number'
  }
]

export const ttsProviderSchemas: Record<string, FieldSchema[]> = {
  edge: [
    {
      key: 'binary_path',
      label: getMessage('configFieldBinaryPath') || 'Binary path',
      kind: 'text'
    },
    { key: 'voice', label: getMessage('configFieldVoice') || 'Voice', kind: 'text' }
  ],
  openai: [
    { key: 'api_key', label: getMessage('configFieldAPIKey') || 'API key', kind: 'secret' },
    { key: 'model', label: getMessage('configFieldModel') || 'Model', kind: 'text' },
    { key: 'speed', label: getMessage('configFieldSpeed') || 'Speed', kind: 'number' },
    { key: 'voice', label: getMessage('configFieldVoice') || 'Voice', kind: 'text' }
  ],
  google: [
    { key: 'api_key', label: getMessage('configFieldAPIKey') || 'API key', kind: 'secret' },
    {
      key: 'language_code',
      label: getMessage('configFieldLanguageCode') || 'Language code',
      kind: 'text'
    },
    { key: 'voice', label: getMessage('configFieldVoice') || 'Voice', kind: 'text' }
  ],
  stepfun: [
    { key: 'api_key', label: getMessage('configFieldAPIKey') || 'API key', kind: 'secret' },
    { key: 'api_url', label: getMessage('configFieldAPIURL') || 'API URL', kind: 'text' },
    { key: 'model', label: getMessage('configFieldModel') || 'Model', kind: 'text' },
    { key: 'voice', label: getMessage('configFieldVoice') || 'Voice', kind: 'text' },
    { key: 'speed', label: getMessage('configFieldSpeed') || 'Speed', kind: 'number' },
    { key: 'volume', label: getMessage('configFieldVolume') || 'Volume', kind: 'number' },
    {
      key: 'instruction',
      label: getMessage('configFieldInstruction') || 'Instruction',
      kind: 'text',
      nullable: true
    },
    {
      key: 'sample_rate',
      label: getMessage('configFieldSampleRate') || 'Sample rate',
      kind: 'number'
    },
    {
      key: 'markdown_filter',
      label: getMessage('configFieldMarkdownFilter') || 'Markdown filter',
      kind: 'boolean',
      nullable: true
    },
    {
      key: 'pronunciation_map',
      label: getMessage('configFieldPronunciationMap') || 'Pronunciation map',
      kind: 'object',
      fields: [
        {
          key: 'tone',
          label: getMessage('configFieldToneReplacements') || 'Tone replacements',
          kind: 'string-list'
        }
      ]
    }
  ]
}

export const transcriptionFields: FieldSchema[] = [
  { key: 'enabled', label: getMessage('configFieldEnabled') || 'Enabled', kind: 'boolean' },
  {
    key: 'default_provider',
    label: getMessage('configFieldDefaultProvider') || 'Default provider',
    kind: 'select',
    options: ['groq', 'openai', 'google', 'stepfun', 'local_whisper']
  },
  {
    key: 'initial_prompt',
    label: getMessage('configFieldWhisperInitialPrompt') || 'Whisper initial prompt',
    kind: 'text',
    nullable: true
  }
]

export const transcriptionProviderSchemas: Record<string, FieldSchema[]> = {
  groq: [
    { key: 'api_key', label: getMessage('configFieldAPIKey') || 'API key', kind: 'secret' },
    { key: 'api_url', label: getMessage('configFieldAPIURL') || 'API URL', kind: 'text' },
    { key: 'model', label: getMessage('configFieldModel') || 'Model', kind: 'text' },
    {
      key: 'language',
      label: getMessage('configFieldLanguage') || 'Language',
      kind: 'text',
      nullable: true
    }
  ],
  openai: [
    { key: 'api_key', label: getMessage('configFieldAPIKey') || 'API key', kind: 'secret' },
    { key: 'model', label: getMessage('configFieldModel') || 'Model', kind: 'text' }
  ],
  google: [
    { key: 'api_key', label: getMessage('configFieldAPIKey') || 'API key', kind: 'secret' },
    {
      key: 'language_code',
      label: getMessage('configFieldLanguageCode') || 'Language code',
      kind: 'text'
    }
  ],
  stepfun: [
    { key: 'api_key', label: getMessage('configFieldAPIKey') || 'API key', kind: 'secret' },
    { key: 'api_url', label: getMessage('configFieldAPIURL') || 'API URL', kind: 'text' },
    { key: 'model', label: getMessage('configFieldModel') || 'Model', kind: 'text' },
    { key: 'language', label: getMessage('configFieldLanguage') || 'Language', kind: 'text' },
    {
      key: 'hotwords',
      label: getMessage('configFieldHotwords') || 'Hotwords',
      kind: 'string-list'
    },
    {
      key: 'prompt',
      label: getMessage('configFieldPrompt') || 'Prompt',
      kind: 'text',
      nullable: true
    },
    {
      key: 'enable_itn',
      label: getMessage('configFieldEnableITN') || 'Enable ITN',
      kind: 'boolean'
    },
    { key: 'pcm_codec', label: getMessage('configFieldPCMCodec') || 'PCM codec', kind: 'text' },
    { key: 'pcm_rate', label: getMessage('configFieldPCMRate') || 'PCM rate', kind: 'number' },
    { key: 'pcm_bits', label: getMessage('configFieldPCMBits') || 'PCM bits', kind: 'number' },
    {
      key: 'pcm_channel',
      label: getMessage('configFieldPCMChannels') || 'PCM channels',
      kind: 'number'
    }
  ],
  local_whisper: [
    { key: 'url', label: getMessage('configFieldURL') || 'URL', kind: 'text' },
    {
      key: 'bearer_token',
      label: getMessage('configFieldBearerToken') || 'Bearer token',
      kind: 'secret',
      nullable: true
    },
    {
      key: 'max_audio_bytes',
      label: getMessage('configFieldMaxAudioBytes') || 'Max audio bytes',
      kind: 'number'
    },
    {
      key: 'timeout_secs',
      label: getMessage('configFieldTimeoutSeconds') || 'Timeout seconds',
      kind: 'number'
    }
  ]
}

export const channelSchemas: Record<string, FieldSchema[]> = {
  telegram: [
    { key: 'id', label: getMessage('configFieldID') || 'ID', kind: 'text', nullable: true },
    {
      key: 'user',
      label: getMessage('configFieldUserBinding') || 'User binding',
      kind: 'text',
      nullable: true
    },
    { key: 'bot_token', label: getMessage('configFieldBotToken') || 'Bot token', kind: 'secret' },
    {
      key: 'username',
      label: getMessage('configFieldUsername') || 'Username',
      kind: 'text',
      nullable: true
    },
    {
      key: 'allowed_users',
      label: getMessage('configFieldAllowedUsers') || 'Allowed users',
      kind: 'string-list'
    },
    {
      key: 'allow_external_users',
      label: getMessage('configFieldAllowExternalUsers') || 'Allow external users',
      kind: 'boolean'
    },
    {
      key: 'mention_only',
      label: getMessage('configFieldMentionOnly') || 'Mention only',
      kind: 'boolean'
    },
    {
      key: 'ack_reactions',
      label: getMessage('configFieldACKReactions') || 'ACK reactions',
      kind: 'boolean'
    }
  ],
  wechat: [
    { key: 'id', label: getMessage('configFieldID') || 'ID', kind: 'text', nullable: true },
    {
      key: 'user',
      label: getMessage('configFieldUserBinding') || 'User binding',
      kind: 'text',
      nullable: true
    },
    { key: 'bot_token', label: getMessage('configFieldBotToken') || 'Bot token', kind: 'secret' },
    {
      key: 'username',
      label: getMessage('configFieldUsername') || 'Username',
      kind: 'text',
      nullable: true
    },
    {
      key: 'allowed_users',
      label: getMessage('configFieldAllowedUsers') || 'Allowed users',
      kind: 'string-list'
    },
    {
      key: 'allow_external_users',
      label: getMessage('configFieldAllowExternalUsers') || 'Allow external users',
      kind: 'boolean'
    },
    {
      key: 'route_tag',
      label: getMessage('configFieldRouteTag') || 'Route tag',
      kind: 'number',
      nullable: true
    }
  ],
  discord: [
    { key: 'id', label: getMessage('configFieldID') || 'ID', kind: 'text', nullable: true },
    {
      key: 'user',
      label: getMessage('configFieldUserBinding') || 'User binding',
      kind: 'text',
      nullable: true
    },
    { key: 'bot_token', label: getMessage('configFieldBotToken') || 'Bot token', kind: 'secret' },
    {
      key: 'username',
      label: getMessage('configFieldUsername') || 'Username',
      kind: 'text',
      nullable: true
    },
    {
      key: 'guild_id',
      label: getMessage('configFieldGuildID') || 'Guild ID',
      kind: 'text',
      nullable: true
    },
    {
      key: 'allowed_users',
      label: getMessage('configFieldAllowedUsers') || 'Allowed users',
      kind: 'string-list'
    },
    {
      key: 'allow_external_users',
      label: getMessage('configFieldAllowExternalUsers') || 'Allow external users',
      kind: 'boolean'
    },
    {
      key: 'listen_to_bots',
      label: getMessage('configFieldListenToBots') || 'Listen to bots',
      kind: 'boolean'
    },
    {
      key: 'mention_only',
      label: getMessage('configFieldMentionOnly') || 'Mention only',
      kind: 'boolean'
    },
    {
      key: 'ack_reactions',
      label: getMessage('configFieldACKReactions') || 'ACK reactions',
      kind: 'boolean'
    }
  ],
  lark: [
    { key: 'id', label: getMessage('configFieldID') || 'ID', kind: 'text', nullable: true },
    {
      key: 'user',
      label: getMessage('configFieldUserBinding') || 'User binding',
      kind: 'text',
      nullable: true
    },
    { key: 'app_id', label: getMessage('configFieldAppID') || 'App ID', kind: 'text' },
    {
      key: 'app_secret',
      label: getMessage('configFieldAppSecret') || 'App secret',
      kind: 'secret'
    },
    {
      key: 'username',
      label: getMessage('configFieldUsername') || 'Username',
      kind: 'text',
      nullable: true
    },
    {
      key: 'verification_token',
      label: getMessage('configFieldVerificationToken') || 'Verification token',
      kind: 'secret',
      nullable: true
    },
    {
      key: 'port',
      label: getMessage('configFieldWebhookPort') || 'Webhook port',
      kind: 'number',
      nullable: true
    },
    {
      key: 'allowed_users',
      label: getMessage('configFieldAllowedUsers') || 'Allowed users',
      kind: 'string-list'
    },
    {
      key: 'allow_external_users',
      label: getMessage('configFieldAllowExternalUsers') || 'Allow external users',
      kind: 'boolean'
    },
    {
      key: 'mention_only',
      label: getMessage('configFieldMentionOnly') || 'Mention only',
      kind: 'boolean'
    },
    {
      key: 'platform',
      label: getMessage('configFieldPlatform') || 'Platform',
      kind: 'select',
      options: ['lark', 'feishu']
    },
    {
      key: 'receive_mode',
      label: getMessage('configFieldReceiveMode') || 'Receive mode',
      kind: 'select',
      options: ['websocket', 'webhook']
    },
    {
      key: 'ack_reactions',
      label: getMessage('configFieldACKReactions') || 'ACK reactions',
      kind: 'boolean'
    }
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

// Re-render the YAML source with the draft values applied. The previous
// source is parsed into a comment-preserving document tree and the draft is
// synced onto it in place, so comments (full-line and inline), key order,
// quoting style, and keys the form schema does not know about all survive.
export function renderConfigYaml(config: JsonObject, previousSource = ''): string {
  let doc = parseDocument(previousSource)
  if (doc.errors.length > 0 || !isMap(doc.contents)) {
    doc = new Document({}) as typeof doc
  }
  syncMapNode(doc, doc.contents as YAMLMap, config)
  return doc.toString({ lineWidth: 0, flowCollectionPadding: false })
}

// Parse hand-edited YAML back into a draft object, or null when invalid.
export function parseConfigDraft(source: string): JsonObject | null {
  const doc = parseDocument(source)
  if (doc.errors.length > 0 || !isMap(doc.contents)) {
    return null
  }
  return normalizeConfigDraft(doc.toJS() as Json)
}

function syncMapNode(doc: Document, map: YAMLMap, json: JsonObject, keepUnknown = true): void {
  if (!keepUnknown) {
    map.items = map.items.filter((pair) => Object.hasOwn(json, pairKey(pair)))
  }
  const pairs = map.items as Pair[]
  for (const [key, value] of Object.entries(json)) {
    const pair = pairs.find((item) => pairKey(item) === key)
    if (!pair) {
      // Keys the file never had are only added when they carry content, so a
      // form round-trip does not pad hand-maintained files with empty keys.
      if (!isEmptyDraftValue(value)) {
        map.items.push(doc.createPair(key, value))
      }
      continue
    }
    if (value === null && (isMap(pair.value) || isSeq(pair.value))) {
      // The form removed this optional block (e.g. a disabled TTS provider).
      map.items.splice(map.items.indexOf(pair), 1)
      continue
    }
    pair.value = syncNode(doc, pair.value, value, keepUnknown)
  }
  // Keys absent from the draft (unknown to the form schema) are kept as-is.
}

function syncNode(doc: Document, node: unknown, value: Json, keepUnknown = true): unknown {
  if (Array.isArray(value)) {
    if (isSeq(node)) {
      syncSeqNode(doc, node, value)
      return node
    }
    if (isNullScalar(node) && isEmptyDraftValue(value)) {
      // Keep a bare `key:` (often holding commented-out examples) instead of
      // overwriting it with an empty placeholder.
      return node
    }
    return replaceNode(doc, node, value)
  }
  if (value !== null && typeof value === 'object') {
    if (isMap(node)) {
      syncMapNode(doc, node, value, keepUnknown)
      return node
    }
    if (isNullScalar(node) && isEmptyDraftValue(value)) {
      return node
    }
    return replaceNode(doc, node, value)
  }
  if (isScalar(node)) {
    if (typeof node.value !== typeof value) {
      // Drop the remembered scalar style so e.g. a quoted string does not
      // turn a number or boolean into a quoted scalar.
      node.type = undefined
    }
    node.value = value
    return node
  }
  return replaceNode(doc, node, value)
}

function syncSeqNode(doc: Document, seq: YAMLSeq, values: Json[]): void {
  const previous = [...seq.items]
  const used = new Set<number>()
  // Match unchanged items first so removing/reordering a row moves its own
  // comments with it. Edited rows may reuse their slot, but never its fields.
  const matches = values.map((value) => {
    const index = previous.findIndex(
      (node, index) =>
        !used.has(index) &&
        JSON.stringify(isNode(node) ? node.toJSON() : node) === JSON.stringify(value)
    )
    if (index >= 0) used.add(index)
    return index
  })
  seq.items = values.map((value, index) => {
    const match = matches[index]
    if (match >= 0) return previous[match]
    if (previous.length === values.length && !used.has(index)) {
      used.add(index)
      return syncNode(doc, previous[index], value, false)
    }
    return doc.createNode(value)
  })
}

function replaceNode(doc: Document, node: unknown, value: Json): unknown {
  const next = doc.createNode(value)
  if (isNode(node)) {
    if (node.commentBefore) {
      next.commentBefore = node.commentBefore
    }
    if (node.comment) {
      next.comment = node.comment
    }
    if (node.spaceBefore) {
      next.spaceBefore = node.spaceBefore
    }
  }
  return next
}

function pairKey(pair: Pair): string {
  return isScalar(pair.key) ? String(pair.key.value) : String(pair.key)
}

function isNullScalar(node: unknown): boolean {
  return node == null || (isScalar(node) && node.value == null)
}

// Empty in the sense that writing it would only add placeholders: nulls,
// empty strings, and containers holding nothing but such values. Numbers and
// booleans always count as content.
function isEmptyDraftValue(value: Json): boolean {
  if (value === null || value === '') {
    return true
  }
  if (Array.isArray(value)) {
    return value.length === 0
  }
  if (typeof value === 'object') {
    return Object.values(value).every(isEmptyDraftValue)
  }
  return false
}

function cloneJson<T extends Json>(value: T): T {
  return JSON.parse(JSON.stringify(value)) as T
}
