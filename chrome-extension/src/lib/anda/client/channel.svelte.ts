import {
  errorToError,
  errorToMessage,
  isTransientWebSocketError
} from '$lib/service-worker/settings'
import { delay } from '$lib/utils/helper'
import { isImmediatePromptCommand, parsePromptCommand } from './commands'
import { conversationToGroup, normalizeMessages } from './conversations'
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
  #pendingFollowUps: ChatMessage[] = $state([])
  #sending: boolean = $state(false)
  #loadingPrevious: boolean = $state(false)
  #pollingConversation: number = $state(0)
  #conversationAncestors: number[] = $state([])
  #syncing: boolean = $state(false)
  #syncAt: number = 0
  #localMessageSeq: number = 0
  #sendEpoch: number = 0
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
    for (const message of this.#pendingFollowUps) {
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

  get pendingFollowUps(): ChatMessage[] {
    return this.#pendingFollowUps
  }

  cancelPendingFollowUp(id: string): boolean {
    if (!this.hasPendingFollowUp(id)) {
      return false
    }
    this.removePendingFollowUp(id)
    return true
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
    const command = parsePromptCommand(prompt)
    const immediate = isImmediatePromptCommand(command)
    if ((this.#sending && !immediate) || (!prompt && attachments.length === 0)) {
      return null
    }

    const ownsSendingFlag = !this.#sending || command?.kind === 'new'
    if (ownsSendingFlag) {
      this.#sending = true
    }
    const resources = attachments.map((attachment) => attachment.resource)
    const queueAsFollowUp = this.shouldQueueFollowUp(command)
    let pendingFollowUpId = ''
    let sendEpoch = this.#sendEpoch

    try {
      const poller = new PollConversation()
      if (command && command.kind === 'new') {
        this.clearConversationDisplay()
        sendEpoch = this.#sendEpoch
        if (ownsSendingFlag) {
          this.#sending = true
        }
        if (command.prompt) {
          this.appendLocalMessage({
            role: 'user',
            text: command.prompt,
            conversation: SubmitMessageConversationId,
            attachments: attachments.length ? attachments : undefined
          })
        } else if (attachments.length) {
          this.appendLocalMessage({
            role: 'user',
            text: '',
            conversation: SubmitMessageConversationId,
            attachments
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
            id: this.nextLocalMessageId('m-side', SubmitMessageConversationId, timestamp),
            role: 'user',
            text: command.prompt,
            conversation: SubmitMessageConversationId,
            attachments: attachments.length ? attachments : undefined,
            timestamp
          }
        ]
      } else if (command && (command.kind === 'stop' || command.kind === 'steer')) {
        this.appendLocalMessage({
          role: 'user',
          text: command.prompt,
          conversation: this.#conversation?._id || SubmitMessageConversationId,
          attachments: attachments.length ? attachments : undefined
        })
      } else if (queueAsFollowUp) {
        pendingFollowUpId = this.appendPendingFollowUp(prompt, attachments)
      } else {
        this.appendLocalMessage({
          role: 'user',
          text: prompt,
          conversation: this.optimisticConversationId(),
          attachments: attachments.length ? attachments : undefined
        })
      }

      this.#api.updateStatus('sending', null)

      const isRequestStale = () =>
        sendEpoch !== this.#sendEpoch ||
        Boolean(pendingFollowUpId && !this.hasPendingFollowUp(pendingFollowUpId))
      const output = await this.agentRun({ name: '', prompt, resources }, isRequestStale)
      if (!output || isRequestStale()) {
        poller.finish()
        return poller
      }

      this.#session = output.session || ''
      const hasConversation = Boolean(output.conversation)
      if (output.conversation) {
        const conversation = await this.fetchConversation(output.conversation)
        this.updateLatestConversation(conversation)

        this.pollConversationLoop(poller)
      }

      if (output.failed_reason) {
        if (pendingFollowUpId) {
          this.removePendingFollowUp(pendingFollowUpId)
        }
        this.appendSystemMessage(output.failed_reason)
        this.#api.updateStatus('failed', null)
        poller.finish()
      } else if (output.chat_history && output.chat_history.length > 0) {
        // side messages
        const timestamp = Date.now()
        const messages = output.chat_history.flatMap((message, index) =>
          normalizeMessages(message, {
            conversation: 0,
            index,
            fallbackTimestamp: timestamp
          })
        )

        const sideMessages = []
        for (const msg of this.#sideMessages) {
          if (msg.id.startsWith('m-side-') && messages.some((m) => sameMessageContent(m, msg))) {
            continue
          }
          sideMessages.push(msg)
        }
        this.#sideMessages = [...sideMessages, ...messages]
        poller.push(...messages.filter((message) => message.role === 'assistant'))
        this.#api.updateStatus('completed', null)
        poller.finish()
      } else if (!hasConversation) {
        if (pendingFollowUpId) {
          this.removePendingFollowUp(pendingFollowUpId)
        }
        this.#api.updateStatus('idle', null)
        poller.finish()
      }

      return poller
    } catch (error) {
      if (pendingFollowUpId) {
        this.removePendingFollowUp(pendingFollowUpId)
      }
      this.#api.updateStatus('request failed', { kind: 'error', text: errorToMessage(error) })
      return null
    } finally {
      if (ownsSendingFlag && sendEpoch === this.#sendEpoch) {
        this.#sending = false
      }
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
          ...result.messages.flatMap((message, index) =>
            normalizeMessages(message, {
              conversation: conversation._id,
              index: start + index,
              fallbackTimestamp: conversation.updated_at
            })
          )
        )
        poller.drain()
      }

      const terminal = isTerminalConversationStatus(conversation.status)
      if (terminal || this.hasPendingLocalAttachments(conversation._id)) {
        const refreshed = await this.fetchConversation(conversation._id)
        conversation.messages = refreshed.messages || []
        conversation.artifacts = refreshed.artifacts || []
        conversation.status = refreshed.status
        conversation.usage = refreshed.usage
        conversation.failed_reason = refreshed.failed_reason
        conversation.updated_at = refreshed.updated_at
        conversation.child = refreshed.child
        this.updateLatestConversation(refreshed)
      }

      if (
        terminal ||
        conversation.status === 'completed' ||
        conversation.status === 'cancelled' ||
        conversation.status === 'failed'
      ) {
        return false
      }
    } catch (error) {
      if (isTransientWebSocketError(error)) {
        this.#api.updateStatus('reconnecting', null)
        return true
      }

      this.#api.updateStatus('poll failed', { kind: 'error', text: errorToMessage(error) })
      return false
    }

    return true
  }

  private async requestMeta(): Promise<RequestMeta> {
    const extra = await this.#api.requestExtra()
    extra.source = this.source
    const workspace = workspaceFromCliSource(this.source)
    if (workspace) {
      extra.workspace = workspace
    }
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

  private async agentRun(input: AgentInput, isStale?: () => boolean): Promise<AgentOutput | null> {
    const meta = await this.requestMeta()
    if (isStale?.()) {
      return null
    }
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
    const existingGroup = this.#messageGroups.find((existing) => existing._id === conversation._id)
    const submitGroup = this.#messageGroups.find(
      (existing) => existing._id === SubmitMessageConversationId
    )
    mergePendingLocalMessages(group, [existingGroup, submitGroup])
    this.removeAcceptedPendingFollowUps(group.messages)

    const idx = this.#messageGroups.findIndex((existing) => existing._id >= conversation._id)
    if (idx >= 0) {
      this.#messageGroups.length = idx
    }

    group.current = true
    this.#messageGroups.push(group)
    if (submitGroup) {
      this.#messageGroups.push(submitGroup)
      this.removeSubmittedMessage((msg) => group.messages.some((m) => sameMessageContent(m, msg)))
    }
  }

  private hasPendingLocalAttachments(conversationId: number): boolean {
    const group = this.#messageGroups.find((group) => group._id === conversationId)
    return Boolean(
      group?.messages.some((message) => message.pending && (message.attachments?.length || 0) > 0)
    )
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
        mergePendingLocalMessages(b, [a])
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
    this.removeAcceptedPendingFollowUps(merged.flatMap((group) => group.messages))
  }

  private clearConversationDisplay(): void {
    this.#sendEpoch += 1
    this.#session = ''
    this.#conversation = null
    this.#messageGroups = []
    this.#sending = false
    this.#loadingPrevious = false
    this.#pollingConversation = 0
    this.#syncing = false
    this.#syncAt = 0
    this.#conversationAncestors = []
    this.#pendingFollowUps = []
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
            id: this.nextLocalMessageId('m', group._id, timestamp),
            conversation: group._id,
            pending: true,
            timestamp
          }
        ]
        return { ...group }
      }
    )
  }

  private appendPendingFollowUp(prompt: string, attachments: ChatAttachment[]): string {
    const timestamp = Date.now()
    const id = this.nextLocalMessageId('m-follow-up', this.#conversation?._id || 0, timestamp)
    this.#pendingFollowUps = [
      ...this.#pendingFollowUps,
      {
        id,
        role: 'user',
        text: prompt,
        conversation: this.#conversation?._id || SubmitMessageConversationId,
        attachments: attachments.length ? attachments : undefined,
        pending: true,
        timestamp
      }
    ]
    return id
  }

  private removePendingFollowUp(id: string): void {
    this.#pendingFollowUps = this.#pendingFollowUps.filter((message) => message.id !== id)
  }

  private hasPendingFollowUp(id: string): boolean {
    return this.#pendingFollowUps.some((message) => message.id === id)
  }

  private removeAcceptedPendingFollowUps(serverMessages: ChatMessage[]): void {
    if (!this.#pendingFollowUps.length || !serverMessages.length) {
      return
    }

    const acceptedMessages = serverMessages
      .filter((message) => message.role === 'user')
      .map((message) => ({
        remainingText: normalizedFollowUpText(message.text)
      }))
    const remaining: ChatMessage[] = []
    for (const pending of this.#pendingFollowUps) {
      const pendingText = normalizedFollowUpText(pending.text)
      const accepted = pendingText
        ? acceptedMessages.find((message) => message.remainingText.includes(pendingText))
        : undefined
      if (!accepted) {
        remaining.push(pending)
        continue
      }

      const index = accepted.remainingText.indexOf(pendingText)
      accepted.remainingText =
        accepted.remainingText.slice(0, index) +
        accepted.remainingText.slice(index + pendingText.length)
    }
    this.#pendingFollowUps = remaining
  }

  private shouldQueueFollowUp(command: ReturnType<typeof parsePromptCommand>): boolean {
    if (command) {
      return false
    }
    return this.#conversation?.status === 'working' || this.#conversation?.status === 'submitted'
  }

  private optimisticConversationId(): number {
    return this.#conversation?.status === 'idle'
      ? this.#conversation._id
      : SubmitMessageConversationId
  }

  private nextLocalMessageId(prefix: string, conversation: number, timestamp = Date.now()): string {
    this.#localMessageSeq += 1
    return `${prefix}-${conversation}-${timestamp}-${this.#localMessageSeq}`
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
        _id,
        status: 'submitted',
        ancestors: [],
        createdAt: nowMs,
        updatedAt: nowMs,
        messages: [],
        current: _id === this.#conversation?._id
      })
      this.#messageGroups = [...this.#messageGroups, group]
    }
  }
}

