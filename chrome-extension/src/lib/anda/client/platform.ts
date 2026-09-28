import { normalizeSettings } from '$lib/service-worker/settings'
import type { SettingsState } from './types'

/** Optional native transport. Credentials stay in the host, never in the web UI. */
export interface ClientPlatform {
  settings(): Promise<SettingsState>
  saveSettings(settings: SettingsState): Promise<void>
  rpc<Result>(method: string, params: unknown[]): Promise<Result>
  config<Result>(
    method: 'GET' | 'PUT',
    content?: string,
    expectedRevision?: string
  ): Promise<Result>
  storage: {
    get(keys: string[]): Promise<Record<string, unknown>>
    set(items: Record<string, unknown>): Promise<void>
  }
  openChat(): Promise<void>
  printHtml?(html: string): Promise<void>
}

let nativePlatform: ClientPlatform | undefined

export function setClientPlatform(platform: ClientPlatform | undefined): void {
  nativePlatform = platform
}

export function getClientPlatform(): ClientPlatform | undefined {
  return nativePlatform
}

export async function storeClientState(items: Record<string, unknown>): Promise<void> {
  if (nativePlatform) return nativePlatform.storage.set(items)
  await chrome.storage.local.set(items)
}

/**
 * Calls a daemon RPC through the host's transport: the native bridge in Anda
 * Desktop, otherwise the extension service worker's WebSocket. A failed call
 * is never retried over another transport or identity.
 */
export async function daemonRpc<Result>(
  method: string,
  params: unknown[],
  settings: SettingsState
): Promise<Result> {
  if (nativePlatform) return nativePlatform.rpc<Result>(method, params)
  if (!settings.token) throw new Error('missing bearer token')
  const response = await chrome.runtime.sendMessage({
    type: 'anda_rpc',
    settings: normalizeSettings(settings),
    method,
    params
  })
  if (!response?.ok) throw new Error(response?.error || `${method} failed`)
  return response.result as Result
}
