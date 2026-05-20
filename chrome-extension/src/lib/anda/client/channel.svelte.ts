import {
  errorToError,
  errorToMessage,
  isTransientWebSocketError
} from '$lib/service-worker/settings'
import { delay } from '$lib/utils/helper'
import { parsePromptCommand } from './commands'
import { conversationToGroup, normalizeMessage } from './conversations'
import { PollConversation } from './poll-conversation'
import type {
  AgentInput,
  AgentOutput,
  ChatAttachment,
  ChatMessage,
  Conversation,
  ConversationDelta,
  MessageGroup,
  RequestMeta,
  RpcOutput,
  SourceState,
  ToolInput,
  ToolOutput
} from './types'
import { SubmitMessageConversationId } from './types'

const pollingIntervalMs = 3000

export interface API {
  activeChannel(): string | null
  requestExtra(): Promise<Record<string, unknown>>
  rpc<Result>(method: string, tupleArgs: unknown[]): Promise<Result>
  updateStatus(status: string, message: { kind: 'info' | 'error'; text: string } | null): void
}

export class Channel extends EventTarget {
  readonly source: string // client-side channel ID.
  // latest server-side session ID. A client-side channel will include one or more server-side sessions, and a conversation belongs to only one session.
  #session: string = $state('')
  #conversation: Conversation | null = $state(null)
  #messageGroups: MessageGroup[] = $state([])
  #sideMessages: ChatMessage[] = $state([])
  #sending: boolean = $state(false)
  #loadingPrevious: boolean = $state(false)
  #pollingConversation: number = $state(0)
  #conversationAncestors: number[] = $state([])
  #syncing: boolean = $state(false)
  #syncAt: number = 0
  #api: API

  constructor(source: string, api: API) {
    super()

    this.source = source
    this.#api = api
  }

  get hasPreviousConversations(): boolean {
    return this.#conversationAncestors.length > 0
  }

  get loadingPrevious(): boolean {
    return this.#loadingPrevious
  }

  get syncing(): boolean {
    return this.#syncing
  }

  get sending(): boolean {
    return this.#sending
  }

  get status(): string {
    if (this.#sending) {
      return 'sending'
    }
    if (this.#syncing) {
      return 'syncing'
    }

    const currentGroup = this.#messageGroups.find((group) => group.current)
    const lastGroup = this.#messageGroups[this.#messageGroups.length - 1]
    return this.#conversation?.status || currentGroup?.status || lastGroup?.status || 'ready'
  }

  get conversationId(): number {
    return this.#conversation?._id || 0
  }

  get latestActivityAt(): number {
    let latest = this.#conversation?.updated_at || 0
    for (const group of this.#messageGroups) {
      latest = Math.max(latest, group.updatedAt || group.createdAt || 0)
    }
    for (const message of this.#sideMessages) {
      latest = Math.max(latest, message.timestamp || 0)
    }
    return latest
  }

  get messageCount(): number {
    return this.#messageGroups.reduce((count, group) => count + group.messages.length, 0)
  }

