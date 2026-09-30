import { afterEach, describe, expect, it, vi } from 'vitest'
import { setClientPlatform, type ClientPlatform } from '../client/platform'
import { chatgptRequest, openChatGptUrl } from './api'
const settings = {
  baseUrl: 'http://127.0.0.1:8042',
  token: 'local-token',
  appearanceTheme: 'system' as const,
  submitKeyMode: 'enter' as const
}
afterEach(() => {
  setClientPlatform(undefined)
  vi.restoreAllMocks()
})
describe('ChatGPT account transport', () => {
  it('keeps desktop credentials in Main and never falls back to renderer fetch', async () => {
    const fetch = vi.spyOn(globalThis, 'fetch').mockRejectedValue(new Error('must not fetch'))
    const chatgpt = vi.fn().mockResolvedValue({ accounts: [] })
    setClientPlatform({ chatgpt } as unknown as ClientPlatform)
    await chatgptRequest(settings, { method: 'accounts' })
    expect(chatgpt).toHaveBeenCalledWith({ method: 'accounts' })
    chatgpt.mockRejectedValue(new Error('disconnected'))
    await expect(chatgptRequest(settings, { method: 'accounts' })).rejects.toThrow('disconnected')
    expect(fetch).not.toHaveBeenCalled()
  })
  it('authenticates extension requests to the configured daemon and refuses redirects', async () => {
    const fetch = vi
      .spyOn(globalThis, 'fetch')
      .mockResolvedValue(new Response(JSON.stringify({ accounts: [] })))
    await chatgptRequest(settings, { method: 'accounts' })
    expect(fetch).toHaveBeenCalledWith(
      'http://127.0.0.1:8042/daemon/chatgpt',
      expect.objectContaining({
        redirect: 'error',
        headers: expect.objectContaining({ Authorization: 'Bearer local-token' })
      })
    )
  })
  it('does not open arbitrary login destinations', async () => {
    await expect(openChatGptUrl('https://evil.invalid/')).rejects.toThrow('Unexpected')
    await expect(openChatGptUrl('javascript:alert(1)')).rejects.toThrow('Unexpected')
    await expect(openChatGptUrl('https://user:password@auth.openai.com/')).rejects.toThrow(
      'Unexpected'
    )
  })
})
