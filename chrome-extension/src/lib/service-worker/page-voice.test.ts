import { describe, expect, it, vi } from 'vitest'
import { handlePageAudioCapture, handlePageSpeechRecognition } from './page-voice'
import type { ChromeApi } from './types'

function createChromeApi(): ChromeApi {
  const tab = { id: 7, active: true, windowId: 1, url: 'https://example.com/' }
  return {
    tabs: {
      get: vi.fn(async () => tab),
      query: vi.fn(async () => [tab])
    },
    storage: {
      local: { get: vi.fn(async () => ({})), set: vi.fn(async () => undefined) },
      session: {
        get: vi.fn(async () => ({})),
        set: vi.fn(async () => undefined),
        remove: vi.fn(async () => undefined)
      }
    },
    scripting: {
      executeScript: vi.fn(async () => [{ result: { available: true, started: true } }])
    }
  } as unknown as ChromeApi
}

describe('page voice capture', () => {
  it('keeps recorded audio out of the page world', async () => {
    const chromeApi = createChromeApi()

    await handlePageAudioCapture(chromeApi, { action: 'start' })

    expect(chromeApi.scripting.executeScript).toHaveBeenCalledWith(
      expect.objectContaining({ target: { tabId: 7 }, world: 'ISOLATED' })
    )
  })

  it('keeps speech transcripts out of the page world', async () => {
    const chromeApi = createChromeApi()

    await handlePageSpeechRecognition(chromeApi, { action: 'start', language: 'en-US' })

    expect(chromeApi.scripting.executeScript).toHaveBeenCalledWith(
      expect.objectContaining({ target: { tabId: 7 }, world: 'ISOLATED' })
    )
  })
})