function workspaceFromCliSource(source: string): string {
  if (!source.startsWith('cli:')) {
    return ''
  }

  const raw = source.slice(4).trim()
  const workspace = raw.startsWith('voice:') ? raw.slice(6).trim() : raw
  if (!isAbsoluteWorkspacePath(workspace)) {
    return ''
  }
  return trimTrailingPathSeparator(workspace)
}

function isAbsoluteWorkspacePath(value: string): boolean {
  return value.startsWith('/') || /^[A-Za-z]:[\\/]/.test(value) || value.startsWith('\\\\')
}

function trimTrailingPathSeparator(value: string): string {
  let trimmed = value.trim()
  while (
    trimmed.length > 1 &&
    /[\\/]$/.test(trimmed) &&
    trimmed !== '/' &&
    !/^[A-Za-z]:[\\/]$/.test(trimmed)
  ) {
    trimmed = trimmed.slice(0, -1)
  }
  return trimmed
}

function sameMessageContent(a: ChatMessage, b: ChatMessage): boolean {
  return a.role === b.role && a.text.trim() === b.text.trim()
}

function normalizedFollowUpText(text: string): string {
  return text.trim().replace(/\s+/g, ' ')
}

function mergePendingLocalMessages(
  group: MessageGroup,
  localGroups: Array<MessageGroup | undefined>
): void {
  const localMessages = localGroups
    .flatMap((localGroup) => localGroup?.messages || [])
    .filter((message) => message.pending)
  if (!localMessages.length) {
    return
  }

  const matched = new Set<ChatMessage>()
  group.messages = group.messages.map((message) => {
    const localMessage = localMessages.find(
      (candidate) => !matched.has(candidate) && sameMessageContent(message, candidate)
    )
    if (!localMessage) {
      return message
    }

    matched.add(localMessage)
    return mergeServerAndLocalMessage(message, localMessage)
  })

  const unmatched = localMessages.filter((message) => !matched.has(message))
  if (!unmatched.length) {
    return
  }

  group.messages = [...group.messages, ...unmatched]
  group.updatedAt = Math.max(group.updatedAt, ...unmatched.map((message) => message.timestamp || 0))
}

