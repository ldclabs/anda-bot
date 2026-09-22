import { afterEach, expect, it, vi } from 'vitest'
import { ConversationMemoryActivity } from './activity-store.svelte'
import { MemoryApi, type Overview, type ActivityPage } from './api'
import type { BrainGraphSettings } from '../brain/api'

afterEach(() => {
  vi.restoreAllMocks()
  vi.unstubAllGlobals()
  vi.useRealTimers()
})

it('drops late results across identities and uses only proven message references', async () => {
  vi.useFakeTimers()
  vi.stubGlobal('document', { hidden: false })
  const settings: BrainGraphSettings = {
    baseUrl: 'http://localhost:8042',
    token: 'a',
    spaceId: 'anda_bot',
    submitKeyMode: 'enter',
    appearanceTheme: 'system'
  }
  let resolveOld: (value: Overview) => void = () => {}
  vi.spyOn(MemoryApi.prototype, 'overview')
    .mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          resolveOld = resolve
        })
    )
    .mockResolvedValue({ caller: 'b', memory: { formation_active: false } } as Overview)
  const page = {
    schema_version: 1,
    complete: true,
    next_cursor: null,
    partial_reason: null,
    items: [
      {
        id: 'proof',
        conversation: '42',
        state: 'completed',
        submitted_at: 1,
        stale: false,
        provenance_complete: true,
        source_messages: [
          { conversation: '42', index: '0', role: 'user', content_digest: 'verified' }
        ]
      },
      {
        id: 'ambiguous',
        conversation: '42',
        state: 'completed',
        submitted_at: 2,
        stale: false,
        provenance_complete: false,
        source_messages: [
          { conversation: '42', index: '1', role: 'assistant', content_digest: 'unknown' }
        ]
      }
    ]
  } as ActivityPage
  const read = vi.spyOn(MemoryApi.prototype, 'activity').mockResolvedValue(page)
  const store = new ConversationMemoryActivity()
  store.configure(settings, '42', false)
  await vi.advanceTimersByTimeAsync(1)
  store.configure({ ...settings, token: 'b' }, '42', false)
  await vi.advanceTimersByTimeAsync(1)
  resolveOld({ caller: 'a' } as Overview)
  await Promise.resolve()
  expect(read).toHaveBeenCalledTimes(1)
  expect(store.messages['m-42-0']?.state).toBe('completed')
  expect(store.messages['m-42-1']).toBeUndefined()
  store.configure({ ...settings, token: 'c' }, '43', false)
  expect(store.messages).toEqual({})
  store.stop()
})
