import { afterEach, describe, expect, it, vi } from 'vitest'
import { isDevelopmentMode } from './chrome'
import type { ChromeApi } from './types'

afterEach(() => {
  vi.restoreAllMocks()
})

describe('isDevelopmentMode', () => {
  it('returns false when the management API is unavailable', async () => {
    await expect(isDevelopmentMode({} as ChromeApi)).resolves.toBe(false)
  })

  it('returns false when management.getSelf fails', async () => {
    const chromeApi = {
      management: {
        getSelf: vi.fn(async () => {
          throw new Error('management unavailable')
        })
      }
    } as unknown as ChromeApi

    await expect(isDevelopmentMode(chromeApi)).resolves.toBe(false)
  })

  it('detects unpacked development installs', async () => {
    const chromeApi = {
      management: {
        getSelf: vi.fn(async () => ({ installType: 'development' }))
      }
    } as unknown as ChromeApi

    await expect(isDevelopmentMode(chromeApi)).resolves.toBe(true)
  })
})