  get messageGroups(): MessageGroup[] {
    return [...this.#messageGroups]
  }

  get sideMessages(): ChatMessage[] {
    return this.#sideMessages
  }

  destroy(): void {
    this.clearConversationDisplay()
  }

  clearConversation(): void {
    this.clearConversationDisplay()
  }

  async init(): Promise<void> {
    if (this.#syncing) {
      return
    }
    const nowMs = Date.now()
    if (nowMs - this.#syncAt < 60000) {
      return
    }

    this.#syncAt = nowMs
    this.#syncing = true
    try {
      const {
        output: { result: state }
      } = await this.toolCall<RpcOutput<SourceState>>({
        name: 'conversations_api',
        args: { type: 'GetSourceState' }
      })
      const sourceConversationId = state.c || state.conv_id || 0
      if (!sourceConversationId) {
        return
      }

      const conversations = await this.fetchConversationChain(sourceConversationId)
      if (conversations.length === 0) {
        return
      }

      const latest = conversations[conversations.length - 1]
      this.updateConversationChain(conversations)
      this.updateLatestConversation(latest)
      if (
        (latest.status === 'working' ||
          latest.status === 'submitted' ||
          latest.status === 'idle') &&
        latest.updated_at > Date.now() - 7 * 24 * 3600 * 1000
      ) {
        this.pollConversationLoop(new PollConversation())
      }
      this.dispatchEvent(new CustomEvent('ChannelInitialized', { detail: { source: this.source } }))
    } catch (error) {
      this.#api.updateStatus('restore failed', { kind: 'error', text: errorToMessage(error) })
    } finally {
      this.#syncing = false
    }
  }

  async sendPrompt(
    prompt: string,
    attachments: ChatAttachment[]
  ): Promise<PollConversation | null> {
    if (this.#sending || (!prompt && attachments.length === 0)) {
      return null
    }

    this.#sending = true
    const resources = attachments.map((attachment) => attachment.resource)
    const command = parsePromptCommand(prompt)

    try {
      const meta = await this.requestMeta()
      const poller = new PollConversation()
      if (command && command.kind === 'new') {
        this.clearConversationDisplay()
        if (command.prompt) {
          this.appendLocalMessage({
            role: 'user',
            text: command.prompt,
            conversation: SubmitMessageConversationId
          })
        }
      } else if (command && command.kind === 'side') {
        if (!command.prompt) {
          return null
        }

        const timestamp = Date.now()
        this.#sideMessages = [
          ...this.#sideMessages,
          {
            id: `m-side-${timestamp}`,
            role: 'user',
            text: command.prompt,
            conversation: SubmitMessageConversationId,
            timestamp
          }
        ]
      } else {
        this.appendLocalMessage({
          role: 'user',
          text: prompt,
          conversation: SubmitMessageConversationId
        })
      }

      this.#api.updateStatus('sending', null)

      const output = await this.agentRun({ name: '', prompt, resources, meta })
      this.#session = output.session || ''
      if (output.conversation) {
        const conversation = await this.fetchConversation(output.conversation)
        this.updateLatestConversation(conversation)

        this.pollConversationLoop(poller)
      }

      if (output.failed_reason) {
        this.appendSystemMessage(output.failed_reason)
        this.#api.updateStatus('failed', null)
        poller.finish()
      } else if (output.chat_history && output.chat_history.length > 0) {
        // side messages
        const timestamp = Date.now()
        const messages = output.chat_history
          .map((message, index) =>
            normalizeMessage(message, {
              conversation: 0,
              index,
              fallbackTimestamp: timestamp
            })
          )
          .filter((message) => !!message)

        const sideMessages = []
        for (const msg of this.#sideMessages) {
          if (
            msg.id.startsWith('m-side-') &&
            messages.some((m) => m.text.trim() === msg.text.trim() && m.role === msg.role)
          ) {
            continue
          }
          sideMessages.push(msg)
        }
        this.#sideMessages = [...sideMessages, ...messages]
        poller.push(...messages.filter((message) => message.role === 'assistant'))
        this.#api.updateStatus('completed', null)
        poller.finish()
      } else {
        this.#api.updateStatus('idle', null)
        poller.finish()
      }

      return poller
    } catch (error) {
      this.#api.updateStatus('request failed', { kind: 'error', text: errorToMessage(error) })
      return null
    } finally {
      this.#sending = false
    }
  }

  private async pollConversationLoop(poller: PollConversation): Promise<void> {
    const conversation = this.#conversation ? { ...this.#conversation } : null
    if (!conversation || this.#pollingConversation === conversation._id) {
      poller.finish()
      return
    }

    this.#pollingConversation = conversation._id
    while (this.#pollingConversation === conversation._id) {
      const shouldContinue = await this.pollConversationOnce(conversation, poller)
      if (!shouldContinue) {
        break
      }
      const ms =
        this.#api.activeChannel() === this.source ? pollingIntervalMs : pollingIntervalMs * 10
      await delay(ms)
    }

    poller.finish()
  }

