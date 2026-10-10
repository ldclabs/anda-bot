// Types shared with the service worker are declared once, in
// `$lib/service-worker/types`, and re-exported here. `ExtensionMessage` and
// `ExtensionResponse` in particular are the two ends of the same
// `chrome.runtime.sendMessage` wire, so a second declaration would let the
// panel and the worker drift apart silently.
import type {
  AppearanceTheme,
  ApprovalMode,
  ChromeApi,
  ChromeRuntimeMessageListener,
  ChromeTabInfo,
  ExtensionMessage,
  ExtensionResponse,
  PageAudioResult,
  PageSpeechResult,
  QuickPrompt,
  SettingsState,
  SubmitKeyMode
} from '$lib/service-worker/types'

export type {
  AppearanceTheme,
  ApprovalMode,
  ChromeApi,
  ChromeRuntimeMessageListener,
  ChromeTabInfo,
  ExtensionMessage,
  ExtensionResponse,
  PageAudioResult,
  PageSpeechResult,
  QuickPrompt,
  SettingsState,
  SubmitKeyMode
}

export type Principal = string
export type Xid = string

export type Json = string | number | boolean | null | { [key: string]: Json } | Json[]

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

export type ChatActionStatus = 'pending' | 'approved' | 'denied' | 'selected' | 'expired' | string

export interface ChatActionChoice {
  id: string
  label: string
  value?: string | null
  description?: string | null
  input?: ChatActionChoiceInput | null
  /** A link the client opens when this option is chosen (an MCP server's page). */
  url?: string | null
}

export interface ChatActionChoiceInput {
  placeholder?: string | null
  required?: boolean
  multiline?: boolean
}

export interface ChatActionTool {
  name: string
  label?: string | null
}

export type ChatActionDetailFormat = 'text' | 'code' | 'list' | 'json' | string

export interface ChatActionDetail {
  label: string
  value: Json
  format?: ChatActionDetailFormat | null
}

export interface ChatActionApproval {
  approveLabel?: string | null
  denyLabel?: string | null
  /** A third answer that approves and stops asking, when the card offers one. */
  rememberLabel?: string | null
}

export interface ChatAction {
  id: string
  name: string
  kind?: string
  status: ChatActionStatus
  tool?: ChatActionTool
  title?: string
  message?: string | null
  summary?: string
  details?: ChatActionDetail[]
  approval?: ChatActionApproval
  command?: string
  workspace?: string
  background?: boolean
  choices?: ChatActionChoice[]
  response?: Json
  createdAt?: number
  expiresAt?: number
  respondedAt?: number
  payload: Record<string, Json>
}

export interface ActionApiOutput {
  action_id: string
  conversation?: number
  status: string
  response: Json
  responded_at?: number
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

export type SkillSourceKind = 'personal' | 'bundled' | 'shared'
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
  /** What the last scan could not read here; the skills it reached are still listed. */
  diagnostics?: SkillDiagnostic[]
}

export interface SkillUsageSummary {
  callable: string
  requests: number
  input_tokens: number
  output_tokens: number
  cached_tokens: number
  total_tokens: number
}

/**
 * `inline` skills are read into the running agent's own context; only
 * `subagent` ones are exposed as isolated `SA_<agent_name>` callables.
 */
export type SkillExecution = 'inline' | 'subagent'

export interface ManagedSkill {
  id: string
  source: SkillSourceKind
  source_label: string
  priority: number
  name: string
  agent_name: string
  description: string
  compatibility?: string | null
  execution: SkillExecution
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
  'submitted' | 'working' | 'idle' | 'completed' | 'cancelled' | 'failed'

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
  /** Tool calls this message made, or tool results it carries (role `tool`). */
  tools?: ChatToolCall[]
  attachments?: ChatAttachment[]
  actions?: ChatAction[]
  timestamp?: number
  pending?: boolean
}

/**
 * One tool invocation. A call carries `args`; its result arrives later in a
 * `tool` message and is paired back by `callId` for display.
 */
