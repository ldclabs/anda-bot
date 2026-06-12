import { describe, expect, it, vi } from 'vitest'

import {
  Channel,
  knownServerMessageCount,
  mergePendingLocalMessages,
  preserveMessageTimestamps,
  type API
} from './channel.svelte'
import type {
  AgentOutput,
  ChatMessage,
  Conversation,
  ConversationDelta,
  MessageGroup,
  SourceState
} from './types'
import { SubmitMessageConversationId } from './types'

function chatMessage(overrides: Partial<ChatMessage> = {}): ChatMessage {
  return {
    id: 'm-test',
    conversation: 5,
    role: 'user',
    text: 'hello',
    timestamp: 1000,
    ...overrides
  }
}

function messageGroup(overrides: Partial<MessageGroup> = {}): MessageGroup {
  return {
    _id: 5,
    status: 'idle',
    ancestors: [],
    messages: [],
    createdAt: 1000,
    updatedAt: 1000,
    current: true,
    ...overrides
  }
}

function conversation(overrides: Partial<Conversation> = {}): Conversation {
  return {
    _id: 5,
    user: 'user-1',
    status: 'idle',
    usage: { input_tokens: 0, output_tokens: 0, cached_tokens: 0, requests: 1 },
    messages: [
      { role: 'user', content: [{ type: 'Text', text: '继续' }], timestamp: 900 },
      { role: 'assistant', content: [{ type: 'Text', text: '好的，已继续。' }], timestamp: 950 }
    ],
    created_at: 800,
    updated_at: 1000,
    ...overrides
  }
}

interface MockBackend {
  api: API
  rpcCalls: Array<{ method: string; type?: string }>
  statusUpdates: string[]
  agentRun: (input: { prompt: string }) => Promise<AgentOutput>
  conversations: Map<number, Conversation>
  sourceState: SourceState | null
  // Consumed one per GetConversationDelta call; falls back to an empty delta
  // derived from `conversations` once drained.
  deltaQueue: ConversationDelta[]
}

function createBackend(options: {
  conversations?: Conversation[]
  sourceState?: SourceState | null
  agentRun?: (input: { prompt: string }) => Promise<AgentOutput>
}): MockBackend {
  const backend: MockBackend = {
    rpcCalls: [],
    statusUpdates: [],
    conversations: new Map((options.conversations || []).map((conv) => [conv._id, conv])),
    sourceState: options.sourceState ?? null,
    agentRun:
      options.agentRun ||
      (async () => {
        throw new Error('agent_run not mocked')
      }),
    deltaQueue: [],
    api: null as unknown as API
  }

  backend.api = {
    activeChannel: () => 'browser:test',
    requestExtra: async () => ({ conversation: 0 }),
    updateStatus: (status) => {
      backend.statusUpdates.push(status)
    },
    rpc: async <Result>(method: string, tupleArgs: unknown[]): Promise<Result> => {
      const input = (tupleArgs[0] || {}) as {
        prompt?: string
        args?: { type?: string; _id?: number }
      }
      backend.rpcCalls.push({ method, type: input.args?.type })
      if (method === 'agent_run') {
        return (await backend.agentRun(input as { prompt: string })) as Result
      }
      if (method === 'tool_call') {
        switch (input.args?.type) {
          case 'GetSourceState':
            return { output: { result: backend.sourceState || {} }, usage: {} } as Result
          case 'GetConversation': {
            const conv = backend.conversations.get(input.args._id || 0)
            if (!conv) {
              return { output: { error: `conversation not found: ${input.args._id}` } } as Result
            }
            return { output: { result: conv }, usage: {} } as Result
          }
          case 'GetConversationDelta': {
            const queued = backend.deltaQueue.shift()
            if (queued) {
              return { output: { result: queued }, usage: {} } as Result
            }
            const conv = backend.conversations.get(input.args._id || 0)
            const delta: ConversationDelta = {
              _id: input.args._id || 0,
              messages: [],
              artifacts: [],
              status: conv?.status || 'completed',
              usage: conv?.usage || {
                input_tokens: 0,
                output_tokens: 0,
                cached_tokens: 0,
                requests: 1
              },
              updated_at: conv?.updated_at || 0
            }
            return { output: { result: delta }, usage: {} } as Result
          }
          default:
            return { output: { error: `unexpected tool call: ${input.args?.type}` } } as Result
        }
      }
      throw new Error(`unexpected rpc: ${method}`)
    }
  }

  return backend
}

