import { describe, expect, it, vi } from 'vitest'
import { Channel, type API } from './channel.svelte'
import { PollConversation } from './poll-conversation'
import type { AgentOutput, ChatMessage, Conversation, ConversationDelta, ToolInput } from './types'

function message(id: string, text: string): ChatMessage {
  return {
    id,
    conversation: 1,
    role: 'assistant',
    text,
    timestamp: 1
  }
}

function usage() {
  return {
    input_tokens: 0,
    output_tokens: 0,
    cached_tokens: 0,
    requests: 0
  }
}

function conversation(id: number, overrides: Partial<Conversation> = {}): Conversation {
  return {
    _id: id,
    user: 'user:test',
    messages: [],
    artifacts: [],
    status: 'completed',
    usage: usage(),
    created_at: id,
    updated_at: id,
    ...overrides
  }
}

function conversationDelta(
  id: number,
  overrides: Partial<ConversationDelta> = {}
): ConversationDelta {
  return {
    _id: id,
    messages: [],
    artifacts: [],
    status: 'completed',
    usage: usage(),
    updated_at: id,
    ...overrides
  }
}

function toolResult<Result>(result: Result) {
  return {
    output: { result },
    usage: usage()
  }
}

function toolArgs(input: ToolInput): Record<string, unknown> {
  if (!input.args || typeof input.args !== 'object' || Array.isArray(input.args)) {
    return {}
  }
  return input.args as Record<string, unknown>
}

function createApi(
  options: {
    agentOutputs?: AgentOutput[]
    toolCall?: (input: ToolInput) => Promise<unknown> | unknown
    defaultAgentOutput?: AgentOutput
  } = {}
): API {
  const agentOutputs = [...(options.agentOutputs || [])]
  const fallbackAgentOutput = options.defaultAgentOutput || {
    content: '',
    usage: usage()
  }

  return {
    activeChannel() {
      return 'source:test'
    },
    requestExtra: vi.fn().mockResolvedValue({}),
    rpc: vi.fn().mockImplementation(async (method: string, tupleArgs: unknown[]) => {
      if (method === 'agent_run') {
        return agentOutputs.shift() || fallbackAgentOutput
      }

      if (method === 'tool_call' && options.toolCall) {
        return options.toolCall(tupleArgs[0] as ToolInput)
      }

      throw new Error(`Unexpected rpc method: ${method}`)
    }),
    updateStatus: vi.fn()
  }
}

describe('PollConversation', () => {
  it('delivers buffered messages before completing the iterator', async () => {
    const poller = new PollConversation()
    const iterator = poller[Symbol.asyncIterator]()
    const first = message('m-1', 'first')
    const second = message('m-2', 'second')

    poller.push(first, second)
    poller.finish()

    await expect(iterator.next()).resolves.toEqual({ value: first, done: false })
    await expect(iterator.next()).resolves.toEqual({ value: second, done: false })
    await expect(iterator.next()).resolves.toEqual({ value: null, done: true })
  })

  it('stops accepting new messages after the consumer closes the iterator', async () => {
    const poller = new PollConversation()
    const first = message('m-1', 'first')
    const second = message('m-2', 'second')
    const received: ChatMessage[] = []

    poller.push(first)

    for await (const current of poller) {
      received.push(current)
      break
    }

    poller.push(second)
    poller.finish()

    expect(received).toEqual([first])
    await expect(poller[Symbol.asyncIterator]().next()).resolves.toEqual({
      value: null,
      done: true
    })
  })
})

describe('Channel.sendPrompt', () => {
  it('finishes a one-shot poller when agent_run returns direct content', async () => {
    const api = createApi({
      defaultAgentOutput: {
        content: '',
        chat_history: [
          { role: 'user', content: [{ type: 'Text', text: 'hello' }] },
          { role: 'assistant', content: [{ type: 'Text', text: 'assistant reply' }] }
        ],
        usage: usage()
      }
    })
    const channel = new Channel('source:test', api)

    const poller = await channel.sendPrompt('hello', [])
    const received: ChatMessage[] = []

    for await (const current of poller!) {
      received.push(current)
    }

    expect(received.map((item) => item.text)).toEqual(['assistant reply'])
    expect(api.updateStatus).toHaveBeenLastCalledWith('completed', null)
  })

  it('returns an already finished poller when agent_run has no conversation and no content', async () => {
    const api = createApi({
      defaultAgentOutput: {
        content: '',
        usage: usage()
      }
    })
    const channel = new Channel('source:test', api)

    const poller = await channel.sendPrompt('hello', [])
    const received: ChatMessage[] = []

    for await (const current of poller!) {
      received.push(current)
    }

    expect(received).toEqual([])
    expect(api.updateStatus).toHaveBeenLastCalledWith('idle', null)
  })

  it('marks the request as failed when agent_run returns a failed reason without a conversation', async () => {
    const api = createApi({
      defaultAgentOutput: {
        content: '',
        failed_reason: 'permission denied',
        usage: usage()
      }
    })
    const channel = new Channel('source:test', api)

    const poller = await channel.sendPrompt('hello', [])
    const received: ChatMessage[] = []

    for await (const current of poller!) {
      received.push(current)
    }

    expect(received).toEqual([])
    expect(api.updateStatus).toHaveBeenLastCalledWith('failed', null)
  })

  it('updates previous-conversation state when a server conversation is created', async () => {
    const currentConversation = conversation(2, {
      ancestors: [1],
      updated_at: 20
    })
    const api = createApi({
      agentOutputs: [
        {
          content: '',
          conversation: currentConversation._id,
          usage: usage()
        }
      ],
      toolCall: async (input) => {
        const args = toolArgs(input)
        switch (args.type) {
          case 'GetConversation':
            return toolResult(currentConversation)
          case 'GetConversationDelta':
            return toolResult(
              conversationDelta(currentConversation._id, {
                updated_at: currentConversation.updated_at
              })
            )
          default:
            throw new Error(`Unexpected tool call: ${String(args.type)}`)
        }
      }
    })
    const channel = new Channel('source:test', api)

    const poller = await channel.sendPrompt('hello', [])

    for await (const _current of poller!) {
      // drain the async poll loop so the test observes the settled state.
    }

    expect(channel.hasPreviousConversations).toBe(true)
  })

  it('returns whether previous conversations were loaded', async () => {
    const currentConversation = conversation(2, {
      ancestors: [1],
      updated_at: 20
    })
    const previousConversation = conversation(1, {
      updated_at: 10
    })
    const api = createApi({
      agentOutputs: [
        {
          content: '',
          conversation: currentConversation._id,
          usage: usage()
        }
      ],
      toolCall: async (input) => {
        const args = toolArgs(input)
        switch (args.type) {
          case 'GetConversation':
            return toolResult(currentConversation)
          case 'GetConversationDelta':
            return toolResult(
              conversationDelta(currentConversation._id, {
                updated_at: currentConversation.updated_at
              })
            )
          case 'BatchGetConversations':
            expect(args.ids).toEqual([1])
            return toolResult([previousConversation])
          default:
            throw new Error(`Unexpected tool call: ${String(args.type)}`)
        }
      }
    })
    const channel = new Channel('source:test', api)

    const poller = await channel.sendPrompt('hello', [])

    for await (const _current of poller!) {
      // drain the async poll loop so the test observes the settled state.
    }

    await expect(channel.loadPreviousConversations()).resolves.toBe(true)
    expect(channel.messageGroups.map((group) => group._id)).toContain(1)
  })
})