export interface ChatToolCall {
  callId?: string
  name: string
  args?: Json
  /** Absent until the tool has returned. */
  output?: Json
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
  | {
      type: 'Action'
      name: string
      payload: Record<string, Json>
      recipients?: Principal[]
      signature?: string
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

/** The context the conversation's latest model request filled. */
export interface ContextUsage {
  /** Input and output tokens of that request. */
  tokens: number
  /** The model's context window; 0 when it is not configured. */
  window: number
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
  context_usage?: ContextUsage
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
  context_usage?: ContextUsage
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

export interface RequestMeta {
  engine?: Principal
  thread?: Xid
  user?: string
  [key: string]: Json | undefined
}

/** A server's state as one word; see the daemon's `McpStatus`. */
export type McpStatus =
  | 'disabled'
  | 'invalid'
  | 'connecting'
  | 'ready'
  | 'needs_auth'
  | 'failed'
  | 'disconnected'
  | 'unknown'

/** When the agent asks before calling a tool: `auto` follows the session's approval mode. */
export type McpApproval = 'auto' | 'ask' | 'allow'

/** How a tool's definition compares with the one that was reviewed. */
export type McpReview = 'trusted' | 'new' | 'changed'

export interface McpUsage {
  calls?: number
  errors?: number
  last_used_at?: number
}

/** One configured server. Every secret in `settings` is `{ redacted: true, secrets? }`. */
export interface McpServerView {
  id: string
  /** The server's own name and description: untrusted. */
  title?: string
  description?: string
  transport: 'stdio' | 'http' | 'unknown'
  /** The command line or URL, redacted. */
  summary: string
  enabled: boolean
  /** In mcp.json; otherwise added for the running daemon only. */
  persisted: boolean
  startup: 'background' | 'eager'
  source: 'file' | 'manual' | 'model' | 'import' | 'registry'
  /** The file an imported server came from, or the Registry entry of an installed one. */
  source_ref?: string
  status: McpStatus
  /** The policy of tools that have none of their own. */
  approval: McpApproval
  allow_external_users: boolean
  auth: 'none' | 'bearer' | 'headers' | 'oauth'
  last_error?: { at: number; message: string }
  last_ready_at?: number
  next_retry_at?: number
  diagnostics?: string[]
  tools: { total: number; hidden: number; needs_review: number }
  instructions_changed?: boolean
  usage?: McpUsage
  settings: Record<string, Json>
  /** The advanced settings, for an entry that parses. */
  options?: McpServerOptions
  /** Its event automations, and its event types once they were listed. */
  events?: McpServerEvents
}

export interface McpServerEvents {
  automations: number
  paused: number
  /** Known once the server's events were listed: whether it reports any. */
  supported?: boolean
  types?: number
}

/** A server's advanced settings; each one left out takes its default. */
export interface McpServerOptions {
  startup?: 'background' | 'eager'
  lifecycle?: 'auto' | 'discover' | 'initialize'
  concurrency?: 'serial' | 'read_only_parallel' | 'parallel'
  timeouts?: {
    setup_secs?: number
    list_secs?: number
    request_secs?: number
    call_secs?: number
    elicitation_secs?: number
  }
  limits?: { output_text_bytes?: number }
  /** Local servers only: the daemon's whole environment, or only the essentials and `env`. */
  inherit_env?: boolean
  tasks?: { max_wait_secs?: number }
  /** The agent and the apps may list and read its resources; on unless false. */
  resources?: boolean
  /** It may ask the user for input while a call runs; on unless false. */
  elicitation?: boolean
}

export interface McpToolView {
  /** The name the model calls it by; hidden tools have none. */
  name?: string
  remote_name: string
  title?: string
  description?: string
  /** Hints from the server: untrusted, and they grant nothing. */
  annotations: {
    read_only?: boolean
    destructive?: boolean
    idempotent?: boolean
    open_world?: boolean
  }
  hidden: boolean
  approval?: McpApproval
  review?: McpReview
}

export interface McpServerDetail extends Omit<McpServerView, 'tools'> {
  instructions?: string
  /** The reviewed instructions, while they differ from `instructions`. */
  reviewed_instructions?: string
  tools: McpToolView[]
}

export interface McpSnapshot {
  config_path: string
  revision: string
  config_changed_on_disk: boolean
  running: boolean
  diagnostics?: string[]
  servers: McpServerView[]
}

export interface McpReceipt {
  revision: string
  added?: string[]
  imported?: string[]
  removed?: string[]
  rebuilt?: string[]
  connected?: string[]
  reviewed?: string[]
  secrets_removed?: string[]
  failed?: { id: string; message: string }[]
  warnings?: string[]
}

export interface McpTestReport {
  status: McpStatus
  error?: string
  instructions?: string
  tools: McpToolView[]
}

export interface McpToolDiff {
  server_id: string
  tool: string
  review: McpReview
  reviewed_at?: number
  changes: { field: string; before: Json; after: Json }[]
}

/** A stored secret, by name only: values never leave the daemon. */
export interface McpSecretView {
  name: string
  is_set: boolean
  updated_at?: number
  used_by: string[]
}

/** An mcp.json entry with its `id`. */
export type McpEntry = { id: string } & Record<string, Json>

export type McpChange =
  | {
      op: 'add'
      server: McpEntry
      persist?: boolean
      /** `registry` with the Registry name and version it was installed from. */
      source?: 'manual' | 'registry'
      source_ref?: string
    }
  | { op: 'update'; server: McpEntry }
  | { op: 'remove'; id: string; keep_credentials?: boolean }
  | { op: 'set_enabled'; id: string; enabled: boolean }
  | { op: 'set_tool_visible'; id: string; tool: string; visible: boolean }
  | { op: 'set_approval'; id: string; tool?: string | null; approval: McpApproval | null }
  | { op: 'set_external_users'; id: string; allowed: boolean }
  | { op: 'mark_reviewed'; id: string; tools?: string[] }
  | { op: 'set_secret'; name: string; value: string | null }
  | { op: 'set_options'; id: string; options: McpServerOptions }

/** A client whose MCP configuration can be imported. */
export type McpImportSource =
  'claude_desktop' | 'claude_code' | 'cursor' | 'vscode' | 'windsurf' | 'codex'

/** What importing a server would do; see the daemon's `McpImportStatus`. */
export type McpImportStatus = 'new' | 'renamed' | 'exists' | 'duplicate' | 'invalid'

/** A server found in another client's configuration, redacted. */
export interface McpImportCandidate {
  key: string
  source: McpImportSource
  path: string
  project?: string
  /** Its name in that file. */
  name: string
  /** The id it is imported as. */
  id: string
  status: McpImportStatus
  existing_id?: string
  duplicate_of?: string
  transport: 'stdio' | 'http' | 'unknown'
  summary: string
  enabled: boolean
  settings: Record<string, Json>
  /** Fields whose plaintext values would move to secrets. */
  plaintext?: string[]
  /** Secrets it references that are not set yet. */
  needs_secrets?: { name: string; description: string }[]
  warnings?: string[]
  error?: string
}

export interface McpImportScan {
  files: {
    source: McpImportSource
    path: string
    project?: string
    servers: number
    error?: string
  }[]
  candidates: McpImportCandidate[]
}

export interface McpImportRequest {
  items: { key: string; id?: string }[]
  /** Values for the secrets the servers need. */
  secrets?: Record<string, string>
  /** Move plaintext tokens to secrets (the default). */
  store_secrets?: boolean
  workspaces?: string[]
  expected_revision?: string
}

/** An input of a Registry `server.json`: a header, an environment variable or an argument. */
export interface McpRegistryInput {
  name?: string
  type?: 'positional' | 'named'
  description?: string
  value?: string
  default?: string
  choices?: string[]
  format?: string
  valueHint?: string
  isRequired?: boolean
  isSecret?: boolean
  isRepeated?: boolean
  variables?: Record<string, McpRegistryInput>
}

export interface McpRegistryRemote {
  type: string
  url: string
  headers?: McpRegistryInput[]
  variables?: Record<string, McpRegistryInput>
}

export interface McpRegistryPackage {
  registryType: string
  registryBaseUrl?: string
  identifier: string
  version?: string
  runtimeHint?: string
  fileSha256?: string
  transport?: { type: string; url?: string }
  runtimeArguments?: McpRegistryInput[]
  packageArguments?: McpRegistryInput[]
  environmentVariables?: McpRegistryInput[]
}

/** A server as the MCP Registry publishes it (`server.json`). */
export interface McpRegistryServer {
  name: string
  title?: string
  description?: string
  version?: string
  websiteUrl?: string
  repository?: { url?: string; source?: string; subfolder?: string }
  remotes?: McpRegistryRemote[]
  packages?: McpRegistryPackage[]
}

export interface McpRegistryPage {
  servers: McpRegistryServer[]
  next_cursor?: string
}

export interface McpSignIn {
  status: 'connected' | 'authorization_required'
  server_id: string
  authorization_url?: string
}

/** How an MCP server delivers an event type. */
export type McpEventDeliveryMode = 'poll' | 'push' | 'webhook'

/** An event type a server can report (MCP Events). Server text is untrusted. */
export interface McpEventDefinition {
  name: string
  description?: string | null
  delivery: McpEventDeliveryMode[]
  /** Only a webhook delivers it: Anda receives it through dMsg. */
  webhook_only: boolean
  input_schema: Json
  payload_schema?: Json | null
}

export interface McpEventsView {
  /** False when the server does not implement MCP Events. */
  supported: boolean
  events: McpEventDefinition[]
  /** Why the events could not be listed. */
  error?: string
  /** The dMsg server that receives webhooks, or why there is none. */
  ingress: { available: true; server_id: string } | { available: false; reason: string }
  triggers: McpTrigger[]
}

export type McpTriggerDelivery = 'auto' | 'poll' | 'push' | 'webhook'

export type McpTriggerState =
  | 'starting'
  | 'active'
  | 'retrying'
  | 'paused'
  | 'waiting'
  | 'needs_auth'
  | 'needs_ingress'
  | 'ended'

/** An automation: an agent run on a server's events. */
export interface McpTrigger {
  id: number
  name: string
  server_id: string
  event: string
  arguments: Record<string, Json>
  instructions: string
  delivery: McpTriggerDelivery
  /** The delivery mode in use. */
  mode?: McpEventDeliveryMode | null
  batch_window_secs: number
  max_runs_per_hour: number
  enabled: boolean
  state: McpTriggerState
  last_error?: string | null
  last_event_at?: number | null
  last_run_at?: number | null
  /** When the server last said events were lost. */
  missed_events_at?: number | null
  events_received: number
  runs: number
  /** Events waiting for a run. */
  pending: number
  last_conversation_id?: number | null
  webhook?: {
    ingress: string
    endpoint_id: string
    subscribed: boolean
    refresh_before?: number | null
  } | null
  created_by: 'owner' | 'model'
  created_at: number
  updated_at: number
}

export interface McpTriggerRun {
  id: number
  started_at: number
  finished_at?: number | null
  events: number
  result?: string | null
  error?: string | null
  conversation_id?: number | null
}

export interface McpTriggerEvent {
  event_id: string
  name: string
  timestamp: string
  received_at: number
  handled: boolean
  verified?: 'v1' | 'v1a' | null
  /** The event's data as JSON text, cut short. Untrusted. */
  data: string
}

export interface McpTriggerDetail extends McpTrigger {
  runs_recent: McpTriggerRun[]
  events_recent: McpTriggerEvent[]
}

/** A server's resources, or why they could not be listed. Server text is untrusted. */
export interface McpResourceListing {
  server_id: string
  title?: string | null
  resources?: McpResource[]
  truncated?: boolean
  error?: string
}

export interface McpResource {
  uri: string
  name: string
  title?: string | null
  description?: string | null
  mime_type?: string | null
  size?: number | null
}

/** One content of a resource read, to attach to a message. */
export interface McpResourceAttachment {
  name: string
  uri: string
  mime_type?: string | null
  size: number
  text: boolean
  /** The bytes, base64. */
  blob: string
}

export interface McpTriggerInput {
  server_id: string
  event: string
  arguments?: Record<string, Json>
  instructions: string
  name?: string
  delivery?: McpTriggerDelivery
  batch_window_secs?: number
  max_runs_per_hour?: number
}

export type McpTriggerChange =
  | { op: 'create'; trigger: McpTriggerInput }
  | {
      op: 'update'
      id: number
      changes: Partial<
        Pick<
          McpTriggerInput,
          | 'name'
          | 'arguments'
          | 'instructions'
          | 'delivery'
          | 'batch_window_secs'
          | 'max_runs_per_hour'
        >
      >
    }
  | { op: 'set_enabled'; id: number; enabled: boolean }
  | { op: 'delete'; id: number }