describe('mergePendingLocalMessages', () => {
  it('keeps a fresh optimistic duplicate instead of merging it into old history', () => {
    const group = messageGroup({
      messages: [
        chatMessage({ id: 'm-5-0', text: '继续' }),
        chatMessage({ id: 'm-5-1', role: 'assistant', text: '好的，已继续。' })
      ]
    })
    const local = chatMessage({ id: 'm-5-1700-1', text: '继续', pending: true, timestamp: 1700 })
    const existing = messageGroup({
      messages: [...group.messages, local]
    })

    mergePendingLocalMessages(group, [existing], knownServerMessageCount(existing, group))

    const last = group.messages[group.messages.length - 1]
    expect(group.messages).toHaveLength(3)
    expect(last?.id).toBe('m-5-1700-1')
    expect(last?.pending).toBe(true)
  })

  it('merges the optimistic message with a server message that arrives later', () => {
    const local = chatMessage({ id: 'm-5-1700-1', text: '继续', pending: true, timestamp: 1700 })
    const existing = messageGroup({
      messages: [chatMessage({ id: 'm-5-0', text: '继续' }), local]
    })
    const group = messageGroup({
      messages: [
        chatMessage({ id: 'm-5-0', text: '继续' }),
        chatMessage({ id: 'm-5-1', text: '继续', timestamp: 1800 })
      ]
    })

    mergePendingLocalMessages(group, [existing], knownServerMessageCount(existing, group))

    expect(group.messages).toHaveLength(2)
    expect(group.messages[1]?.pending).toBeUndefined()
  })

  it('still dedupes steer commands by their full prompt text', () => {
    const local = chatMessage({
      id: 'm-5-1700-1',
      text: '/steer focus on tests',
      pending: true
    })
    const existing = messageGroup({ messages: [chatMessage({ id: 'm-5-0' }), local] })
    const group = messageGroup({
      messages: [
        chatMessage({ id: 'm-5-0' }),
        chatMessage({ id: 'm-5-1', text: '/steer focus on tests' })
      ]
    })

    mergePendingLocalMessages(group, [existing], knownServerMessageCount(existing, group))

    expect(group.messages).toHaveLength(2)
    expect(group.messages.filter((message) => message.pending)).toHaveLength(0)
  })
})

describe('preserveMessageTimestamps', () => {
  it('keeps first-seen timestamps for unchanged message ids', () => {
    const previous = messageGroup({
      messages: [chatMessage({ id: 'm-5-0', timestamp: 1000 })]
    })
    const group = messageGroup({
      messages: [
        chatMessage({ id: 'm-5-0', timestamp: 2000 }),
        chatMessage({ id: 'm-5-1', timestamp: 2000 })
      ]
    })

    preserveMessageTimestamps(group, previous)

    expect(group.messages[0]?.timestamp).toBe(1000)
    expect(group.messages[1]?.timestamp).toBe(2000)
  })
})

