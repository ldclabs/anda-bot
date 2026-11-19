import { mkdtemp, rm } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { afterEach, expect, it, onTestFinished, vi } from 'vitest'
import type { AgentInput, TranscriptionToolOutput } from '$lib/anda/client/types'
import { DesktopStore } from '../src/main/store'
import { DesktopClient } from '../src/renderer/client.svelte'
import type { Preferences } from '../src/shared/contract'

async function setup() {
  const directory = await mkdtemp(join(tmpdir(), 'anda-renderer-client-'))
  const path = join(directory, 'desktop.json')
  const store = new DesktopStore(path)
  const agentRun = vi.fn(async (_input: AgentInput, _submissionId?: string) => ({ content: '' }))
  const rpc = vi.fn(
    async (method: string, params: unknown[], submissionId?: string): Promise<unknown> => {
      if (method === 'register_workspace') return { workspace: params[0] }
      if (method === 'tool_call') return { output: { result: {} } }
      if (method === 'agent_run') return agentRun(params[0] as AgentInput, submissionId)
      throw new Error(`Unexpected RPC: ${method}`)
    }
  )
  const acknowledgeSubmission = vi.fn()
  vi.stubGlobal('window', {
    anda: {
      rpc,
      acknowledgeSubmission,
      preferences: (patch: Partial<Preferences>) => {
        Object.assign(store.state.preferences, patch)
        return store.save()
      }
    }
  })
  vi.stubGlobal('document', { hidden: false })
  const client = new DesktopClient()
  client.connection.connected = true
  onTestFinished(async () => {
    client.dispose()
    await rm(directory, { recursive: true, force: true })
  })
  return { client, rpc, agentRun, acknowledgeSubmission, path }
}

afterEach(() => vi.unstubAllGlobals())

it('sends a delayed first voice transcript to its original chat and workspace', async () => {
  const { client, rpc, agentRun, acknowledgeSubmission, path } = await setup()
  const other = {
    source: 'desktop:other',
    workspace: '/other',
    title: 'Other',
    updatedAt: 20,
    readAt: 10
  }
  await client.savePreferences({ chats: [other], drafts: { [other.source]: 'other draft' } })
  client.newChat('/original')
  const original = client.activeSource
  client.voice.capabilities.transcription = ['wav']
  const transcription = Promise.withResolvers<{ output: TranscriptionToolOutput }>()
  rpc.mockReturnValueOnce(transcription.promise)
  const submitted = Promise.withResolvers<{ content: string }>()
  agentRun.mockReturnValueOnce(submitted.promise)

  const sending = client.sendVoiceTurn({
    fileName: 'voice.wav',
    audioBase64: 'AQID',
    ttsEnabled: false
  })
  await vi.waitFor(() =>
    expect(rpc).toHaveBeenCalledWith('tool_call', [
      expect.objectContaining({ name: 'transcribe_audio' })
    ])
  )
  await client.switchChannel(other.source)
  transcription.resolve({
    output: { text: 'voice message', file_name: 'voice.wav', provider: 'test' }
  })
  await vi.waitFor(() => expect(agentRun).toHaveBeenCalledOnce())
  const foregroundSending = client.sending
  const originalSending = client.channels.get(original)?.sending
  submitted.resolve({ content: '' })
  await sending

  expect(agentRun).toHaveBeenCalledExactlyOnceWith(
    expect.objectContaining({
      prompt: 'voice message',
      meta: expect.objectContaining({ source: original, workspace: '/original' })
    }),
    expect.any(String)
  )
  expect(foregroundSending).toBe(false)
  expect(originalSending).toBe(true)
  expect(acknowledgeSubmission).toHaveBeenCalledExactlyOnceWith(agentRun.mock.calls[0]![1])
  const reopened = new DesktopStore(path)
  await reopened.load()
  expect(client.activeSource).toBe(other.source)
  expect(reopened.state.preferences).toMatchObject({
    activeSource: other.source,
    chats: [other, { source: original, workspace: '/original', title: 'voice message' }],
    drafts: { [other.source]: 'other draft' }
  })
})

it.each(['success', 'failure'])(
  'keeps the latest chat after a late folder registration (%s)',
  async (outcome) => {
    const { client, rpc, path } = await setup()
    const registration = Promise.withResolvers<unknown>()
    rpc.mockReturnValueOnce(registration.promise)
    const folder = client.switchChannel('cli:/original')
    expect(rpc).toHaveBeenCalledWith('register_workspace', ['/original'])
    await client.switchChannel('desktop:latest')
    if (outcome === 'success') registration.resolve({ workspace: '/original' })
    else registration.reject(new Error('Folder is unavailable'))
    await folder

    const reopened = new DesktopStore(path)
    await reopened.load()
    expect(client.activeSource).toBe('desktop:latest')
    expect(reopened.state.preferences.activeSource).toBe('desktop:latest')
    expect(client.systemMessage).toBeNull()
  }
)

it('keeps a new unsent chat open when the previous folder finishes registering', async () => {
  const { client, rpc, path } = await setup()
  await client.switchChannel('desktop:saved')
  const registration = Promise.withResolvers<unknown>()
  rpc.mockReturnValueOnce(registration.promise)
  const folder = client.switchChannel('cli:/original')
  client.newChat('/next')
  const draftSource = client.activeSource
  registration.resolve({ workspace: '/original' })
  await folder

  const reopened = new DesktopStore(path)
  await reopened.load()
  expect(client.activeSource).toBe(draftSource)
  expect(client.workspace).toBe('/next')
  expect(reopened.state.preferences.activeSource).toBe('desktop:saved')
})
