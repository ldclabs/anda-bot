import {
  errorToError,
  errorToMessage,
  isTransientWebSocketError
} from '$lib/service-worker/settings'
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
  #sending: boolean = $state(false)
  #loadingPrevious: boolean = $state(false)
  #pollingConversation: number = $state(0)
  #conversationAncestors: number[] = $state([])
  #syncing: boolean = $state(false)
  #syncAt: number = 0
  #localMessageSeq: number = 0
  #sendEpoch: number = 0
  #pollWake: (() => void) | null = null
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
    const epoch = this.#sendEpoch
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
      if (conversations.length === 0 || epoch !== this.#sendEpoch) {
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

    // A /side request runs a detached subagent and can take a long time; it
    // must not hold the sending flag and freeze the composer for other input.
    const ownsSendingFlag = (!this.#sending || command?.kind === 'new') && command?.kind !== 'side'
    if (ownsSendingFlag) {
      this.#sending = true
    }
    const resources = attachments.map((attachment) => attachment.resource)
    let sendEpoch = this.#sendEpoch
    // A bare /new only detaches the current session; the daemon answers with
    // the old conversation id, which must not be re-fetched into the display
    // that was just cleared.
    const bareNew = command?.kind === 'new' && !command.prompt && attachments.length === 0
    const localMessageIds: string[] = []
    let delivered = false

    try {
      const poller = new PollConversation()
      if (command && command.kind === 'new') {
        this.clearConversationDisplay()
        sendEpoch = this.#sendEpoch
        if (ownsSendingFlag) {
          this.#sending = true
        }
        if (command.prompt || attachments.length) {
          localMessageIds.push(
            this.appendLocalMessage({
              role: 'user',
              text: command.prompt,
              conversation: SubmitMessageConversationId,
              attachments: attachments.length ? attachments : undefined
            })
          )
        }
      } else if (command && command.kind === 'side') {
        if (!command.prompt) {
          return null
        }

        const timestamp = Date.now()
        const sideId = this.nextLocalMessageId('m-side', SubmitMessageConversationId, timestamp)
        localMessageIds.push(sideId)
        this.#sideMessages = [
          ...this.#sideMessages,
          {
            id: sideId,
            role: 'user',
            text: command.prompt,
            conversation: SubmitMessageConversationId,
            attachments: attachments.length ? attachments : undefined,
            timestamp
          }
        ]
      } else if (command && (command.kind === 'stop' || command.kind === 'steer')) {
        // Anchor to the displayed conversation: both commands target the
        // session that the user is looking at, even when the local status is
        // stale. A failed delivery removes the message again below.
        localMessageIds.push(
          this.appendLocalMessage({
            role: 'user',
            text: command.prompt,
            conversation: this.#conversation?._id || SubmitMessageConversationId,
            attachments: attachments.length ? attachments : undefined
          })
        )
      } else {
        localMessageIds.push(
          this.appendLocalMessage({
            role: 'user',
            text: prompt,
            conversation: this.optimisticConversationId(),
            attachments: attachments.length ? attachments : undefined
          })
        )
      }

      this.#api.updateStatus('sending', null)

      const isRequestStale = () => sendEpoch !== this.#sendEpoch
      const output = await this.agentRun({ name: '', prompt, resources }, isRequestStale)
      delivered = Boolean(output)
      if (!output || isRequestStale()) {
        poller.finish()
        return poller
      }

      this.#session = output.session || ''
      const conversationId = bareNew ? 0 : output.conversation || 0
      const hasConversation = conversationId > 0
      if (hasConversation) {
        const conversation = await this.fetchConversation(conversationId)
        if (isRequestStale()) {
          poller.finish()
          return poller
        }
        this.updateLatestConversation(conversation)

        this.pollConversationLoop(poller)
        // If a loop was already polling this conversation, skip its remaining
        // sleep so the just-submitted prompt's status flip shows up promptly.
        this.wakePolling()
      }

      if (output.failed_reason) {
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
        this.#api.updateStatus(bareNew ? 'ready' : 'idle', null)
        poller.finish()
      }

      return poller
    } catch (error) {
      this.#api.updateStatus('request failed', { kind: 'error', text: errorToMessage(error) })
      if (!delivered) {
        // The prompt never reached the daemon: drop the optimistic messages
        // and rethrow so the composer can restore the draft for a retry.
        this.removeLocalMessages(localMessageIds)
        throw error
      }
      // Delivered but a follow-up fetch failed; the poll loop will reconcile.
      return null
    } finally {
      if (ownsSendingFlag && sendEpoch === this.#sendEpoch) {
        this.#sending = false
      }
    }
  }

  private async pollConversationLoop(poller: PollConversation): Promise<void> {
    const epoch = this.#sendEpoch
    const conversation = this.#conversation ? { ...this.#conversation } : null
    if (!conversation || this.#pollingConversation === conversation._id) {
      poller.finish()
      return
    }

    this.#pollingConversation = conversation._id
    while (this.#pollingConversation === conversation._id && epoch === this.#sendEpoch) {
      const shouldContinue = await this.pollConversationOnce(conversation, poller, epoch)
      if (!shouldContinue) {
        break
      }
      const ms =
        this.#api.activeChannel() === this.source ? pollingIntervalMs : pollingIntervalMs * 10
      await this.pollIdle(ms)
    }

    // Release the lock so the same conversation can be polled again later
    // (e.g. when a follow-up prompt re-activates it after a terminal status).
    if (this.#pollingConversation === conversation._id) {
      this.#pollingConversation = 0
    }
    poller.finish()
  }

  // Sleep between poll ticks, but allow wakePolling() to cut the wait short
  // (new prompt submitted, channel re-activated, display cleared).
  private pollIdle(ms: number): Promise<void> {
    return new Promise((resolve) => {
      const finish = () => {
        clearTimeout(timer)
        if (this.#pollWake === finish) {
          this.#pollWake = null
        }
        resolve()
      }
      const timer = setTimeout(finish, ms)
      this.#pollWake = finish
    })
  }

  wakePolling(): void {
    this.#pollWake?.()
  }

  private async pollConversationOnce(
    conversation: Conversation,
    poller: PollConversation,
    epoch: number
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
      if (epoch !== this.#sendEpoch) {
        // The display was cleared (/new) while this request was in flight;
        // applying the stale result would resurrect the old conversation.
        return false
      }

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
        if (epoch !== this.#sendEpoch) {
          return false
        }
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
    // Two writers race here (the poll loop and sendPrompt's fetch); a slower
    // response carrying an older snapshot must not shrink the displayed group.
    const current = this.#conversation
    if (
      current &&
      current._id === conversation._id &&
      conversation.updated_at < current.updated_at &&
      (conversation.messages?.length || 0) <= (current.messages?.length || 0)
    ) {
      return
    }

    this.#conversation = conversation
    this.#conversationAncestors = conversation.ancestors || []
    this.#api.updateStatus(conversation.status, null)
    const group = conversationToGroup(conversation)
    const existingGroup = this.#messageGroups.find((existing) => existing._id === conversation._id)
    const submitGroup = this.#messageGroups.find(
      (existing) => existing._id === SubmitMessageConversationId
    )
    mergePendingLocalMessages(
      group,
      [existingGroup, submitGroup],
      knownServerMessageCount(existingGroup, group)
    )
    preserveMessageTimestamps(group, existingGroup)

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
        mergePendingLocalMessages(b, [a], knownServerMessageCount(a, b))
        preserveMessageTimestamps(b, a)
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
    // Let a sleeping poll loop notice the epoch change and exit right away.
    this.wakePolling()
  }

  private appendSystemMessage(text: string): void {
    this.appendLocalMessage({
      role: 'system',
      text,
      conversation: this.#conversation?._id || SubmitMessageConversationId
    })
  }

  private appendLocalMessage(message: Omit<ChatMessage, 'id'>): string {
    const conversationId =
      message.conversation || this.#conversation?._id || SubmitMessageConversationId
    const timestamp = Date.now()
    const id = this.nextLocalMessageId('m', conversationId, timestamp)
    this.updateMessageGroupWith(conversationId, (group) => {
      group.messages = [
        ...group.messages,
        {
          ...message,
          id,
          conversation: group._id,
          pending: true,
          timestamp
        }
      ]
      return { ...group }
    })
    return id
  }

  private removeLocalMessages(ids: string[]): void {
    if (!ids.length) {
      return
    }
    const idSet = new Set(ids)
    this.#messageGroups = this.#messageGroups
      .map((group) =>
        group.messages.some((message) => idSet.has(message.id))
          ? { ...group, messages: group.messages.filter((message) => !idSet.has(message.id)) }
          : group
      )
      .filter((group) => group.messages.length > 0 || group._id !== SubmitMessageConversationId)
    if (this.#sideMessages.some((message) => idSet.has(message.id))) {
      this.#sideMessages = this.#sideMessages.filter((message) => !idSet.has(message.id))
    }
  }

  private optimisticConversationId(): number {
    return this.#conversation &&
      ['idle', 'submitted', 'working'].includes(this.#conversation.status)
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

// Number of messages in `existing` that the server already knows about: every
// non-pending message plus pending ones that share an id with the incoming
// server group (a message merged earlier but still awaiting attachment
// confirmation keeps its server id and occupies a server slot).
export function knownServerMessageCount(
  existing: MessageGroup | undefined,
  incoming: MessageGroup
): number {
  if (!existing) {
    return 0
  }
  const serverIds = new Set(incoming.messages.map((message) => message.id))
  return existing.messages.reduce(
    (count, message) => count + (!message.pending || serverIds.has(message.id) ? 1 : 0),
    0
  )
}

export function mergePendingLocalMessages(
  group: MessageGroup,
  localGroups: Array<MessageGroup | undefined>,
  minMatchIndex = 0
): void {
  const localMessages = localGroups
    .flatMap((localGroup) => localGroup?.messages || [])
    .filter((message) => message.pending)
  if (!localMessages.length) {
    return
  }

  const matched = new Set<ChatMessage>()
  group.messages = group.messages.map((message, index) => {
    // A pending message that already carries this server message's id was
    // merged on an earlier pass (e.g. waiting for attachment confirmation);
    // always re-merge it so refreshed server data can settle it.
    let merged = message
    const sameIdLocal = localMessages.find(
      (local) => !matched.has(local) && local.id === message.id
    )
    if (sameIdLocal) {
      matched.add(sameIdLocal)
      merged = mergeServerAndLocalMessage(merged, sameIdLocal)
    }

    // Content-based matching is only allowed against server messages that
    // arrived after the local message was created. Matching older history
    // makes a freshly sent duplicate ("继续", "test", …) merge into an old
    // bubble and vanish from the bottom of the chat until the real server
    // copy lands seconds later.
    if (index < minMatchIndex) {
      return merged
    }
    const acceptedLocalMessages = findAcceptedLocalMessages(merged, localMessages, matched)
    if (!acceptedLocalMessages.length) {
      return merged
    }

    return acceptedLocalMessages.reduce(mergeServerAndLocalMessage, merged)
  })

  const unmatched = localMessages.filter((message) => !matched.has(message))
  if (!unmatched.length) {
    return
  }

  group.messages = [...group.messages, ...unmatched]
  group.updatedAt = Math.max(group.updatedAt, ...unmatched.map((message) => message.timestamp || 0))
}

// Messages without their own timestamp fall back to `conversation.updated_at`,
// which moves on every delta; keep the first-seen timestamp so rendered time
// labels do not drift across poll ticks.
export function preserveMessageTimestamps(
  group: MessageGroup,
  previous: MessageGroup | undefined
): void {
  if (!previous) {
    return
  }
  const seen = new Map<string, number | undefined>()
  for (const message of previous.messages) {
    seen.set(message.id, message.timestamp)
  }
  group.messages = group.messages.map((message) => {
    const timestamp = seen.get(message.id)
    return timestamp && timestamp !== message.timestamp ? { ...message, timestamp } : message
  })
}

function findAcceptedLocalMessages(
  server: ChatMessage,
  localMessages: ChatMessage[],
  matched: Set<ChatMessage>
): ChatMessage[] {
  const accepted: ChatMessage[] = []
  let remainingServerText = normalizedMessageText(server.text)
  let acceptedEmptyExact = false

  for (const local of localMessages) {
    if (matched.has(local) || local.role !== server.role) {
      continue
    }

    const localText = normalizedMessageText(local.text)
    if (sameMessageContent(server, local)) {
      if (!localText) {
        if (acceptedEmptyExact) {
          continue
        }
        acceptedEmptyExact = true
      } else {
        const index = remainingServerText.indexOf(localText)
        if (index < 0) {
          continue
        }
        remainingServerText =
          remainingServerText.slice(0, index) + remainingServerText.slice(index + localText.length)
      }
      matched.add(local)
      accepted.push(local)
      continue
    }

    if (server.role !== 'user' || !localText) {
      continue
    }

    const index = remainingServerText.indexOf(localText)
    if (index < 0) {
      continue
    }
    remainingServerText =
      remainingServerText.slice(0, index) + remainingServerText.slice(index + localText.length)
    matched.add(local)
    accepted.push(local)
  }

  return accepted
}

function normalizedMessageText(text: string): string {
  return text.trim().replace(/\s+/g, ' ')
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
