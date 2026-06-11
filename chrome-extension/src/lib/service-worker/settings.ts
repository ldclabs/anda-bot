import type { AppearanceTheme, ChromeApi, SettingsState, SubmitKeyMode } from './types'
import { getCurrentBrowser } from './chrome'

export const defaultSettings: SettingsState = {
  baseUrl: 'http://127.0.0.1:8042',
  token: '',
  submitKeyMode: 'enter',
  appearanceTheme: 'system'
}

const browserSessionStorageKey = 'browserSessionId'

export async function loadSettings(chromeApi: ChromeApi): Promise<SettingsState> {
  const saved = await chromeApi.storage.local.get([
    'baseUrl',
    'token',
    'submitKeyMode',
    'appearanceTheme'
  ])
  return normalizeSettings({
    baseUrl: saved.baseUrl || defaultSettings.baseUrl,
    token: saved.token || '',
    submitKeyMode: saved.submitKeyMode || defaultSettings.submitKeyMode,
    appearanceTheme: saved.appearanceTheme || defaultSettings.appearanceTheme
  })
}

export async function browserSession(chromeApi: ChromeApi): Promise<string> {
  const saved = await chromeApi.storage.local.get([browserSessionStorageKey])
  let id = saved.browserSessionId || '0'
  // Regenerate when the stored value is missing, too small, or not numeric (NaN fails the check).
  if (!(parseInt(id, 10) >= 1000)) {
    id = Date.now().toString()
    await chromeApi.storage.local.set({ browserSessionId: id })
  }
  let scope = await browserSessionScope(chromeApi)
  return `browser:${scope}:${id}`
}

async function browserSessionScope(chromeApi: ChromeApi): Promise<string> {
  let browser = await getCurrentBrowser()
  return chromeApi.extension?.inIncognitoContext ? `incognito_${browser}` : browser
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
    submitKeyMode: normalizeSubmitKeyMode(settings.submitKeyMode),
    appearanceTheme: normalizeAppearanceTheme(settings.appearanceTheme)
  }
}

function normalizeSubmitKeyMode(value: unknown): SubmitKeyMode {
  return value === 'modifier-enter' ? 'modifier-enter' : 'enter'
}

export function normalizeAppearanceTheme(value: unknown): AppearanceTheme {
  return value === 'light' || value === 'dark' ? value : 'system'
}

function trimTrailingSlash(value: string): string {
  return String(value || '').replace(/\/+$/, '')
}

export function errorToMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error)
}

export function errorToCode(error: unknown): string | undefined {
  if (!error || typeof error !== 'object') {
    return undefined
  }
  const code = (error as { code?: unknown }).code
  return typeof code === 'string' && code.trim() ? code.trim() : undefined
}

export function errorToError(error: unknown): Error {
  return error instanceof Error ? error : new Error(String(error))
}

export function isTransientWebSocketError(error: unknown): boolean {
  const message = errorToMessage(error).toLowerCase()
  return (
    message.includes('websocket') &&
    (message.includes('timed out') ||
      message.includes('timeout') ||
      message.includes('closed') ||
      message.includes('disconnected') ||
      message.includes('not connected'))
  )
}