describe('Channel.sendPrompt', () => {
  it('keeps a duplicate follow-up visible in an idle conversation', async () => {
    const backend = createBackend({
      conversations: [conversation()],
      sourceState: { conv_id: 5, status: 'idle', timestamp: 1000 },
      agentRun: async () => ({
        content: '',
        usage: { input_tokens: 0, output_tokens: 0, cached_tokens: 0, requests: 1 },
        conversation: 5,
        session: 'sess-1'
      })
    })
    const channel = new Channel('browser:test', backend.api)
    await channel.init()

    try {
      await channel.sendPrompt('继续', [])

      const group = channel.messageGroups.find((existing) => existing._id === 5)
      const last = group?.messages[group.messages.length - 1]
      expect(last?.text).toBe('继续')
      expect(last?.role).toBe('user')
      expect(last?.pending).toBe(true)
      // The old history copy is still in place.
      expect(group?.messages.filter((message) => message.text === '继续')).toHaveLength(2)
    } finally {
      channel.destroy()
    }
  })

  it('does not resurrect the old conversation after a bare /new', async () => {
    const backend = createBackend({
      conversations: [conversation()],
      sourceState: { conv_id: 5, status: 'idle', timestamp: 1000 },
      agentRun: async () => ({
        content: '',
        usage: { input_tokens: 0, output_tokens: 0, cached_tokens: 0, requests: 1 },
        // The daemon answers a bare /new with the detached conversation id.
        conversation: 5
      })
    })
    const channel = new Channel('browser:test', backend.api)
    await channel.init()
    expect(channel.messageGroups).toHaveLength(1)

    try {
      await channel.sendPrompt('/new', [])

      expect(channel.messageGroups).toHaveLength(0)
      expect(channel.conversationId).toBe(0)
      expect(backend.statusUpdates[backend.statusUpdates.length - 1]).toBe('ready')
    } finally {
      channel.destroy()
    }
  })

  it('removes optimistic messages and rethrows when delivery fails', async () => {
    const backend = createBackend({
      agentRun: async () => {
        throw new Error('daemon unreachable')
      }
    })
    const channel = new Channel('browser:test', backend.api)

    await expect(channel.sendPrompt('hello there', [])).rejects.toThrow('daemon unreachable')

    expect(channel.messageGroups).toHaveLength(0)
    expect(backend.statusUpdates[backend.statusUpdates.length - 1]).toBe('request failed')
  })

  it('delivers follow-up turn output to the voice poller of an already polled conversation', async () => {
    const now = Date.now()
    const idleConversation = conversation({ updated_at: now, created_at: now - 1000 })
    const turnUsage = { input_tokens: 0, output_tokens: 0, cached_tokens: 0, requests: 1 }
    const backend = createBackend({
      conversations: [idleConversation],
      sourceState: { conv_id: 5, status: 'idle', timestamp: now },
      agentRun: async () => ({
        content: '',
        usage: turnUsage,
        conversation: 5,
        session: 'sess-1'
      })
    })
    backend.deltaQueue.push(
      // First tick right after init: nothing new yet.
      { _id: 5, messages: [], artifacts: [], status: 'idle', usage: turnUsage, updated_at: now },
      // Next tick: the follow-up turn's messages have landed.
      {
        _id: 5,
        messages: [
          { role: 'user', content: [{ type: 'Text', text: '继续' }], timestamp: now + 1 },
          {
            role: 'assistant',
            content: [{ type: 'Text', text: '收到，继续处理。' }],
            timestamp: now + 2
          }
        ],
        artifacts: [],
        status: 'idle',
        usage: turnUsage,
        updated_at: now + 2
      }
    )
    const channel = new Channel('browser:test', backend.api)
    await channel.init()

    // Keep the poll loop from sleeping so the test stays fast and deterministic.
    const wakeTimer = setInterval(() => channel.wakePolling(), 5)
    try {
      const poller = await channel.sendPrompt('继续', [])
      expect(poller).not.toBeNull()

      const spoken: string[] = []
      for await (const message of poller!) {
        spoken.push(message.text)
      }
      // Only the assistant reply is delivered (no user echo), and the poller
      // finishes at the turn boundary instead of waiting for conversation end.
      expect(spoken).toEqual(['收到，继续处理。'])
    } finally {
      clearInterval(wakeTimer)
      channel.destroy()
    }
  })

  it('places /steer follow-ups into the submit group when no conversation is active', async () => {
    let resolveRun: ((output: AgentOutput) => void) | null = null
    const backend = createBackend({
      agentRun: () =>
        new Promise<AgentOutput>((resolve) => {
          resolveRun = resolve
        })
    })
    const channel = new Channel('browser:test', backend.api)

    try {
      const pending = channel.sendPrompt('/steer focus on tests', [])
      const submitGroup = channel.messageGroups.find(
        (group) => group._id === SubmitMessageConversationId
      )
      expect(submitGroup?.messages[0]?.text).toBe('/steer focus on tests')
      expect(submitGroup?.messages[0]?.pending).toBe(true)

      await vi.waitFor(() => expect(resolveRun).toBeTruthy())
      resolveRun!({
        content: '',
        usage: { input_tokens: 0, output_tokens: 0, cached_tokens: 0, requests: 1 }
      })
      await pending
    } finally {
      channel.destroy()
    }
  })
})
