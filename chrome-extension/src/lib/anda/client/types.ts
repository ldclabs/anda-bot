import type {
  AppearanceTheme,
  ChromeApi,
  QuickPrompt,
  SettingsState,
  SubmitKeyMode
} from '$lib/service-worker/types'

export type { AppearanceTheme, ChromeApi, QuickPrompt, SettingsState, SubmitKeyMode }

export type Principal = string
export type Xid = string

export type Json = string | number | boolean | null | { [key: string]: Json } | Json[]

export interface StorageState extends Partial<SettingsState> {
  browserSessionId?: string
  workspaceChannelSources?: string[]
  uiLanguage?: string
  quickPrompts?: QuickPrompt[]
}

export interface ChromeTabInfo {
  id?: number
  windowId?: number
  title?: string
  url?: string
  incognito?: boolean
}

export type MessageRole = 'user' | 'assistant' | 'system' | 'tool' | 'external_user'

export interface AttachmentSummary {
  id: string
  name: string
  type?: string
  size?: number
}

export interface Resource {
  _id?: number
  tags: string[]
  name: string
  description?: string
  uri?: string
  mime_type?: string
  blob?: string // base64-encoded content
  size?: number
  hash?: string // base64-encoded SHA3-256 hash of the content.
  metadata?: Record<string, unknown>
}

export interface ChatAttachment extends AttachmentSummary {
  resource: Resource
}

export interface VoiceCapabilities {
  transcription: string[]
  daemonTts: string[]
  chromeTts: boolean
}

export interface ModelState {
  activeModel: string | null
  modelNames: string[]
}

export interface DaemonModelState {
  active_model?: string | null
  model_names?: string[]
}

export interface PromptSkill {
  name: string
  description?: string
}

export type SkillSourceKind = 'personal' | 'bundled' | 'shared' | 'legacy'
export type SkillDiagnosticSeverity = 'info' | 'warning' | 'error'

export interface SkillDiagnostic {
  severity: SkillDiagnosticSeverity
  code: string
  message: string
}

export interface SkillSourceInfo {
  source: SkillSourceKind
  source_label: string
  priority: number
  path: string
  editable: boolean
  exists: boolean
}

export interface SkillUsageSummary {
  callable: string
  requests: number
  input_tokens: number
  output_tokens: number
  cached_tokens: number
  total_tokens: number
}

export interface ManagedSkill {
  id: string
  source: SkillSourceKind
  source_label: string
  priority: number
  name: string
  agent_name: string
  description: string
  compatibility?: string | null
  allowed_tools: string[]
  metadata: Record<string, unknown>
  path: string
  directory: string
  editable: boolean
  active: boolean
  disabled: boolean
  shadowed_by?: string | null
  diagnostics: SkillDiagnostic[]
  updated_at?: number | null
  size?: number | null
  file_count: number
  usage?: SkillUsageSummary | null
  version: string
}

export interface ManagedSkillDetail extends ManagedSkill {
  content: string
  files: SkillFileEntry[]
}

export type SkillFileKind = 'directory' | 'file'

export interface SkillFileEntry {
  path: string
  name: string
  kind: SkillFileKind
  size?: number | null
  updated_at?: number | null
}

export interface SkillFileContent {
  id: string
  path: string
  content: string
  size: number
  updated_at?: number | null
  truncated: boolean
}

export interface SkillValidationResult {
  valid: boolean
  diagnostics: SkillDiagnostic[]
  name?: string
  agent_name?: string
}

export type VoiceProvider = 'chrome' | 'anda'

export type ConversationStatus =
  | 'submitted'
  | 'working'
  | 'idle'
  | 'completed'
  | 'cancelled'
  | 'failed'

export interface VoiceRecordingInput {
  voiceProvider?: VoiceProvider
  transcript?: string
  audioBase64?: string
  fileName?: string
  mimeType?: string
  size?: number
  ttsEnabled: boolean
}

export const SubmitMessageConversationId = Number.MAX_SAFE_INTEGER

export interface ChatMessage {
  id: string
  conversation: number
  role: MessageRole
  text: string
  externalUser?: ExternalUserMessageInfo
  thinkingText?: string
  attachments?: ChatAttachment[]
  timestamp?: number
  pending?: boolean
}

export interface ExternalUserMessageInfo {
  channel?: string
  sender?: string
  space?: string
  scope?: string
}

export interface BookmarkMessageInfo {
  index: number
  role: MessageRole
  text: string
}

/**
 * A bookmarked conversation. Fields match the daemon `bookmarks_api` JSON shape
 * (snake_case) to avoid a mapping layer. Individual marked messages carry the
 * message index from `m-<conversation>-<index>`.
 */
export interface Bookmark {
  _id: number
  user: string
  conversation: number
  source: string
  folder_ids: number[]
  messages: BookmarkMessageInfo[]
  created_at: number
}

export interface BookmarkedMessage {
  bookmark: Bookmark
  message_id: string
  message_index: number
  conversation: number
  source: string
  role: MessageRole
  folder_ids: number[]
  text: string
  created_at: number
}

export interface BookmarkFolder {
  _id: number
  name: string
  parent_id: number | null
  order: number
  created_at: number
  updated_at: number
}

export interface BookmarkFolders {
  version: number
  next_folder_id: number
  folders: Record<string, BookmarkFolder>
  updated_at: number
}

