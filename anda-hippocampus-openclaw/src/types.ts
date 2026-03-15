/**
 * TypeScript types for the Anda Hippocampus API.
 * Reference: anda_hippocampus/API.md
 */

export interface RpcError {
  message: string
  data?: unknown
}

export interface RpcResponse<T> {
  result?: T
  error?: RpcError
}

export interface InputContext {
  user?: string | undefined
  agent?: string | undefined
  session?: string | undefined
  topic?: string | undefined
}

export type MessageRole = 'system' | 'user' | 'assistant' | 'tool'

export type MessageContentPart =
  | string
  | {
      type: string
      text?: string
      [k: string]: unknown
    }

export interface Message {
  role: MessageRole
  content: string | MessageContentPart[]
  name?: string | undefined
  user?: string | undefined
  timestamp?: number | undefined
}

export interface FormationInput {
  messages: Message[]
  context?: InputContext | undefined
  timestamp: string
}

export interface RecallInput {
  query: string
  context?: InputContext | undefined
}

export interface Usage {
  input_tokens?: number
  output_tokens?: number
  total_tokens?: number
}

export interface AgentOutput {
  content: string
  conversation?: number
  failed_reason?: string
  usage?: Usage
  model?: string
  [k: string]: unknown
}

/**
 * Plugin configuration options.
 */
export interface HippocampusPluginConfig {
  /** Anda Hippocampus base URL. Can point to the hosted service or a self-hosted/local deployment. Default: "https://brain.anda.ai" */
  baseUrl?: string
  /** Memory space ID (required) */
  spaceId: string
  /** Space token for API authentication (required) */
  spaceToken: string
  /** Default context to include with every request */
  defaultContext?: InputContext
  /** Request timeout for formation in ms. Default: 30000 */
  formationTimeoutMs?: number
  /** Request timeout for recall in ms. Default: 120000 */
  recallTimeoutMs?: number
}
