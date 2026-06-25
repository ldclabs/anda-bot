import { afterEach, describe, expect, it, vi } from 'vitest'
import { ANDA_BOT_SPACE_ID, BrainApi, type BrainGraphSettings } from './api'

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
        result: [{ id: 'node-1', type: 'Memory', name: 'Node 1', attributes: {} }]
      }
    }))
    const fetch = vi.fn()
    vi.stubGlobal('chrome', { runtime: { sendMessage } })
    vi.stubGlobal('fetch', fetch)

    const api = new BrainApi(settings())
    const response = await api.executeKipReadonly({ command: 'FIND(?node) WHERE { ?node {} }' })

    expect(response.result).toEqual([
      { id: 'node-1', type: 'Memory', name: 'Node 1', attributes: {} }
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
      params: [{ command: 'FIND(?node) WHERE { ?node {} }' }]
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
})
