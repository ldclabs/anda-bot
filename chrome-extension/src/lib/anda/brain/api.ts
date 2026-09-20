import { defaultSettings, normalizeSettings } from '$lib/service-worker/settings'
import type { SettingsState } from '$lib/service-worker/types'

export const ANDA_BOT_SPACE_ID = 'anda_bot'

type BrainChromeApi = {
  runtime?: {
    sendMessage<Result>(message: BrainRpcMessage): Promise<BrainRpcResponse<Result>>
  }
  storage?: {
    local?: {
      get(keys: string[]): Promise<BrainStorageState>
      set(items: BrainStorageState): Promise<void>
    }
  }
}

type BrainStorageState = Partial<SettingsState> & {
  brainSpaceId?: unknown
}

type BrainRpcMessage = {
  type: 'anda_rpc'
  settings: SettingsState
  method: string
  params: unknown[]
}

type BrainRpcResponse<Result> =
  | { ok: true; result?: Result; status?: string }
  | { ok: false; error: string; status?: string }

export type Json =
  | string
  | number
  | boolean
  | null
  | Json[]
  | {
      [key: string]: Json
    }

export interface KipOperation {
  command: string
  op_id?: string
  parameters?: Record<string, Json>
}

export interface KipRequest {
  operations: KipOperation[]
  execution?: { mode: 'independent' | 'sequence' | 'atomic' }
  parameters?: Record<string, Json>
  dry_run?: boolean
  read?: Record<string, Json>
}

export interface KipError {
  code: string
  message: string
  hint?: string
  details?: unknown
}

export interface KipOperationResult<T> {
  status: string
  result?: T
  error?: KipError
  next_cursor?: string
}

export interface KipResponse<T = unknown> {
  kip: string
  status: string
  results: KipOperationResult<T>[]
  error?: KipError
  next_cursor?: string
}

/** Partial data never conceals a failed or unexecuted operation. */
export function assertKipSucceeded(response: KipResponse, expectedOperations?: number): void {
  const error =
    response.error ||
    (Array.isArray(response.results)
      ? response.results.find((item) => item.error)?.error
      : undefined)
  if (error) throw new Error(formatKipError(error))
  if (
    response.kip !== '2.0' ||
    response.status !== 'succeeded' ||
    !Array.isArray(response.results) ||
    response.results.length === 0 ||
    (expectedOperations !== undefined && response.results.length !== expectedOperations) ||
    response.results.some((item) => item.status !== 'succeeded' || !('result' in item))
  ) {
    throw new Error(`KIP operation did not succeed: ${JSON.stringify(response)}`)
  }
}

export interface BrainStatus {
  id: string
  concepts: number
  propositions: number
  conversations: number
  formation_processing: boolean
  maintenance_processing: boolean
  formation_processed_id: number
  maintenance_processed_id: number
}

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
  settings: BrainGraphSettings,
  caller?: string
): Promise<string> {
  // The verified caller is stable across bearer refreshes. Custom direct
  // Brain endpoints do not expose it, so retain credential isolation there.
  const identity = caller ? ['caller', caller] : ['credential', settings.token]
  const bytes = await crypto.subtle.digest(
    'SHA-256',
    new TextEncoder().encode(JSON.stringify([settings.baseUrl, settings.spaceId, identity]))
  )
  return `brain-responses:${Array.from(new Uint8Array(bytes), (byte) =>
    byte.toString(16).padStart(2, '0')
  ).join('')}`
}

export interface BrainGraphSettings extends SettingsState {
  spaceId: string
}

export class BrainApi {
  readonly settings: BrainGraphSettings

  constructor(settings: BrainGraphSettings) {
    this.settings = {
      ...normalizeSettings(settings),
      spaceId: normalizeSpaceId(settings.spaceId)
    }
  }

  get spaceBaseUrl(): string {
    return `${this.settings.baseUrl}/v1/${encodeURIComponent(this.settings.spaceId)}`
  }

  async status(): Promise<BrainStatus> {
    const rpcResponse = await this.extensionRpc<BrainStatus>('brain_status', [])
    if (rpcResponse) {
      return rpcResponse
    }

    const response = await this.request<BrainStatus | BrainResult<BrainStatus>>(
      '/formation_status',
      {
        method: 'GET'
      }
    )
    return isBrainResult(response) ? unwrapBrainResult(response, 'Brain status') : response
  }

  async executeKipReadonly<T = unknown>(request: KipRequest): Promise<KipResponse<T>> {
    const rpcResponse = await this.extensionRpc<KipResponse<T>>('brain_kip_readonly', [request])
    const response =
      rpcResponse ??
      (await this.request<KipResponse<T>>('/execute_kip_readonly', {
        method: 'POST',
        body: JSON.stringify(request)
      }))
    assertKipSucceeded(response, request.operations.length)
    return response
  }

  async attention(cursor?: string): Promise<AttentionPage> {
    const query = { cursor: cursor ?? null, limit: 20 }
    const rpc = await this.extensionRpc<AttentionPage>('brain_attention', [query])
    if (rpc) return rpc
    const params = new URLSearchParams({ limit: '20' })
    if (cursor) params.set('cursor', cursor)
    return unwrapBrainResult(
      await this.request<BrainResult<AttentionPage>>(`/attention?${params}`, { method: 'GET' }),
      'Brain inbox'
    )
  }

