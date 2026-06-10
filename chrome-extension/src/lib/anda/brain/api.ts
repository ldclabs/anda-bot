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

export type KipCommandItem =
  | string
  | {
      command: string
      parameters?: Record<string, Json>
    }

export interface KipRequest {
  command?: KipCommandItem
  commands?: KipCommandItem[]
  parameters?: Record<string, Json>
  dry_run?: boolean
}

export interface KipError {
  code: string
  message: string
  hint?: string
  data?: unknown
}

export interface KipResponse<T> {
  result?: T
  error?: KipError
  next_cursor?: string
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

    const response = await this.request<BrainStatus | BrainResult<BrainStatus>>('/formation_status', {
      method: 'GET'
    })
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
    if (response.error) {
      throw new Error(formatKipError(response.error))
    }
    return response
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

function formatKipError(error: KipError): string {
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
