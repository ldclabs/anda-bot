import { getMessage } from '$lib/i18n'
import { getClientPlatform } from '../client/platform'
import type { SettingsState } from '../client/types'

import type { ChatGptRequest } from './types'
export type {
  ChatGptRequest,
  ChatGptAccount,
  ChatGptAccounts,
  ChatGptLogin,
  ChatGptModel
} from './types'
export const usageUrl = 'https://chatgpt.com/settings/usage'

export async function chatgptRequest<T>(
  settings: SettingsState,
  request: ChatGptRequest
): Promise<T> {
  const native = getClientPlatform()
  if (native) {
    if (!native.chatgpt)
      throw new Error(
        getMessage('chatgptUpdateDesktop') || 'Update Anda Desktop to connect ChatGPT'
      )
    return native.chatgpt<T>(request)
  }
  if (!settings.token) throw new Error(getMessage('pasteTokenFirst') || 'missing bearer token')
  const response = await fetch(`${settings.baseUrl}/daemon/chatgpt`, {
    method: 'POST',
    headers: { Authorization: `Bearer ${settings.token}`, 'Content-Type': 'application/json' },
    body: JSON.stringify(request),
    redirect: 'error',
    signal: AbortSignal.timeout(60_000)
  })
  // An older daemon or a proxy can answer with an empty or HTML body.
  const text = await response.text()
  let result: { error?: string } | null = null
  try {
    result = text ? JSON.parse(text) : null
  } catch {
    result = null
  }
  if (!response.ok || !result) {
    const status = String(response.status)
    throw new Error(
      result?.error ||
        getMessage('chatgptRequestFailed', status) ||
        `ChatGPT request failed (${status})`
    )
  }
  return result as T
}
export async function openChatGptUrl(url: string): Promise<void> {
  const parsed = new URL(url)
  if (
    parsed.protocol !== 'https:' ||
    !['auth.openai.com', 'chatgpt.com'].includes(parsed.hostname) ||
    parsed.username ||
    parsed.password ||
    parsed.port
  )
    throw new Error('Unexpected ChatGPT URL')
  const native = getClientPlatform()
  if (native?.openExternal) await native.openExternal(parsed.href)
  else await chrome.tabs.create({ url: parsed.href })
}
