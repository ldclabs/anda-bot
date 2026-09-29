import { afterEach, describe, expect, it, vi } from 'vitest'
import { MemoryApi, unwrap } from './api'
import type { SettingsState } from '$lib/service-worker/types'

const settings: SettingsState = {
  baseUrl: 'http://127.0.0.1:8042',
  token: 'owner-token',
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
  it('pages an entity by its cursor and looks entities up by name', async () => {
    const page = {
      schema_version: 1,
      entity: { id: 'C-1', name: 'Owner', type: 'Person', about_owner: true },
      items: [],
      complete: false,
      partial_reason: 'scan_limit'
    }
    const sendMessage = vi.fn(async (message: { method: string; params: unknown }) => ({
      ok: true,
      result:
        message.method === 'memory_entity'
          ? { result: page, next_cursor: 'next-page' }
          : { result: { schema_version: 1, items: [page.entity] } }
    }))
    vi.stubGlobal('chrome', { runtime: { sendMessage } })
    const api = new MemoryApi(settings)
    expect(await api.entity(null)).toEqual({ ...page, next_cursor: 'next-page' })
    await api.entity('C-1', 'next-page')
    expect(sendMessage.mock.calls.map(([message]) => message.params)).toEqual([
      [{ id: null, cursor: null, limit: 20 }],
      [{ id: 'C-1', cursor: 'next-page', limit: 20 }]
    ])
    expect((await api.entitySearch('Owner')).items).toEqual([page.entity])
    expect(sendMessage).toHaveBeenLastCalledWith(
      expect.objectContaining({ method: 'memory_entity_search', params: [{ query: 'Owner' }] })
    )
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
  it('does not send a read that was already cancelled', async () => {
    const sendMessage = vi.fn()
    vi.stubGlobal('chrome', { runtime: { sendMessage } })
    const controller = new AbortController()
    controller.abort()
    await expect(new MemoryApi(settings).overview(controller.signal)).rejects.toThrow()
    expect(sendMessage).not.toHaveBeenCalled()
  })
  it('preserves a missing draft status so recovery can offer review or discard', async () => {
    vi.stubGlobal('chrome', {
      runtime: {
        sendMessage: vi.fn(async () => ({
          ok: true,
          result: { error: { code: 'not_found', message: 'No operation' } }
        }))
      }
    })
    await expect(new MemoryApi(settings).changeStatus('saved-prepare-id')).rejects.toMatchObject({
      code: 'not_found'
    })
  })
})

it('loads every watch page, including empty history pages, without clearing partial errors', async () => {
  const pages = [
    {
      result: {
        schema_version: 1,
        items: [],
        complete: false,
        partial_reason: 'watch_status_unavailable'
      },
      next_cursor: 'history'
    },
    {
      result: {
        schema_version: 1,
        items: [{ operation_id: 'a', state: 'armed' }],
        complete: false
      },
      next_cursor: 'active'
    },
    {
      result: {
        schema_version: 1,
        items: [
          { operation_id: 'b', state: 'armed' },
          { operation_id: 'old', state: 'cancelled' }
        ],
        complete: true
      }
    }
  ]
  const sendMessage = vi.fn(async () => ({ ok: true, result: pages.shift() }))
  vi.stubGlobal('chrome', { runtime: { sendMessage } })
  const result = await new MemoryApi(settings).watches()
  expect(result.items.map((watch) => watch.operation_id)).toEqual(['a', 'b'])
  expect(result.complete).toBe(false)
  expect(sendMessage).toHaveBeenNthCalledWith(
    2,
    expect.objectContaining({
      method: 'memory_watches',
      params: [{ cursor: 'history', limit: 50 }]
    })
  )
})

it('completes a paginated watch list and stops if pagination is cancelled', async () => {
  const controller = new AbortController()
  const sendMessage = vi
    .fn()
    .mockResolvedValueOnce({
      ok: true,
      result: {
        result: { schema_version: 1, items: [], complete: false },
        next_cursor: 'next'
      }
    })
    .mockResolvedValueOnce({
      ok: true,
      result: {
        result: {
          schema_version: 1,
          items: [{ operation_id: 'last', state: 'armed' }],
          complete: true
        }
      }
    })
  vi.stubGlobal('chrome', { runtime: { sendMessage } })
  const result = await new MemoryApi(settings).watches(controller.signal)
  expect(result.complete).toBe(true)
  expect(result.items[0]?.operation_id).toBe('last')
  expect(sendMessage.mock.calls[1]?.[0]).toMatchObject({
    params: [{ cursor: 'next', limit: 50 }]
  })
  const stopped = new AbortController()
  stopped.abort()
  await expect(new MemoryApi(settings).watches(stopped.signal)).rejects.toThrow()
  expect(sendMessage).toHaveBeenCalledTimes(2)
})
