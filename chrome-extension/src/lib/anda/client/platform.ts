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
