import { afterEach, describe, expect, it, vi } from 'vitest'
import { MemoryApi, unwrap } from './api'
import type { BrainGraphSettings } from '../brain/api'

const settings: BrainGraphSettings = {
  baseUrl: 'http://127.0.0.1:8042',
  token: 'owner-token',
  spaceId: 'anda_bot',
  submitKeyMode: 'enter',
  appearanceTheme: 'system'
}
afterEach(() => vi.unstubAllGlobals())
describe('memory product API', () => {
  it('sends search and subscription mutations once with their fixed request identity', async () => {
    const sendMessage = vi.fn(async () => ({
      ok: true,
      result: { error: { code: 'acceptance_unknown', message: 'ack lost' } }
    }))
    vi.stubGlobal('chrome', { runtime: { sendMessage } })
    await expect(new MemoryApi(settings).watch('fixed-id', 'A-42')).rejects.toThrow(
      'acceptance_unknown'
    )
    expect(sendMessage).toHaveBeenCalledTimes(1)
    expect(sendMessage).toHaveBeenCalledWith(
      expect.objectContaining({
        method: 'memory_watch',
        params: [{ operation_id: 'fixed-id', record_id: 'A-42' }]
      })
    )
    await expect(new MemoryApi(settings).search('preference')).rejects.toThrow('acceptance_unknown')
    expect(sendMessage).toHaveBeenCalledTimes(2)
  })
  it('never converts a partial error or unknown schema into successful data', () => {
    expect(() =>
      unwrap({ error: { code: 'unavailable', message: 'failed' }, result: { schema_version: 1 } })
    ).toThrow('failed')
    expect(() => unwrap({ result: { schema_version: 2 } })).toThrow('unsupported_memory_api')
  })
  it('keeps the original extension identity and does not retry failed RPC via HTTP', async () => {
    const sendMessage = vi.fn(async () => ({ ok: false, error: 'forbidden' }))
    const fetch = vi.fn()
    vi.stubGlobal('chrome', { runtime: { sendMessage } })
    vi.stubGlobal('fetch', fetch)
    await expect(new MemoryApi(settings).overview()).rejects.toThrow('forbidden')
    expect(sendMessage).toHaveBeenCalledWith(
      expect.objectContaining({
        method: 'memory_overview',
        params: [],
        settings: expect.objectContaining({ token: 'owner-token' })
      })
    )
    expect(fetch).not.toHaveBeenCalled()
  })
  it('preserves cursor and incomplete inventory without treating page completion as saved facts', async () => {
    vi.stubGlobal('chrome', {
      runtime: {
        sendMessage: vi.fn(async () => ({
          ok: true,
          result: {
            result: {
              schema_version: 1,
              items: [],
              complete: false,
              partial_reason: 'projection_incomplete'
            },
            next_cursor: 'opaque'
          }
        }))
      }
    })
    const page = await new MemoryApi(settings).activity()
    expect(page.next_cursor).toBe('opaque')
    expect(page.complete).toBe(false)
    expect(page.partial_reason).toBe('projection_incomplete')
  })
  it('recognizes old HTTP servers and forwards cancellation', async () => {
    vi.stubGlobal('chrome', undefined)
    const fetch = vi.fn(async () => new Response('', { status: 404 }))
    vi.stubGlobal('fetch', fetch)
    const controller = new AbortController()
    await expect(new MemoryApi(settings).overview(controller.signal)).rejects.toThrow(
      'unsupported_memory_api'
    )
    expect(fetch).toHaveBeenCalledWith(
      'http://127.0.0.1:8042/daemon/memory/v1/overview',
      expect.objectContaining({
        signal: controller.signal,
        headers: expect.objectContaining({ Authorization: 'Bearer owner-token' })
      })
    )
  })
  it('preserves a missing draft status so recovery can offer review or discard', async () => {
    vi.stubGlobal('chrome', undefined)
    vi.stubGlobal(
      'fetch',
      vi.fn(
        async () =>
          new Response(JSON.stringify({ error: { code: 'not_found', message: 'No operation' } }), {
            status: 404,
            headers: { 'Content-Type': 'application/json' }
          })
      )
    )
    await expect(new MemoryApi(settings).changeStatus('saved-prepare-id')).rejects.toMatchObject({
      code: 'not_found'
    })
  })
})
