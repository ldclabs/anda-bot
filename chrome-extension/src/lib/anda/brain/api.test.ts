import { afterEach, describe, expect, it, vi } from 'vitest'
import {
  ANDA_BOT_SPACE_ID,
  BrainApi,
  assertKipSucceeded,
  brainPendingStorageKey,
  type BrainGraphSettings
} from './api'

function settings(spaceId = ANDA_BOT_SPACE_ID): BrainGraphSettings {
  return {
    baseUrl: 'http://127.0.0.1:8042/',
    token: 'browser-token',
    submitKeyMode: 'enter',
    appearanceTheme: 'system',
    spaceId
  }
}

afterEach(() => {
  vi.unstubAllGlobals()
})

describe('BrainApi', () => {
  it('uses extension RPC for the default Anda Bot space', async () => {
    const sendMessage = vi.fn(async () => ({
      ok: true,
      result: {
        kip: '2.0',
        status: 'succeeded',
        results: [
          {
            status: 'succeeded',
            result: [
              {
                id: 'C-1',
                kind: 'concept',
                schema_ref: 'kip://test@1.0.0/Memory',
                name: 'Node 1',
                attributes: {}
              }
            ]
          }
        ]
      }
    }))
    const fetch = vi.fn()
    vi.stubGlobal('chrome', { runtime: { sendMessage } })
    vi.stubGlobal('fetch', fetch)

    const api = new BrainApi(settings())
    const response = await api.executeKipReadonly({
      operations: [{ command: 'FIND(?node) WHERE { ?node CONCEPT {} }' }]
    })

    expect(response.results[0].result).toEqual([
      {
        id: 'C-1',
        kind: 'concept',
        schema_ref: 'kip://test@1.0.0/Memory',
        name: 'Node 1',
        attributes: {}
      }
    ])
    expect(fetch).not.toHaveBeenCalled()
    expect(sendMessage).toHaveBeenCalledWith({
      type: 'anda_rpc',
      settings: {
        baseUrl: 'http://127.0.0.1:8042',
        token: 'browser-token',
        submitKeyMode: 'enter',
        appearanceTheme: 'system',
        approvalMode: 'on_risk'
      },
      method: 'brain_kip_readonly',
      params: [{ operations: [{ command: 'FIND(?node) WHERE { ?node CONCEPT {} }' }] }]
    })
  })

  it('falls back to Brain REST for custom spaces', async () => {
    const sendMessage = vi.fn()
    const fetch = vi.fn(async () => ({
      ok: true,
      status: 200,
      statusText: 'OK',
      text: async () =>
        JSON.stringify({
          result: {
            id: 'custom',
            concepts: 1,
            propositions: 2,
            conversations: 3,
            formation_processing: false,
            maintenance_processing: false,
            formation_processed_id: 4,
            maintenance_processed_id: 5
          }
        })
    }))
    vi.stubGlobal('chrome', { runtime: { sendMessage } })
    vi.stubGlobal('fetch', fetch)

    const api = new BrainApi(settings('custom'))
    const status = await api.status()

    expect(status).toMatchObject({ id: 'custom', concepts: 1, propositions: 2 })
    expect(sendMessage).not.toHaveBeenCalled()
    expect(fetch).toHaveBeenCalledWith(
      'http://127.0.0.1:8042/v1/custom/formation_status',
      expect.objectContaining({ method: 'GET' })
    )
  })
  it('rejects partial, missing and failed operations without treating partial results as success', () => {
    const ok = { kip: '2.0', status: 'succeeded', results: [{ status: 'succeeded', result: [] }] }
    expect(() => assertKipSucceeded(ok, 1)).not.toThrow()
    expect(() => assertKipSucceeded({ ...ok, status: 'partial' }, 1)).toThrow()
    expect(() => assertKipSucceeded({ ...ok, results: [] }, 1)).toThrow()
    expect(() => assertKipSucceeded(ok, 2)).toThrow()
    expect(() =>
      assertKipSucceeded({ ...ok, results: [{ status: 'failed', result: [] }] }, 1)
    ).toThrow()
    expect(() => assertKipSucceeded({ ...ok, results: [{ status: 'succeeded' }] }, 1)).toThrow()
  })
  it('preserves application KIP fields and the caller bearer over direct HTTP', async () => {
    vi.stubGlobal('chrome', undefined)
    const fetch = vi.fn(
      async (_url: string, _init: RequestInit) =>
        new Response(
          JSON.stringify({
            kip: '2.0',
            status: 'succeeded',
            results: [{ status: 'succeeded', result: [] }]
          }),
          { status: 200 }
        )
    )
    vi.stubGlobal('fetch', fetch)
    const request = {
      operations: [{ op_id: 'read-1', command: 'DESCRIBE PRIMER', parameters: { name: 'bound' } }],
      execution: { mode: 'independent' as const },
      read: { snapshot_token: 'opaque' },
      parameters: { shared: 1 },
      dry_run: true
    }
    await new BrainApi(settings()).executeKipReadonly(request)
    const init = fetch.mock.calls[0][1] as RequestInit
    expect(JSON.parse(init.body as string)).toEqual(request)
    expect(new Headers(init.headers).get('Authorization')).toBe('Bearer browser-token')
    expect(JSON.parse(init.body as string)).not.toHaveProperty('kip')
  })

  it('uses separate runtime RPCs and preserves stable response event identity', async () => {
    const sendMessage = vi.fn(async (_message: unknown) => ({
      ok: true,
      result: { receipt_id: 'r', status: 'answer_received_not_authorization' }
    }))
    vi.stubGlobal('chrome', { runtime: { sendMessage } })
    const api = new BrainApi(settings())
    const response = { kind: 'clarification' as const, event_key: 'same-event', answer: 'Tomorrow' }
    const id = 'a'.repeat(64)
    await api.respond(id, response)
    await api.respond(id, response)
    expect(sendMessage.mock.calls[0][0]).toMatchObject({
      method: 'brain_respond',
      params: [id, response],
      settings: { token: 'browser-token' }
    })
    expect(sendMessage.mock.calls[1][0]).toEqual(sendMessage.mock.calls[0][0])
    await api.runtimeStatus()
    expect(sendMessage.mock.calls[2][0]).toMatchObject({ method: 'brain_runtime_status' })
    await expect(api.respond(`wake/v1/${id}`, response)).rejects.toThrow('Invalid attention')
  })

  it('does not fall back to another identity after a runtime rejection', async () => {
    vi.stubGlobal('chrome', {
      runtime: { sendMessage: vi.fn(async () => ({ ok: false, error: '403: revoked' })) }
    })
    const fetch = vi.fn()
    vi.stubGlobal('fetch', fetch)
    await expect(new BrainApi(settings()).attention('expired-cursor')).rejects.toThrow('revoked')
    expect(fetch).not.toHaveBeenCalled()
  })

  it('keeps pending response identity stable across bearer rotation', async () => {
    const before = settings()
    const after = { ...before, token: 'rotated-browser-token' }

    expect(await brainPendingStorageKey(before, 'owner-principal')).toBe(
      await brainPendingStorageKey(after, 'owner-principal')
    )
    expect(await brainPendingStorageKey(before, 'owner-principal')).not.toBe(
      await brainPendingStorageKey(after, 'other-principal')
    )
    expect(await brainPendingStorageKey(before)).not.toBe(await brainPendingStorageKey(after))
  })
})
