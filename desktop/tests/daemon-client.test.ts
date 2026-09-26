import { afterEach, describe, expect, it } from 'vitest'
import { mkdtemp, readFile, rm } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { randomUUID } from 'node:crypto'
import { WebSocketServer } from 'ws'
import { DaemonClient } from '../src/main/daemon-client'
import { DesktopStore } from '../src/main/store'

const cleanup: Array<() => Promise<void> | void> = []
afterEach(async () => {
  for (const f of cleanup.splice(0).reverse()) await f()
})

async function fixture(dropSubmission = false, appTransport = false, failSubmission = false) {
  const directory = await mkdtemp(join(tmpdir(), 'anda-desktop-test-'))
  cleanup.push(() => rm(directory, { recursive: true, force: true }))
  const store = new DesktopStore(join(directory, 'desktop.json'))
  await store.load()
  const server = new WebSocketServer({ host: '127.0.0.1', port: 0 })
  await new Promise<void>((resolve) => server.once('listening', resolve))
  cleanup.push(
    () =>
      new Promise<void>((resolve) => {
        for (const ws of server.clients) ws.terminate()
        server.close(() => resolve())
      })
  )
  let submissions = 0
  const receipts = new Map<string, unknown>()
  const authorization: string[] = []
  server.on('connection', (ws, request) => {
    authorization.push(request.headers.authorization || '')
    expect(request.url).not.toContain('token')
    ws.on('message', (data) => {
      const message = JSON.parse(data.toString())
      if (message.method === 'chat/submit') {
        expect(message.jsonrpc).toBe('2.0')
        receipts.set(
          message.params.requestId,
          failSubmission
            ? { state: 'failed', error: 'Task rejected' }
            : { state: 'completed', result: { conversation: 7 } }
        )
      }
      if (['agent_run', 'chat/submit'].includes(message.method)) {
        submissions++
        if (dropSubmission) {
          ws.terminate()
          return
        }
      }
      const reply = () =>
        ws.send(
          JSON.stringify({
            id: message.id,
            result:
              message.method === 'initialize'
                ? {
                    protocolVersion: 1,
                    instanceId: 'fixture',
                    capabilities: { stateInvalidation: true, submissionReceipts: true }
                  }
                : message.method === 'capabilities'
                  ? { desktop: { app_transport: appTransport } }
                  : message.method === 'chat/submit'
                    ? receipts.get(message.params.requestId)
                    : message.method === 'submission/read'
                      ? receipts.get(message.params.requestId) || null
                      : message.method === 'agent_run'
                        ? { conversation: 7 }
                        : {}
          })
        )
      if (message.params?.[0]?.prompt?.startsWith('/side ')) setTimeout(reply, 50)
      else reply()
    })
  })
  const address = server.address() as { port: number }
  const client = new DaemonClient(directory, directory, store, `http://127.0.0.1:${address.port}`)
  cleanup.push(() => client.disconnect())
  await client.connect()
  return {
    client,
    store,
    directory,
    authorization,
    submissions: () => submissions,
    notify: () => {
      for (const ws of server.clients)
        ws.send(
          JSON.stringify({
            jsonrpc: '2.0',
            method: 'state/changed',
            params: { instanceId: 'test', revision: '1' }
          })
        )
    }
  }
}

