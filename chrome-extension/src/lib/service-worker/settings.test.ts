import { describe, expect, it, vi } from 'vitest'
import { browserSession } from './settings'
import type { ChromeApi } from './types'

function createChromeApi(initialSessionId: unknown): ChromeApi {
  const state: Record<string, unknown> = {}
  if (initialSessionId !== undefined) {
    state.browserSessionId = initialSessionId
  }
  return {
    extension: { inIncognitoContext: false },
    storage: {
      local: {
        get: vi.fn(async (keys: string[]) => {
          const result: Record<string, unknown> = {}
          for (const key of keys) {
            if (key in state) {
              result[key] = state[key]
            }
          }
          return result
        }),
        set: vi.fn(async (items: Record<string, unknown>) => {
          Object.assign(state, items)
        })
      }
    }
  } as unknown as ChromeApi
}

function sessionIdPart(session: string): string {
  return session.split(':').pop() || ''
}

describe('browserSession', () => {
  it('keeps a valid stored session id', async () => {
    const chromeApi = createChromeApi('1700000000000')
    const session = await browserSession(chromeApi)
    expect(sessionIdPart(session)).toBe('1700000000000')
    expect(chromeApi.storage.local.set).not.toHaveBeenCalled()
  })

  it('regenerates a missing session id', async () => {
    const chromeApi = createChromeApi(undefined)
    const session = await browserSession(chromeApi)
    expect(Number.parseInt(sessionIdPart(session), 10)).toBeGreaterThanOrEqual(1000)
    expect(chromeApi.storage.local.set).toHaveBeenCalledWith({
      browserSessionId: sessionIdPart(session)
    })
  })

  it('regenerates a corrupted non-numeric session id', async () => {
    const chromeApi = createChromeApi('not-a-number')
    const session = await browserSession(chromeApi)
    expect(sessionIdPart(session)).not.toBe('not-a-number')
    expect(Number.parseInt(sessionIdPart(session), 10)).toBeGreaterThanOrEqual(1000)
    expect(chromeApi.storage.local.set).toHaveBeenCalledWith({
      browserSessionId: sessionIdPart(session)
    })
  })
})
