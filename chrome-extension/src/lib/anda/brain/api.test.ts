import { afterEach, describe, expect, it, vi } from 'vitest'
import { BrainApi, brainPendingStorageKey } from './api'
import type { SettingsState } from '$lib/service-worker/types'

function settings(): SettingsState {
  return {
    baseUrl: 'http://127.0.0.1:8042/',
    token: 'browser-token',
    submitKeyMode: 'enter',
    appearanceTheme: 'system'
  }
}

afterEach(() => {
  vi.unstubAllGlobals()
})

describe('BrainApi', () => {
  it('sends Brain calls through the extension RPC with normalized settings', async () => {
    const page = { scope: { space_id: 'anda_bot', space_instance: 'i' }, items: [], complete: true }
    const sendMessage = vi.fn(async () => ({ ok: true, result: page }))
    const fetch = vi.fn()
    vi.stubGlobal('chrome', { runtime: { sendMessage } })
    vi.stubGlobal('fetch', fetch)

    expect(await new BrainApi(settings()).attention('next')).toEqual(page)
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
      method: 'brain_attention',
      params: [{ cursor: 'next', limit: 20 }]
    })
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

  it('finds pending responses saved under the Anda Bot space key', async () => {
    const bytes = await crypto.subtle.digest(
      'SHA-256',
      new TextEncoder().encode(
        JSON.stringify(['http://127.0.0.1:8042/', 'anda_bot', ['caller', 'owner-principal']])
      )
    )
    const saved = Array.from(new Uint8Array(bytes), (byte) =>
      byte.toString(16).padStart(2, '0')
    ).join('')

    expect(await brainPendingStorageKey(settings(), 'owner-principal')).toBe(
      `brain-responses:${saved}`
    )
  })
})