  private async pollConversationOnce(
    conversation: Conversation,
    poller: PollConversation
  ): Promise<boolean> {
    try {
      const {
        output: { result }
      } = await this.toolCall<RpcOutput<ConversationDelta>>({
        name: 'conversations_api',
        args: {
          type: 'GetConversationDelta',
          _id: conversation._id,
          messages_offset: conversation.messages?.length || 0,
          artifacts_offset: conversation.artifacts?.length || 0
        }
      })

      conversation.messages = [...(conversation.messages || []), ...result.messages]
      conversation.artifacts = [...(conversation.artifacts || []), ...result.artifacts]
      conversation.status = result.status
      conversation.usage = result.usage
      conversation.failed_reason = result.failed_reason
      conversation.updated_at = result.updated_at
      conversation.child = result.child

      this.updateLatestConversation({ ...conversation })
      if (result.messages.length > 0) {
        const start = conversation.messages!.length - result.messages.length || 0
        poller.push(
          ...result.messages
            .map((message, index) =>
              normalizeMessage(message, {
                conversation: conversation._id,
                index: start + index,
                fallbackTimestamp: conversation.updated_at
              })
            )
            .filter((message) => !!message)
        )
        poller.drain()
      }

      if (
        conversation.status === 'completed' ||
        conversation.status === 'cancelled' ||
        conversation.status === 'failed'
      ) {
        return false
      }
    } catch (error) {
      if (isTransientWebSocketError(error)) {
        this.#api.updateStatus('reconnecting', null)
        return false
      }

      this.#api.updateStatus('poll failed', { kind: 'error', text: errorToMessage(error) })
      return false
    }

    return true
  }

  private async requestMeta(): Promise<RequestMeta> {
    const extra = await this.#api.requestExtra()
    extra.source = this.source
    extra.conversation = this.#conversation?._id || 0
    return extra as RequestMeta
  }

  private async fetchConversation(conversationId: number): Promise<Conversation> {
    const {
      output: { result }
    } = await this.toolCall<RpcOutput<Conversation>>({
      name: 'conversations_api',
      args: { type: 'GetConversation', _id: conversationId }
    })
    return result
  }

  private async fetchConversationChain(conversationId: number): Promise<Conversation[]> {
    const conversations: Conversation[] = []
    const seen = new Set<number>()
    let nextId: number = conversationId

    while (nextId > 0) {
      if (seen.has(nextId)) {
        break
      }
      seen.add(nextId)
      const conversation = await this.fetchConversation(nextId)
      conversations.push(conversation)
      nextId = conversation.child || 0
    }

    return conversations
  }

  async loadPreviousConversations(): Promise<boolean> {
    if (this.#loadingPrevious || this.#conversationAncestors.length === 0) {
      return false
    }

    this.#loadingPrevious = true
    try {
      const {
        output: { result }
      } = await this.toolCall<RpcOutput<Conversation[]>>({
        name: 'conversations_api',
        args: {
          type: 'BatchGetConversations',
          ids: this.#conversationAncestors
        }
      })
      this.updateConversationChain(result)
      return result.length > 0
    } catch (error) {
      this.#api.updateStatus('history failed', { kind: 'error', text: errorToMessage(error) })
      return false
    } finally {
      this.#loadingPrevious = false
    }
  }

  private async agentRun(input: AgentInput): Promise<AgentOutput> {
    const meta = await this.requestMeta()
    input.meta = { ...input.meta, ...meta }
    return this.#api.rpc<AgentOutput>('agent_run', [input])
  }

