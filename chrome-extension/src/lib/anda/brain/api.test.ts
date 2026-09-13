import { afterEach, describe, expect, it, vi } from 'vitest'
import { ANDA_BOT_SPACE_ID, BrainApi, assertKipSucceeded, type BrainGraphSettings } from './api'

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
      kip: '2.0',
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
      params: [{ kip: '2.0', operations: [{ command: 'FIND(?node) WHERE { ?node CONCEPT {} }' }] }]
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
})
