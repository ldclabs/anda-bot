import { daemonRpc } from '../client/platform'
import type { SettingsState } from '$lib/service-worker/types'

/** The Brain space the daemon serves to its clients. */
export const ANDA_BOT_SPACE_ID = 'anda_bot'

export interface AttentionItem {
  id: string
  wake_ref: string
  parent_id?: string
  summary: string
  state: string
  reason?: string
  decision_ref?: string
  attempt_ref?: string
  dispatch_ref?: string
  clarification?: unknown
  delivery?: unknown
}
export interface AttentionPage {
  scope: { space_id: string; space_instance: string }
  items: AttentionItem[]
  next_cursor?: string
  complete: boolean
}
export type AttentionResponse =
  | { kind: 'clarification'; event_key: string; answer: string }
  | { kind: 'agent_statement'; event_key: string; statement: string }
export interface ResponseReceipt {
  receipt_id: string
  status: string
  evidence_ref?: string
}
export interface RuntimeStatus {
  /** Verified engine caller on the built-in Anda Bot WebSocket proxy. */
  caller?: string
  configured: boolean
  attention_enabled: boolean
  actions_enabled: boolean
  observation_enabled: boolean
  observer_authenticated: boolean
  blocked_reasons: string[]
  visible_items: number
  inventory_complete: boolean
  learning: Record<string, unknown>
  utility: Record<string, unknown>
  trust: Record<string, unknown>
  semantic_attention: Record<string, unknown>
}

export async function brainPendingStorageKey(
  settings: SettingsState,
  caller?: string
): Promise<string> {
  // The verified caller is stable across bearer refreshes. Without one, keep
  // responses isolated by credential.
  const identity = caller ? ['caller', caller] : ['credential', settings.token]
  const bytes = await crypto.subtle.digest(
    'SHA-256',
    new TextEncoder().encode(JSON.stringify([settings.baseUrl, ANDA_BOT_SPACE_ID, identity]))
  )
  return `brain-responses:${Array.from(new Uint8Array(bytes), (byte) =>
    byte.toString(16).padStart(2, '0')
  ).join('')}`
}

/** Brain runtime calls go through the daemon RPC under the caller's identity. */
export class BrainApi {
  constructor(private readonly settings: SettingsState) {}

  async attention(cursor?: string): Promise<AttentionPage> {
    return daemonRpc<AttentionPage>(
      'brain_attention',
      [{ cursor: cursor ?? null, limit: 20 }],
      this.settings
    )
  }

  async runtimeStatus(): Promise<RuntimeStatus> {
    return daemonRpc<RuntimeStatus>('brain_runtime_status', [], this.settings)
  }

  async respond(id: string, response: AttentionResponse): Promise<ResponseReceipt> {
    if (!/^[a-f0-9]{64}$/.test(id)) throw new Error('Invalid attention item id')
    return daemonRpc<ResponseReceipt>('brain_respond', [id, response], this.settings)
  }
}
