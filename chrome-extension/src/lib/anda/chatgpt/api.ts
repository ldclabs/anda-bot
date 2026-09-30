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
    if (!native.chatgpt) throw new Error('Update Anda Desktop to connect ChatGPT')
    return native.chatgpt<T>(request)
  }
  if (!settings.token) throw new Error('Connect to your Anda daemon first')
  const response = await fetch(`${settings.baseUrl}/daemon/chatgpt`, {
    method: 'POST',
    headers: { Authorization: `Bearer ${settings.token}`, 'Content-Type': 'application/json' },
    body: JSON.stringify(request),
    redirect: 'error',
    signal: AbortSignal.timeout(60_000)
  })
  const result = await response.json()
  if (!response.ok) throw new Error(result.error || `ChatGPT request failed (${response.status})`)
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
