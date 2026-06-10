import { defaultSettings, errorToMessage, normalizeSettings } from '$lib/service-worker/settings'
import type { SettingsState } from '$lib/service-worker/types'

export type Json =
  | string
  | number
  | boolean
  | null
  | Json[]
  | {
      [key: string]: Json
    }

export interface DaemonConfigResponse {
  path: string
  content: string
  config: Json
}

type ConfigStorageState = Partial<SettingsState>

type ConfigChromeApi = {
  storage?: {
    local?: {
      get(keys: string[]): Promise<ConfigStorageState>
      set(items: ConfigStorageState): Promise<void>
    }
  }
}

export class DaemonConfigApi {
  readonly settings: SettingsState

  constructor(settings: SettingsState) {
    this.settings = normalizeSettings(settings)
  }

  async load(): Promise<DaemonConfigResponse> {
    return this.request<DaemonConfigResponse>('/daemon/config', { method: 'GET' })
  }

  async save(content: string): Promise<DaemonConfigResponse> {
    return this.request<DaemonConfigResponse>('/daemon/config', {
      method: 'PUT',
      body: JSON.stringify({ content })
    })
  }

  private async request<T>(path: string, init: RequestInit): Promise<T> {
    if (!this.settings.token) {
      throw new Error('missing bearer token')
    }

    const headers = new Headers(init.headers)
    headers.set('Accept', 'application/json')
    headers.set('Authorization', `Bearer ${this.settings.token}`)
    if (init.body) {
      headers.set('Content-Type', 'application/json')
    }

    const response = await fetch(`${this.settings.baseUrl}${path}`, {
      ...init,
      headers
    })
    const text = await response.text()
    if (!response.ok) {
      throw new Error(`Config API ${response.status}: ${text || response.statusText}`)
    }
    if (!text.trim()) {
      return undefined as T
    }
    try {
      return JSON.parse(text) as T
    } catch (error) {
      throw new Error(`Config API returned invalid JSON: ${errorToMessage(error)}`)
    }
  }
}

export async function loadConfigSettings(): Promise<SettingsState> {
  const chromeApi = getConfigChromeApi()
  if (chromeApi?.storage?.local) {
    const saved = await chromeApi.storage.local.get([
      'baseUrl',
      'token',
      'submitKeyMode',
      'appearanceTheme'
    ])
    return normalizeSettings({
      baseUrl: String(saved.baseUrl || defaultSettings.baseUrl),
      token: String(saved.token || ''),
      submitKeyMode: saved.submitKeyMode || defaultSettings.submitKeyMode,
      appearanceTheme: saved.appearanceTheme || defaultSettings.appearanceTheme
    })
  }

  return normalizeSettings({ ...defaultSettings, ...safeReadLocalStorage() })
}

export async function saveConfigSettings(settings: SettingsState): Promise<void> {
  const normalized = normalizeSettings(settings)
  const chromeApi = getConfigChromeApi()
  if (chromeApi?.storage?.local) {
    await chromeApi.storage.local.set(normalized)
    return
  }
  safeWriteLocalStorage(normalized)
}

function getConfigChromeApi(): ConfigChromeApi | null {
  return (globalThis as typeof globalThis & { chrome?: ConfigChromeApi }).chrome || null
}

function safeReadLocalStorage(): Partial<SettingsState> {
  try {
    const raw = globalThis.localStorage?.getItem('andaConfigSettings')
    return raw ? (JSON.parse(raw) as Partial<SettingsState>) : {}
  } catch {
    return {}
  }
}

function safeWriteLocalStorage(settings: SettingsState): void {
  try {
    globalThis.localStorage?.setItem('andaConfigSettings', JSON.stringify(settings))
  } catch {
    // Local storage is a best-effort dev fallback only.
  }
}