  private async toolCall<Result>(input: ToolInput): Promise<ToolOutput<Result>> {
    const meta = await this.requestMeta()
    input.meta = { ...input.meta, ...meta }
    const rt = await this.#api.rpc<ToolOutput<Result>>('tool_call', [input])
    const error = (rt.output as any).error
    if (error != null) {
      throw errorToError(error)
    }
    return rt
  }

  private updateLatestConversation(conversation: Conversation): void {
    this.#conversation = conversation
    this.#conversationAncestors = conversation.ancestors || []
    this.#api.updateStatus(conversation.status, null)
    const group = conversationToGroup(conversation)
    const submitGroup = this.#messageGroups.find(
      (existing) => existing._id === SubmitMessageConversationId
    )

    const idx = this.#messageGroups.findIndex((existing) => existing._id >= conversation._id)
    if (idx >= 0) {
      this.#messageGroups.length = idx
    }

    group.current = true
    this.#messageGroups.push(group)
    if (submitGroup) {
      this.#messageGroups.push(submitGroup)
      this.removeSubmittedMessage((msg) =>
        group.messages.some((m) => m.text.trim() === msg.text.trim() && m.role === msg.role)
      )
    }
  }

  private updateConversationChain(conversations: Conversation[]): void {
    if (!conversations.length) {
      return
    }

    const existing = this.#messageGroups
    const incoming = conversations.map(conversationToGroup)
    let i = 0
    let j = 0
    const merged: MessageGroup[] = []

    while (i < existing.length && j < incoming.length) {
      const a = existing[i]!
      const b = incoming[j]!

      if (a._id === b._id) {
        // Replace existing with incoming when IDs match
        merged.push(b)
        i++
        j++
      } else if (a._id < b._id) {
        merged.push(a)
        i++
      } else {
        merged.push(b)
        j++
      }
    }

    while (i < existing.length) merged.push(existing[i++]!)
    while (j < incoming.length) {
      const conv = incoming[j++]!
      merged.push(conv)
    }

    this.#conversationAncestors = merged[0]?.ancestors || []
    this.#messageGroups = merged
  }

  private clearConversationDisplay(): void {
    this.#session = ''
    this.#conversation = null
    this.#messageGroups = []
    this.#sending = false
    this.#loadingPrevious = false
    this.#pollingConversation = 0
    this.#syncing = false
    this.#syncAt = 0
    this.#conversationAncestors = []
  }

  private appendSystemMessage(text: string): void {
    this.appendLocalMessage({
      role: 'system',
      text,
      conversation: this.#conversation?._id || SubmitMessageConversationId
    })
  }

  private appendLocalMessage(message: Omit<ChatMessage, 'id'>): void {
    this.updateMessageGroupWith(
      message.conversation || this.#conversation?._id || SubmitMessageConversationId,
      (group) => {
        const timestamp = Date.now()
        group.messages = [
          ...group.messages,
          {
            ...message,
            id: `m-${group._id}-${timestamp}`,
            conversation: group._id,
            timestamp
          }
        ]
        return { ...group }
      }
    )
  }

  private removeSubmittedMessage(isSubmitted: (msg: ChatMessage) => boolean): void {
    this.updateMessageGroupWith(SubmitMessageConversationId, (group) => {
      group.messages = group.messages.filter((message) => !isSubmitted(message))
      return { ...group }
    })
  }

  private updateMessageGroupWith(_id: number, fn: (group: MessageGroup) => MessageGroup) {
    const idx = this.#messageGroups.findIndex((group) => group._id === _id)
    if (idx >= 0) {
      const updated = fn(this.#messageGroups[idx]!)
      this.#messageGroups[idx] = updated
    } else {
      const nowMs = Date.now()
      const group = fn({
        _id: SubmitMessageConversationId,
        status: 'submitted',
        ancestors: [],
        createdAt: nowMs,
        updatedAt: nowMs,
        messages: [],
        current: false
      })
      this.#messageGroups = [...this.#messageGroups, group]
    }
  }
}