describe('daemon transport and recovery', () => {
  it('uses negotiated application transport and forwards caller state invalidation', async () => {
    const f = await fixture(false, true)
    expect(f.client.view.liveEvents).toBe(true)
    const event = new Promise((resolve) => f.client.once('state', resolve))
    f.notify()
    expect(await event).toMatchObject({ revision: '1' })
    expect(
      await f.client.rpc('agent_run', [
        { name: '', prompt: 'hello', meta: { source: 'desktop:one' } }
      ])
    ).toEqual({ conversation: 7 })
    expect(f.submissions()).toBe(1)
    expect(f.store.state.pending).toMatchObject([{ state: 'completed', receipt: true }])
    const id = f.store.state.pending[0]!.id
    const restored = new DesktopStore(join(f.directory, 'desktop.json'))
    await restored.load()
    expect(restored.state.pending[0]?.id).toBe(id)
    expect((await f.client.readSubmission(id))?.result).toEqual({ conversation: 7 })
    await f.client.acknowledgeSubmission(id)
    expect(f.store.state.pending).toEqual([])
    await restored.load()
    expect(restored.state.pending).toEqual([])
  })
  it('retains a failed receipt until the renderer acknowledges its error', async () => {
    const f = await fixture(false, true, true)
    const id = randomUUID()
    await expect(
      f.client.rpc(
        'agent_run',
        [{ name: '', prompt: 'hello', meta: { source: 'desktop:one' } }],
        id
      )
    ).rejects.toThrow('Task rejected')
    expect(f.store.state.pending).toMatchObject([{ id, state: 'failed' }])
    expect(await f.client.readSubmission(id)).toMatchObject({
      state: 'failed',
      error: 'Task rejected'
    })
    await f.client.acknowledgeSubmission(id)
    expect(f.store.state.pending).toEqual([])
  })
  it('reconciles an ACK lost after server acceptance without resubmitting', async () => {
    const f = await fixture(true, true)
    await expect(
      f.client.rpc('agent_run', [{ name: '', prompt: 'hello', meta: { source: 'desktop:one' } }])
    ).rejects.toThrow('SUBMISSION_UNKNOWN')
    expect(f.store.state.pending).toHaveLength(1)
    await f.client.connect()
    expect(f.store.state.pending[0]?.state).toBe('completed')
    expect((await f.client.readSubmission(f.store.state.pending[0]!.id))?.result).toEqual({
      conversation: 7
    })
    const restored = new DesktopStore(join(f.directory, 'desktop.json'))
    await restored.load()
    expect(restored.state.pending[0]?.id).toBe(f.store.state.pending[0]?.id)
    expect(f.submissions()).toBe(1)
  })
  it('allows a new foreground message while a side request is awaiting its reply', async () => {
    const f = await fixture()
    const side = f.client.rpc('agent_run', [
      { name: '', prompt: '/side inspect', meta: { source: 'desktop:one' } }
    ])
    await new Promise((resolve) => setTimeout(resolve, 10))
    const foreground = f.client.rpc('agent_run', [
      { name: '', prompt: 'continue', meta: { source: 'desktop:one' } }
    ])
    await Promise.all([side, foreground])
    expect(f.submissions()).toBe(2)
    expect(f.store.state.pending).toEqual([])
  })
  it('does not restart a deliberately stopped daemon when a view polls', async () => {
    const f = await fixture()
    f.client.manuallyStopped = true
    f.client.disconnect()
    await expect(f.client.rpc('information', [])).rejects.toThrow('stopped')
    expect(f.authorization).toHaveLength(1)
  })
  it('authenticates in a header, accepts a submission and persists no bearer', async () => {
    const f = await fixture()
    expect(f.client.view.connected).toBe(true)
    expect(
      await f.client.rpc('agent_run', [
        { name: '', prompt: 'hello', meta: { source: 'desktop:one' } }
      ])
    ).toEqual({ conversation: 7 })
    expect(f.authorization).toEqual(['Bearer desktop-test-token'])
    expect(f.store.state.pending).toEqual([])
    expect(await readFile(join(f.directory, 'desktop.json'), 'utf8')).not.toContain(
      'desktop-test-token'
    )
  })
  it('never retries a submission whose acknowledgement was lost, including after restart', async () => {
    const f = await fixture(true)
    const params = [{ name: '', prompt: 'do work', meta: { source: 'desktop:one' } }]
    await expect(f.client.rpc('agent_run', params)).rejects.toThrow('SUBMISSION_UNKNOWN')
    await f.client.connect()
    await expect(f.client.rpc('agent_run', params)).rejects.toThrow('unconfirmed')
    expect(f.submissions()).toBe(1)
    const restored = new DesktopStore(join(f.directory, 'desktop.json'))
    await restored.load()
    expect(restored.state.pending[0]?.state).toBe('unknown')
    expect(restored.state.pending[0]?.source).toBe('desktop:one')
  })
  it('preserves the latest serialized settings when several writes overlap', async () => {
    const f = await fixture()
    const writes = []
    for (let i = 0; i < 10; i++) {
      f.store.state.preferences.drafts.one = `draft ${i}`
      writes.push(f.store.save())
    }
    await Promise.all(writes)
    const restored = new DesktopStore(join(f.directory, 'desktop.json'))
    await restored.load()
    expect(restored.state.preferences.drafts.one).toBe('draft 9')
  })
})