  async runtimeStatus(): Promise<RuntimeStatus> {
    const rpc = await this.extensionRpc<RuntimeStatus>('brain_runtime_status', [])
    if (rpc) return rpc
    return unwrapBrainResult(
      await this.request<BrainResult<RuntimeStatus>>('/runtime/status', { method: 'GET' }),
      'Brain runtime'
    )
  }

  async respond(id: string, response: AttentionResponse): Promise<ResponseReceipt> {
    if (!/^[a-f0-9]{64}$/.test(id)) throw new Error('Invalid attention item id')
    const rpc = await this.extensionRpc<ResponseReceipt>('brain_respond', [id, response])
    if (rpc) return rpc
    return unwrapBrainResult(
      await this.request<BrainResult<ResponseReceipt>>(`/attention/${id}/responses`, {
        method: 'POST',
        body: JSON.stringify(response)
      }),
      'Brain response'
    )
  }

  private async extensionRpc<T>(method: string, params: unknown[]): Promise<T | null> {
    if (this.settings.spaceId !== ANDA_BOT_SPACE_ID) {
      return null
    }

    const chromeApi = getBrainChromeApi()
    if (!chromeApi?.runtime?.sendMessage) {
      return null
    }
    if (!this.settings.token) {
      throw new Error('missing bearer token')
    }

    const response = await chromeApi.runtime.sendMessage<T>({
      type: 'anda_rpc',
      settings: normalizeSettings(this.settings),
      method,
      params
    })
    if (!response?.ok) {
      throw new Error(response?.error || `Brain RPC ${method} failed`)
    }
    return response.result as T
  }

  private async request<T>(path: string, init: RequestInit): Promise<T> {
    const headers = new Headers(init.headers)
    headers.set('Accept', 'application/json')
    if (init.body) {
      headers.set('Content-Type', 'application/json')
    }
    if (this.settings.token) {
      headers.set('Authorization', `Bearer ${this.settings.token}`)
    }

    const response = await fetch(`${this.spaceBaseUrl}${path}`, {
      ...init,
      headers
    })
    const text = await response.text()

    if (!response.ok) {
      throw new Error(`Brain API ${response.status}: ${text || response.statusText}`)
    }

    if (!text.trim()) {
      return undefined as T
    }

    try {
      return JSON.parse(text) as T
    } catch (error) {
      throw new Error(`Brain API returned invalid JSON: ${String(error)}`)
    }
  }
}

export async function loadBrainGraphSettings(): Promise<BrainGraphSettings> {
  const chromeApi = getBrainChromeApi()
  if (chromeApi?.storage?.local) {
    const saved = await chromeApi.storage.local.get([
      'baseUrl',
      'token',
      'submitKeyMode',
      'appearanceTheme',
      'brainSpaceId'
    ])
    return {
      ...normalizeSettings({
        baseUrl: String(saved.baseUrl || defaultSettings.baseUrl),
        token: String(saved.token || ''),
        submitKeyMode: saved.submitKeyMode || defaultSettings.submitKeyMode,
        appearanceTheme: saved.appearanceTheme || defaultSettings.appearanceTheme
      }),
      spaceId: normalizeSpaceId(saved.brainSpaceId)
    }
  }

  const saved = safeReadLocalStorage()
  return {
    ...normalizeSettings({
      ...defaultSettings,
      ...saved
    }),
    spaceId: normalizeSpaceId(saved.brainSpaceId)
  }
}

export async function saveBrainGraphSettings(settings: BrainGraphSettings): Promise<void> {
  const normalized = {
    ...normalizeSettings(settings),
    brainSpaceId: normalizeSpaceId(settings.spaceId)
  }
  const chromeApi = getBrainChromeApi()
  if (chromeApi?.storage?.local) {
    await chromeApi.storage.local.set(normalized)
    return
  }
  localStorage.setItem('andaBrainGraphSettings', JSON.stringify(normalized))
}

interface BrainResult<T> {
  result?: T
  error?: KipError
}

export function normalizeSpaceId(value: unknown): string {
  const spaceId = String(value || '').trim()
  return spaceId || ANDA_BOT_SPACE_ID
}

function getBrainChromeApi(): BrainChromeApi | undefined {
  return (globalThis as typeof globalThis & { chrome?: BrainChromeApi }).chrome
}

export function formatKipError(error: KipError): string {
  const prefix = error.code ? `${error.code}: ` : ''
  const hint = error.hint ? ` ${error.hint}` : ''
  return `${prefix}${error.message}${hint}`
}

function isBrainResult<T>(value: T | BrainResult<T>): value is BrainResult<T> {
  if (!value || typeof value !== 'object') {
    return false
  }
  const record = value as Record<string, unknown>
  return 'result' in record || 'error' in record
}

function unwrapBrainResult<T>(response: BrainResult<T>, label: string): T {
  if (response.error) {
    throw new Error(formatKipError(response.error))
  }
  if (response.result === undefined) {
    throw new Error(`${label} returned no result`)
  }
  return response.result
}

function safeReadLocalStorage(): Record<string, unknown> {
  try {
    const raw = localStorage.getItem('andaBrainGraphSettings')
    return raw ? (JSON.parse(raw) as Record<string, unknown>) : {}
  } catch (_error) {
    return {}
  }
}
