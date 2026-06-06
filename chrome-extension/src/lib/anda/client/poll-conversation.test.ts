import { describe, expect, it, vi } from 'vitest'
import { Channel, type API } from './channel.svelte'
import { PollConversation } from './poll-conversation'
import type {
  AgentInput,
  AgentOutput,
  ChatAttachment,
  ChatMessage,
  Conversation,
  ConversationDelta,
  Resource,
  ToolInput
} from './types'
import { SubmitMessageConversationId } from './types'

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

function rawMessage(role: 'user' | 'assistant', text: string, timestamp: number) {
  return {
    role,
    content: [{ type: 'Text' as const, text }],
    timestamp
  }
}

function rawMessageWithResource(
  role: 'user' | 'assistant',
  text: string,
  resource: Resource,
  timestamp: number
) {
  return {
    role,
    content: [
      { type: 'Text' as const, text },
      { type: 'Resource' as const, ...resource }
    ],
    timestamp
  }
}

function rawMessageParts(role: 'user' | 'assistant', texts: string[], timestamp: number) {
  return {
    role,
    content: texts.map((text) => ({ type: 'Text' as const, text })),
    timestamp
  }
}

function resource(overrides: Partial<Resource> = {}): Resource {
  return {
    _id: 7,
    tags: ['image', 'jpeg'],
    name: 'company&memory.jpeg',
    mime_type: 'image/jpeg',
    size: 143207,
    ...overrides
  }
}

function imageAttachment(): ChatAttachment {
  const localResource = resource({
    _id: 0,
    blob: 'local-image-bytes',
    metadata: { source: 'chrome_extension', last_modified: 1 }
  })
  return {
    id: 'company&memory.jpeg-143207-1',
    name: localResource.name,
    type: localResource.mime_type,
    size: localResource.size,
    resource: localResource
  }
}