function mergeServerAndLocalMessage(server: ChatMessage, local: ChatMessage): ChatMessage {
  const attachments = mergeAttachments(server.attachments || [], local.attachments || [])
  const missingLocalAttachments = (local.attachments || []).some(
    (attachment) => !hasMatchingAttachment(server.attachments || [], attachment)
  )

  return {
    ...server,
    attachments: attachments.length ? attachments : undefined,
    pending: missingLocalAttachments || undefined
  }
}

function mergeAttachments(server: ChatAttachment[], local: ChatAttachment[]): ChatAttachment[] {
  const merged = [...server]
  for (const localAttachment of local) {
    const idx = merged.findIndex((attachment) => sameAttachment(attachment, localAttachment))
    if (idx >= 0) {
      merged[idx] = mergeAttachment(merged[idx]!, localAttachment)
    } else {
      merged.push(localAttachment)
    }
  }
  return merged
}

function mergeAttachment(server: ChatAttachment, local: ChatAttachment): ChatAttachment {
  const resource = {
    ...local.resource,
    ...server.resource,
    tags: mergeTags(local.resource.tags, server.resource.tags),
    metadata: {
      ...(local.resource.metadata || {}),
      ...(server.resource.metadata || {})
    },
    blob: server.resource.blob || local.resource.blob,
    description: server.resource.description || local.resource.description
  }

  return {
    ...local,
    ...server,
    id: resource._id ? `resource-${resource._id}` : server.id || local.id,
    name: server.name || local.name,
    type: server.type || local.type,
    size: server.size || local.size,
    resource
  }
}

function hasMatchingAttachment(attachments: ChatAttachment[], target: ChatAttachment): boolean {
  return attachments.some((attachment) => sameAttachment(attachment, target))
}

function sameAttachment(a: ChatAttachment, b: ChatAttachment): boolean {
  if (a.id && b.id && a.id === b.id) {
    return true
  }
  if (a.resource._id && b.resource._id && a.resource._id === b.resource._id) {
    return true
  }
  if (a.resource.hash && b.resource.hash && a.resource.hash === b.resource.hash) {
    return true
  }
  if (a.resource.uri && b.resource.uri && a.resource.uri === b.resource.uri) {
    return true
  }

  const aType = a.type || a.resource.mime_type || ''
  const bType = b.type || b.resource.mime_type || ''
  const sizeMatches = a.size == null || b.size == null || a.size === b.size
  return a.name === b.name && sizeMatches && aType === bType
}

function mergeTags(a: string[] = [], b: string[] = []): string[] {
  return Array.from(new Set([...a, ...b].filter(Boolean)))
}

function isTerminalConversationStatus(status: Conversation['status']): boolean {
  return status === 'completed' || status === 'cancelled' || status === 'failed'
}