export interface MessageGroup {
  _id: number
  status: ConversationStatus
  ancestors: number[]
  messages: ChatMessage[]
  createdAt: number
  updatedAt: number
  current: boolean
}

export interface ClientSnapshot {
  settings: SettingsState
  tab: ChromeTabInfo | null
  status: string
  voiceCapabilities: VoiceCapabilities
}

export interface ChromeEvent<Listener extends (...args: never[]) => void> {
  addListener(listener: Listener): void
  removeListener(listener: Listener): void
}

export interface ChromeTabChangeInfo {
  title?: string
  url?: string
}

export interface AgentInput {
  /// agent name, use default agent if empty.
  name: string
  /// agent prompt or message.
  prompt: string
  /// The resources to process by the agent.
  resources?: Resource[]
  /// Optional topics or tags associated with the agent execution.
  topics?: string[]
  /// The metadata for the agent request
  meta?: RequestMeta
}

export interface AgentOutput {
  /// The output content from the agent, may be empty.
  content: string
  /// The reasoning or thought process of the agent, if available.
  thoughts?: string
  /// The usage statistics for the agent execution.
  usage: Usage
  tools_usage?: Record<string, Usage>
  /// Indicates failure reason if present, None means successful execution.
  /// Should be None when finish_reason is "stop" or "tool_calls".
  failed_reason?: string
  /// Tool calls returned by the LLM function calling.
  tool_calls?: ToolCall[]

  chat_history?: Message[]

  /// A collection of artifacts generated during execution.
  artifacts?: Resource[]
  /// The conversation ID.
  conversation?: number
  /// The session ID for the agent execution, if applicable.
  /// This is used to correlate related conversations or executions.
  session?: string
  /// The model used by the agent.
  model?: string
}

export interface ToolInput<TArgs = Json> {
  /// tool name.
  name: string
  /// arguments in JSON format.
  args: TArgs
  /// The resources to process by the tool.
  resources?: Resource[]
  /// The metadata for the tool request.
  meta?: RequestMeta
}

/**
 * Represents the output of a tool execution.
 */
export interface ToolOutput<TOut = Json> {
  /// The output from the tool.
  output: TOut
  /// A collection of artifacts generated by the tool execution.
  artifacts?: Resource[]
  /// The usage statistics for the tool execution.
  usage: Usage
}

export interface ToolCall {
  id: string
  name: string
  /// tool function arguments (JSON serialized string).
  args: string
  /// The result of the tool call, if available.
  result?: Json
}

export type ContentPart =
  | {
      type: 'Text'
      text: string
    }
  | {
      type: 'Reasoning'
      text: string
    }
  | {
      type: 'FileData'
      fileUri: string
      mimeType?: string
    }
  | {
      type: 'InlineData'
      mimeType: string
      data: string
    }
  | {
      type: 'ToolCall'
      name: string
      args: Json
      callId?: string
    }
  | {
      type: 'ToolOutput'
      name: string
      output: Json
      callId?: string
    }
  | ({
      type: 'Resource'
    } & Resource)
  | ({
      type: 'Any' // no specific type, the content is determined by the fields of the object
    } & Record<string, Json>)

export interface Message {
  role: 'user' | 'assistant' | 'tool'
  content: ContentPart[]
  name?: string
  user?: Principal
  timestamp?: number
}

export interface Usage {
  input_tokens: number
  output_tokens: number
  cached_tokens: number
  requests: number
}

export interface Conversation {
  _id: number
  user: Principal
  thread?: Xid
  messages?: Message[]
  resources?: Resource[]
  artifacts?: Resource[]
  status: ConversationStatus
  usage: Usage
  failed_reason?: string
  steering_messages?: string[]
  follow_up_messages?: string[]
  child?: number
  ancestors?: number[]
  label?: string
  extra?: Record<string, unknown>
  created_at: number
  updated_at: number
}

export interface ConversationDelta {
  _id: number
  messages: Message[]
  artifacts: Resource[]
  status: ConversationStatus
  usage: Usage
  failed_reason?: string
  updated_at: number
  child?: number
}

export interface SourceState {
  c?: number
  conv_id?: number
  s?: ConversationStatus
  status?: ConversationStatus
  t?: number
  timestamp?: number
}

export type SourceStateMap = Record<string, SourceState>

export interface RpcOutput<Result> {
  result: Result
  next_cursor?: string | null
  error?: unknown
}

export interface DaemonVoiceCapabilities {
  transcription?: boolean | string[]
  tts?: boolean | string[]
}

export interface TranscriptionToolOutput {
  text: string
  provider: string
  file_name: string
}

export interface TtsToolOutput {
  provider: string
  artifact: string
  mime_type: string
  format: string
  size: number
}

export interface PageSpeechResult {
  available?: boolean
  started?: boolean
  transcript?: string
  canceled?: boolean
  error?: string
}

export interface PageAudioResult {
  available?: boolean
  started?: boolean
  audioBase64?: string
  mimeType?: string
  size?: number
  canceled?: boolean
  error?: string
}

export interface RequestMeta {
  engine?: Principal
  thread?: Xid
  user?: string
  [key: string]: Json | undefined
}

export interface ExtensionMessage {
  type: string
  settings: SettingsState
  method?: string
  params?: unknown[]
  text?: string
  language?: string
  mimeType?: string
}

export type ExtensionResponse<Result> =
  | { ok: true; result?: Result; status?: string }
  | { ok: false; error: string; status?: string }

export type SnapshotListener = (snapshot: ClientSnapshot) => void