function deferred<Result>() {
  let resolve!: (value: Result) => void
  let reject!: (reason?: unknown) => void
  const promise = new Promise<Result>((resolvePromise, rejectPromise) => {
    resolve = resolvePromise
    reject = rejectPromise
  })
  return { promise, resolve, reject }
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
    agentOutputs?: Array<AgentOutput | Promise<AgentOutput>>
    onAgentRun?: (input: AgentInput) => void
    requestExtra?: () => Promise<Record<string, unknown>> | Record<string, unknown>
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
    requestExtra: vi.fn().mockImplementation(options.requestExtra || (() => ({}))),
    rpc: vi.fn().mockImplementation(async (method: string, tupleArgs: unknown[]) => {
      if (method === 'agent_run') {
        options.onAgentRun?.(tupleArgs[0] as AgentInput)
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
  it('inserts an idle conversation prompt into the active conversation immediately', async () => {
    const prompt = 'continue from idle'
    const currentConversation = conversation(1, {
      messages: [rawMessage('user', 'hello', 10), rawMessage('assistant', 'waiting', 11)],
      status: 'idle',
      updated_at: 11
    })
    const agentRunStarted = deferred<void>()
    const agentRun = deferred<AgentOutput>()
    const api = createApi({
      agentOutputs: [agentRun.promise],
      onAgentRun: () => agentRunStarted.resolve(),
      toolCall: async (input) => {
        const args = toolArgs(input)
        switch (args.type) {
          case 'GetSourceState':
            return toolResult({ conv_id: currentConversation._id })
          case 'GetConversation':
            return toolResult(currentConversation)
          case 'GetConversationDelta':
            return toolResult(
              conversationDelta(currentConversation._id, {
                status: 'completed',
                updated_at: currentConversation.updated_at
              })
            )
          default:
            throw new Error(`Unexpected tool call: ${String(args.type)}`)
        }
      }
    })
    const channel = new Channel('source:test', api)
    await channel.init()

    const send = channel.sendPrompt(prompt, [])
    await agentRunStarted.promise

    try {
      expect(channel.messageGroups.map((group) => group._id)).toEqual([currentConversation._id])
      expect(channel.messageGroups[0]!.messages.map((item) => item.text)).toEqual([
        'hello',
        'waiting',
        prompt
      ])
      expect(channel.messageGroups[0]!.messages.at(-1)).toMatchObject({
        conversation: currentConversation._id,
        pending: true
      })
    } finally {
      agentRun.resolve({
        content: '',
        conversation: currentConversation._id,
        usage: usage()
      })
      const poller = await send
      for await (const _current of poller!) {
        // drain the async poll loop so the test observes the stale-refresh state.
      }
    }

    expect(channel.messageGroups.map((group) => group._id)).toEqual([currentConversation._id])
    expect(channel.messageGroups[0]!.messages.map((item) => item.text)).toEqual([
      'hello',
      'waiting',
      prompt
    ])
  })

  it('keeps an idle prompt visible when a stale channel sync finishes after send starts', async () => {
    let nowMs = 1_000_000_000_000
    const nowSpy = vi.spyOn(Date, 'now').mockImplementation(() => nowMs)
    const prompt = 'continue after switching back'
    const currentConversation = conversation(1, {
      messages: [rawMessage('user', 'hello', 10), rawMessage('assistant', 'waiting', 11)],
      status: 'idle',
      updated_at: 11
    })
    const secondFetchStarted = deferred<void>()
    const secondFetch = deferred<Conversation>()
    const agentRunStarted = deferred<void>()
    const agentRun = deferred<AgentOutput>()
    let getConversationCalls = 0
    const api = createApi({
      agentOutputs: [agentRun.promise],
      onAgentRun: () => agentRunStarted.resolve(),
      toolCall: async (input) => {
        const args = toolArgs(input)
        switch (args.type) {
          case 'GetSourceState':
            return toolResult({ conv_id: currentConversation._id })
          case 'GetConversation':
            getConversationCalls += 1
            if (getConversationCalls === 1) {
              return toolResult(currentConversation)
            }
            secondFetchStarted.resolve()
            return toolResult(await secondFetch.promise)
          default:
            throw new Error(`Unexpected tool call: ${String(args.type)}`)
        }
      }
    })
    const channel = new Channel('source:test', api)

    try {
      await channel.init()
      expect(channel.messageGroups[0]!.messages.map((item) => item.text)).toEqual([
        'hello',
        'waiting'
      ])

      nowMs += 61_000
      const sync = channel.init()
      await secondFetchStarted.promise

      const send = channel.sendPrompt(prompt, [])
      await agentRunStarted.promise
      expect(channel.messageGroups[0]!.messages.map((item) => item.text)).toEqual([
        'hello',
        'waiting',
        prompt
      ])

      secondFetch.resolve(currentConversation)
      await sync
      expect(channel.messageGroups[0]!.messages.map((item) => item.text)).toEqual([
        'hello',
        'waiting',
        prompt
      ])

      agentRun.resolve({ content: '', usage: usage() })
      await send
    } finally {
      nowSpy.mockRestore()
      agentRun.resolve({ content: '', usage: usage() })
      secondFetch.resolve(currentConversation)
    }
  })

  it('queues a plain prompt outside the conversation while the active conversation is working', async () => {
    const prompt = 'please adjust the approach'
    const currentConversation = conversation(1, {
      messages: [rawMessage('user', 'hello', 10), rawMessage('assistant', 'working', 11)],
      status: 'working',
      updated_at: 11
    })
    const finalConversation = conversation(1, {
      messages: [
        rawMessage('user', 'hello', 10),
        rawMessage('assistant', 'working', 11),
        rawMessage('user', prompt, 12)
      ],
      status: 'completed',
      updated_at: 13
    })
    const agentRunStarted = deferred<void>()
    const agentRun = deferred<AgentOutput>()
    let getConversationCalls = 0
    const api = createApi({
      agentOutputs: [agentRun.promise],
      onAgentRun: () => agentRunStarted.resolve(),
      toolCall: async (input) => {
        const args = toolArgs(input)
        switch (args.type) {
          case 'GetSourceState':
            return toolResult({ conv_id: currentConversation._id })
          case 'GetConversation':
            getConversationCalls += 1
            return toolResult(getConversationCalls < 3 ? currentConversation : finalConversation)
          case 'GetConversationDelta':
            return toolResult(
              conversationDelta(currentConversation._id, {
                messages: [rawMessage('user', prompt, 12)],
                status: 'completed',
                updated_at: finalConversation.updated_at
              })
            )
          default:
            throw new Error(`Unexpected tool call: ${String(args.type)}`)
        }
      }
    })
    const channel = new Channel('source:test', api)
    await channel.init()

    const send = channel.sendPrompt(prompt, [])
    await agentRunStarted.promise

    try {
      expect(channel.messageGroups.map((group) => group._id)).toEqual([currentConversation._id])
      expect(channel.messageGroups[0]!.messages.map((item) => item.text)).toEqual([
        'hello',
        'working'
      ])
      expect(channel.pendingFollowUps.map((item) => item.text)).toEqual([prompt])
    } catch (error) {
      agentRun.resolve({
        content: '',
        conversation: currentConversation._id,
        usage: usage()
      })
      const poller = await send
      for await (const _current of poller!) {
        // drain the async poll loop before surfacing the assertion failure.
      }
      throw error
    }

    agentRun.resolve({
      content: '',
      conversation: currentConversation._id,
      usage: usage()
    })
    const poller = await send
    for await (const _current of poller!) {
      // drain the async poll loop so the test observes the settled state.
    }

    expect(channel.pendingFollowUps).toEqual([])
    expect(channel.messageGroups[0]!.messages.map((item) => item.text)).toEqual([
      'hello',
      'working',
      prompt
    ])
  })

  it('cancels a queued follow-up before it leaves the frontend', async () => {
    const prompt = 'never send this follow-up'
    const currentConversation = conversation(1, {
      messages: [rawMessage('user', 'hello', 10), rawMessage('assistant', 'working', 11)],
      status: 'working',
      updated_at: 11
    })
    const requestExtraStarted = deferred<void>()
    const requestExtra = deferred<Record<string, unknown>>()
    let blockNextRequestExtra = false
    const agentInputs: AgentInput[] = []
    const api = createApi({
      onAgentRun: (input) => {
        agentInputs.push(input)
      },
      requestExtra: async () => {
        if (blockNextRequestExtra) {
          blockNextRequestExtra = false
          requestExtraStarted.resolve()
          return requestExtra.promise
        }
        return {}
      },
      toolCall: async (input) => {
        const args = toolArgs(input)
        switch (args.type) {
          case 'GetSourceState':
            return toolResult({ conv_id: currentConversation._id })
          case 'GetConversation':
            return toolResult(currentConversation)
          default:
            throw new Error(`Unexpected tool call: ${String(args.type)}`)
        }
      }
    })
    const channel = new Channel('source:test', api)
    await channel.init()

    blockNextRequestExtra = true
    const send = channel.sendPrompt(prompt, [])
    await requestExtraStarted.promise

    const pendingFollowUp = channel.pendingFollowUps[0]!
    expect(pendingFollowUp.text).toBe(prompt)
    expect(channel.cancelPendingFollowUp(pendingFollowUp.id)).toBe(true)
    expect(channel.pendingFollowUps).toEqual([])

    requestExtra.resolve({})
    const poller = await send
    for await (const _current of poller!) {
      // drain the cancelled poller so the test observes the final state.
    }

    expect(agentInputs).toEqual([])
    expect(channel.cancelPendingFollowUp(pendingFollowUp.id)).toBe(false)
    expect(channel.pendingFollowUps).toEqual([])
    expect(channel.messageGroups[0]!.messages.map((item) => item.text)).toEqual([
      'hello',
      'working'
    ])
  })

  it('clears a queued follow-up when it is accepted inside a combined user message', async () => {
    const firstPrompt = '是的，一个交互优化'
    const secondPrompt = '后面的提交变更就没必要跑测试了'
    const runtimeText =
      '[$system: kind="background shell"]\nThis message is from the Anda runtime.\n\n"tool output"'
    const currentConversation = conversation(1, {
      messages: [rawMessage('user', 'hello', 10), rawMessage('assistant', 'working', 11)],
      status: 'working',
      updated_at: 11
    })
    const finalConversation = conversation(1, {
      messages: [
        rawMessage('user', 'hello', 10),
        rawMessage('assistant', 'working', 11),
        rawMessageParts('user', [firstPrompt, runtimeText, secondPrompt], 12)
      ],
      status: 'completed',
      updated_at: 13
    })
    const agentRunStarted = deferred<void>()
    const agentRun = deferred<AgentOutput>()
    let getConversationCalls = 0
    const api = createApi({
      agentOutputs: [agentRun.promise],
      onAgentRun: () => agentRunStarted.resolve(),
      toolCall: async (input) => {
        const args = toolArgs(input)
        switch (args.type) {
          case 'GetSourceState':
            return toolResult({ conv_id: currentConversation._id })
          case 'GetConversation':
            getConversationCalls += 1
            return toolResult(getConversationCalls < 2 ? currentConversation : finalConversation)
          case 'GetConversationDelta':
            return toolResult(
              conversationDelta(currentConversation._id, {
                messages: [rawMessageParts('user', [firstPrompt, runtimeText, secondPrompt], 12)],
                status: 'completed',
                updated_at: finalConversation.updated_at
              })
            )
          default:
            throw new Error(`Unexpected tool call: ${String(args.type)}`)
        }
      }
    })
    const channel = new Channel('source:test', api)
    await channel.init()

    const send = channel.sendPrompt(firstPrompt, [])
    await agentRunStarted.promise

    expect(channel.pendingFollowUps.map((item) => item.text)).toEqual([firstPrompt])

    agentRun.resolve({
      content: '',
      conversation: currentConversation._id,
      usage: usage()
    })

    const poller = await send
    for await (const _current of poller!) {
      // drain the async poll loop.
    }

    expect(channel.pendingFollowUps).toEqual([])
    expect(channel.messageGroups[0]!.messages.at(-2)).toMatchObject({
      role: 'user',
      text: `${firstPrompt}\n\n${secondPrompt}`
    })
    expect(channel.messageGroups[0]!.messages.at(-1)).toMatchObject({
      role: 'tool',
      text: ''
    })
    expect(channel.messageGroups[0]!.messages.at(-1)?.thinkingText).toContain('background shell')
    expect(channel.messageGroups[0]!.messages.at(-1)?.thinkingText).toContain('tool output')
  })

  it.each(['/stop because it is wrong', '/cancel because it is wrong'])(
    'inserts %s into the active conversation while the request is pending',
    async (prompt) => {
      const currentConversation = conversation(1, {
        messages: [rawMessage('user', 'hello', 10), rawMessage('assistant', 'working', 11)],
        updated_at: 11
      })
      const agentRunStarted = deferred<void>()
      const agentRun = deferred<AgentOutput>()
      const api = createApi({
        agentOutputs: [agentRun.promise],
        onAgentRun: () => agentRunStarted.resolve(),
        toolCall: async (input) => {
          const args = toolArgs(input)
          switch (args.type) {
            case 'GetSourceState':
              return toolResult({ conv_id: currentConversation._id })
            case 'GetConversation':
              return toolResult(currentConversation)
            default:
              throw new Error(`Unexpected tool call: ${String(args.type)}`)
          }
        }
      })
      const channel = new Channel('source:test', api)
      await channel.init()

      const send = channel.sendPrompt(prompt, [])
      await agentRunStarted.promise

      expect(channel.messageGroups.map((group) => group._id)).toEqual([currentConversation._id])
      expect(channel.messageGroups[0]!.messages.map((item) => item.text)).toEqual([
        'hello',
        'working',
        prompt
      ])
      expect(channel.messageGroups[0]!.messages.at(-1)).toMatchObject({
        conversation: currentConversation._id,
        pending: true
      })
      expect(channel.messageGroups.map((group) => group._id)).not.toContain(
        SubmitMessageConversationId
      )

      agentRun.resolve({ content: '', usage: usage() })
      await send
    }
  )

  it('keeps a pending stop command in the active conversation after a stale refresh', async () => {
    const prompt = '/stop because it is wrong'
    const currentConversation = conversation(1, {
      messages: [rawMessage('user', 'hello', 10), rawMessage('assistant', 'working', 11)],
      updated_at: 11
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
          case 'GetSourceState':
            return toolResult({ conv_id: currentConversation._id })
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
    await channel.init()

    const poller = await channel.sendPrompt(prompt, [])
    for await (const _current of poller!) {
      // drain the async poll loop so the test observes the settled state.
    }

    expect(channel.messageGroups.map((group) => group._id)).toEqual([currentConversation._id])
    expect(channel.messageGroups[0]!.messages.map((item) => item.text)).toEqual([
      'hello',
      'working',
      prompt
    ])
  })

  it('keeps a steer command in the active conversation instead of queued follow-ups', async () => {
    const prompt = '/steer focus on the shorter fix'
    const currentConversation = conversation(1, {
      messages: [rawMessage('user', 'hello', 10), rawMessage('assistant', 'working', 11)],
      status: 'working',
      updated_at: 11
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
          case 'GetSourceState':
            return toolResult({ conv_id: currentConversation._id })
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
    await channel.init()

    const poller = await channel.sendPrompt(prompt, [])
    for await (const _current of poller!) {
      // drain the async poll loop so the test observes the settled state.
    }

    expect(channel.pendingFollowUps).toEqual([])
    expect(channel.sideMessages).toEqual([])
    expect(channel.messageGroups.map((group) => group._id)).toEqual([currentConversation._id])
    expect(channel.messageGroups[0]!.messages.map((item) => item.text)).toEqual([
      'hello',
      'working',
      prompt
    ])
  })

  it('sends a steer command immediately while another send is still in progress', async () => {
    const currentConversation = conversation(1, {
      messages: [rawMessage('user', 'hello', 10), rawMessage('assistant', 'waiting', 11)],
      status: 'idle',
      updated_at: 11
    })
    const firstAgentRunStarted = deferred<void>()
    const firstAgentRun = deferred<AgentOutput>()
    const agentInputs: AgentInput[] = []
    const api = createApi({
      agentOutputs: [
        firstAgentRun.promise,
        {
          content: '',
          conversation: currentConversation._id,
          usage: usage()
        }
      ],
      onAgentRun: (input) => {
        agentInputs.push(input)
        if (agentInputs.length === 1) {
          firstAgentRunStarted.resolve()
        }
      },
      toolCall: async (input) => {
        const args = toolArgs(input)
        switch (args.type) {
          case 'GetSourceState':
            return toolResult({ conv_id: currentConversation._id })
          case 'GetConversation':
            return toolResult(currentConversation)
          case 'GetConversationDelta':
            return toolResult(
              conversationDelta(currentConversation._id, {
                status: 'completed',
                updated_at: currentConversation.updated_at
              })
            )
          default:
            throw new Error(`Unexpected tool call: ${String(args.type)}`)
        }
      }
    })
    const channel = new Channel('source:test', api)
    await channel.init()

    const firstSend = channel.sendPrompt('continue', [])
    await firstAgentRunStarted.promise

    const steerPoller = await channel.sendPrompt('/steer correct course', [])

    expect(agentInputs.map((input) => input.prompt)).toEqual(['continue', '/steer correct course'])
    expect(steerPoller).not.toBeNull()

    firstAgentRun.resolve({
      content: '',
      conversation: currentConversation._id,
      usage: usage()
    })
    await firstSend
    for await (const _current of steerPoller!) {
      // drain the steer poller before the test completes.
    }
  })

  it('starts a new session immediately and ignores stale output from the detached send', async () => {
    const currentConversation = conversation(1, {
      messages: [rawMessage('user', 'hello', 10), rawMessage('assistant', 'working', 11)],
      status: 'working',
      updated_at: 11
    })
    const firstAgentRunStarted = deferred<void>()
    const secondAgentRunStarted = deferred<void>()
    const firstAgentRun = deferred<AgentOutput>()
    const secondAgentRun = deferred<AgentOutput>()
    const agentInputs: AgentInput[] = []
    const api = createApi({
      agentOutputs: [firstAgentRun.promise, secondAgentRun.promise],
      onAgentRun: (input) => {
        agentInputs.push(input)
        if (agentInputs.length === 1) {
          firstAgentRunStarted.resolve()
        } else if (agentInputs.length === 2) {
          secondAgentRunStarted.resolve()
        }
      },
      toolCall: async (input) => {
        const args = toolArgs(input)
        switch (args.type) {
          case 'GetSourceState':
            return toolResult({ conv_id: currentConversation._id })
          case 'GetConversation':
            return toolResult(currentConversation)
          default:
            throw new Error(`Unexpected tool call: ${String(args.type)}`)
        }
      }
    })
    const channel = new Channel('source:test', api)
    await channel.init()

    const firstSend = channel.sendPrompt('queued follow-up', [])
    await firstAgentRunStarted.promise

    const newSend = channel.sendPrompt('/new fresh start', [])
    await secondAgentRunStarted.promise

    expect(agentInputs.map((input) => input.prompt)).toEqual([
      'queued follow-up',
      '/new fresh start'
    ])
    expect(channel.pendingFollowUps).toEqual([])
    expect(channel.messageGroups.map((group) => group._id)).toEqual([SubmitMessageConversationId])
    expect(channel.messageGroups[0]!.messages.map((item) => item.text)).toEqual([
      '/new fresh start'
    ])

    secondAgentRun.resolve({
      content: '',
      usage: usage()
    })
    const newPoller = await newSend
    for await (const _current of newPoller!) {
      // drain the new-session poller before resolving the detached send.
    }

    firstAgentRun.resolve({
      content: '',
      conversation: currentConversation._id,
      usage: usage()
    })
    const firstPoller = await firstSend
    for await (const _current of firstPoller!) {
      // drain the stale poller so the test observes the final state.
    }

    expect(channel.messageGroups.map((group) => group._id)).toEqual([SubmitMessageConversationId])
    expect(channel.messageGroups[0]!.messages.map((item) => item.text)).toEqual([
      '/new fresh start'
    ])
  })

  it('does not send an older prompt if /new invalidates it before agent_run starts', async () => {
    const currentConversation = conversation(1, {
      messages: [rawMessage('user', 'hello', 10), rawMessage('assistant', 'working', 11)],
      status: 'working',
      updated_at: 11
    })
    const oldRequestMetaStarted = deferred<void>()
    const oldRequestMeta = deferred<Record<string, unknown>>()
    let blockNextRequestExtra = false
    const agentInputs: AgentInput[] = []
    const api = createApi({
      onAgentRun: (input) => {
        agentInputs.push(input)
      },
      requestExtra: async () => {
        if (blockNextRequestExtra) {
          blockNextRequestExtra = false
          oldRequestMetaStarted.resolve()
          return oldRequestMeta.promise
        }
        return {}
      },
      toolCall: async (input) => {
        const args = toolArgs(input)
        switch (args.type) {
          case 'GetSourceState':
            return toolResult({ conv_id: currentConversation._id })
          case 'GetConversation':
            return toolResult(currentConversation)
          default:
            throw new Error(`Unexpected tool call: ${String(args.type)}`)
        }
      }
    })
    const channel = new Channel('source:test', api)
    await channel.init()

    blockNextRequestExtra = true
    const oldSend = channel.sendPrompt('late follow-up', [])
    await oldRequestMetaStarted.promise

    const newPoller = await channel.sendPrompt('/new fresh start', [])
    expect(agentInputs.map((input) => input.prompt)).toEqual(['/new fresh start'])

    oldRequestMeta.resolve({})
    const oldPoller = await oldSend
    for await (const _current of oldPoller!) {
      // drain the cancelled stale poller.
    }
    for await (const _current of newPoller!) {
      // drain the new-session poller.
    }

    expect(agentInputs.map((input) => input.prompt)).toEqual(['/new fresh start'])
    expect(channel.pendingFollowUps).toEqual([])
    expect(channel.messageGroups.map((group) => group._id)).toEqual([SubmitMessageConversationId])
    expect(channel.messageGroups[0]!.messages.map((item) => item.text)).toEqual([
      '/new fresh start'
    ])
  })

  it('keeps local attachments when the server user message is still text-only', async () => {
    const prompt = 'look at this image'
    const attachment = imageAttachment()
    const currentConversation = conversation(2, {
      messages: [rawMessage('user', prompt, 10)],
      status: 'completed',
      updated_at: 12
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
                status: currentConversation.status,
                updated_at: currentConversation.updated_at
              })
            )
          default:
            throw new Error(`Unexpected tool call: ${String(args.type)}`)
        }
      }
    })
    const channel = new Channel('source:test', api)

    const poller = await channel.sendPrompt(prompt, [attachment])
    for await (const _current of poller!) {
      // drain the async poll loop so the test observes the settled state.
    }

    const group = channel.messageGroups.find((item) => item._id === currentConversation._id)!
    expect(group.messages[0]).toMatchObject({
      text: prompt,
      pending: true,
      attachments: [
        {
          id: attachment.id,
          name: attachment.name,
          resource: { blob: 'local-image-bytes' }
        }
      ]
    })
  })

  it('merges completed backend resources with local attachment blobs', async () => {
    const prompt = 'look at this image'
    const attachment = imageAttachment()
    const interimConversation = conversation(2, {
      messages: [rawMessage('user', prompt, 10)],
      status: 'working',
      updated_at: 10
    })
    const finalResource = resource({
      _id: 7,
      hash: 'image-hash',
      description: '[$system: kind=image_understanding]\n\nImage understanding result'
    })
    const finalConversation = conversation(2, {
      messages: [
        rawMessageWithResource('user', prompt, finalResource, 10),
        rawMessage('assistant', 'done', 13)
      ],
      status: 'completed',
      updated_at: 14
    })
    let getConversationCalls = 0
    const api = createApi({
      agentOutputs: [
        {
          content: '',
          conversation: interimConversation._id,
          usage: usage()
        }
      ],
      toolCall: async (input) => {
        const args = toolArgs(input)
        switch (args.type) {
          case 'GetConversation':
            getConversationCalls += 1
            return toolResult(getConversationCalls === 1 ? interimConversation : finalConversation)
          case 'GetConversationDelta':
            return toolResult(
              conversationDelta(interimConversation._id, {
                messages: [rawMessage('assistant', 'done', 13)],
                status: 'completed',
                updated_at: finalConversation.updated_at
              })
            )
          default:
            throw new Error(`Unexpected tool call: ${String(args.type)}`)
        }
      }
    })
    const channel = new Channel('source:test', api)

    const poller = await channel.sendPrompt(prompt, [attachment])
    for await (const _current of poller!) {
      // drain the async poll loop so the test observes the settled state.
    }

    const group = channel.messageGroups.find((item) => item._id === finalConversation._id)!
    expect(getConversationCalls).toBeGreaterThanOrEqual(2)
    expect(group.messages.map((item) => item.text)).toEqual([prompt, 'done'])
    expect(group.messages[0]).toMatchObject({
      pending: undefined,
      attachments: [
        {
          id: 'resource-7',
          name: 'company&memory.jpeg',
          type: 'image/jpeg',
          resource: {
            _id: 7,
            hash: 'image-hash',
            description: finalResource.description,
            blob: 'local-image-bytes'
          }
        }
      ]
    })
  })

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
