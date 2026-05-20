import type { ChromeApi, SettingsState, SubmitKeyMode } from './types'

export const defaultSettings: SettingsState = {
  baseUrl: 'http://127.0.0.1:8042',
  token: '',
  submitKeyMode: 'enter'
}

const browserSessionStorageKey = 'browserSessionId'

export async function loadSettings(chromeApi: ChromeApi): Promise<SettingsState> {
  const saved = await chromeApi.storage.local.get(['baseUrl', 'token', 'submitKeyMode'])
  return normalizeSettings({
    baseUrl: saved.baseUrl || defaultSettings.baseUrl,
    token: saved.token || '',
    submitKeyMode: saved.submitKeyMode || defaultSettings.submitKeyMode
  })
}

export async function browserSession(chromeApi: ChromeApi): Promise<string> {
  const saved = await chromeApi.storage.local.get([browserSessionStorageKey])
  let id = saved.browserSessionId || '0'
  if (parseInt(id, 10) < 1000) {
    id = Date.now().toString()
    await chromeApi.storage.local.set({ browserSessionId: id })
  }
  return `browser:${browserSessionScope(chromeApi)}:${id}`
}

function browserSessionScope(chromeApi: ChromeApi): string {
  return chromeApi.extension?.inIncognitoContext ? 'incognito' : 'chrome'
}

export function websocketUrl(settings: SettingsState): string {
  const base = trimTrailingSlash(settings.baseUrl)
  const wsBase = base.replace(/^http:/i, 'ws:').replace(/^https:/i, 'wss:')
  return `${wsBase}/ws/engine/default?token=${encodeURIComponent(settings.token)}`
}

export function connectionKey(settings: SettingsState): string {
  return `${trimTrailingSlash(settings.baseUrl)}\n${settings.token}`
}

export function normalizeSettings(settings: SettingsState): SettingsState {
  return {
    baseUrl: trimTrailingSlash(settings.baseUrl.trim() || defaultSettings.baseUrl),
    token: settings.token.trim(),
    submitKeyMode: normalizeSubmitKeyMode(settings.submitKeyMode)
  }
}

function normalizeSubmitKeyMode(value: unknown): SubmitKeyMode {
  return value === 'modifier-enter' ? 'modifier-enter' : 'enter'
}

function trimTrailingSlash(value: string): string {
  return String(value || '').replace(/\/+$/, '')
}

export function errorToMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error)
}

export function errorToError(error: unknown): Error {
  return error instanceof Error ? error : new Error(String(error))
}

export function isTransientWebSocketError(error: unknown): boolean {
  const message = errorToMessage(error).toLowerCase()
  return (
    message.includes('websocket connection closed') ||
    message.includes('websocket connection timed out') ||
    message.includes('websocket is not connected')
  )
}
