import {
	errorToError,
	errorToMessage,
	isTransientWebSocketError
} from '$lib/service-worker/settings'
import { delay } from '$lib/utils/helper'
import { parseNewPromptCommand } from './commands'
import { conversationToGroup, normalizeMessage, splitLegacyThoughtText } from './conversations'
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
import { SideMessageConversationId, SubmitMessageConversationId } from './types'

const pollingIntervalMs = 3000

function isPersistedConversationId(conversationId: number): boolean {
	return conversationId > SideMessageConversationId && conversationId < SubmitMessageConversationId
}

export interface API {
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

	get messageGroups(): MessageGroup[] {
		return this.#messageGroups
	}

	destroy(): void {
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
				latest.status === 'working' ||
				latest.status === 'submitted' ||
				latest.status === 'idle'
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
		const newCommand = parseNewPromptCommand(prompt)

		try {
			const meta = await this.requestMeta()
			const poller = new PollConversation()
			if (newCommand) {
				this.clearConversationDisplay()
				if (newCommand.prompt) {
					this.appendLocalMessage({
						role: 'user',
						text: prompt,
						conversation: SubmitMessageConversationId
					})
				}
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
			} else if (output.failed_reason) {
				this.appendSystemMessage(output.failed_reason)
				this.#api.updateStatus('failed', null)
				poller.finish()
			} else if (output.content && output.content.trim()) {
				const content = splitLegacyThoughtText(output.content)

				const timestamp = Date.now()
				this.appendLocalMessage({
					role: 'assistant',
					text: content.text,
					thinkingText: content.thinkingText,
					conversation: SideMessageConversationId
				})

				poller.push({
					id: `m-0-${timestamp}`,
					conversation: SideMessageConversationId,
					role: 'assistant',
					text: content.text,
					thinkingText: content.thinkingText,
					timestamp
				})
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
			await delay(pollingIntervalMs)
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
		return { extra } as RequestMeta
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

		const existingIndex = this.#messageGroups.findIndex(
			(existing) => existing.conversation._id >= conversation._id
		)

		if (existingIndex >= 0) {
			const existingSideMessages = this.#messageGroups[existingIndex].messages.filter(
				(message) => message.conversation === SideMessageConversationId
			)
			if (existingSideMessages.length > 0) {
				group.messages = [...existingSideMessages, ...group.messages]
				group.messages.sort((a, b) => (a.timestamp || 0) - (b.timestamp || 0))
			}
			this.#messageGroups.length = existingIndex
		}

		group.current = true
		this.#messageGroups = [...this.#messageGroups, group]
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

			if (a.conversation._id === b.conversation._id) {
				// Replace existing with incoming when IDs match
				merged.push(b)
				i++
				j++
			} else if (a.conversation._id < b.conversation._id) {
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

		this.#messageGroups = merged
		const firstPersistedConversation = merged.find((group) =>
			isPersistedConversationId(group.conversation._id)
		)?.conversation
		this.#conversationAncestors = firstPersistedConversation?.ancestors || []
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
		this.appendLocalMessage({ role: 'system', text, conversation: SideMessageConversationId })
	}

	private appendLocalMessage(message: Omit<ChatMessage, 'id'>): void {
		this.updateMessageGroupWith(
			message.conversation || this.#conversation?._id || SideMessageConversationId,
			(group) => {
				const timestamp = Date.now()
				group.messages = [
					...group.messages,
					{
						...message,
						id: `m-${group.conversation._id}-${timestamp}`,
						conversation: group.conversation._id,
						timestamp
					}
				]
				return { ...group }
			}
		)
	}

	private updateMessageGroupWith(conversation: number, fn: (group: MessageGroup) => MessageGroup) {
		const idx = this.#messageGroups.findIndex((group) => group.conversation._id === conversation)
		if (idx >= 0) {
			const updated = fn(this.#messageGroups[idx]!)
			this.#messageGroups[idx] = updated
		} else {
			const nowMs = Date.now()
			const group = fn({
				conversation: {
					_id: conversation,
					user: '',
					status: 'submitted',
					usage: {
						input_tokens: 0,
						output_tokens: 0,
						cached_tokens: 0,
						requests: 0
					},
					created_at: nowMs,
					updated_at: nowMs
				} as Conversation,
				createdAt: nowMs,
				updatedAt: nowMs,
				messages: [],
				current: false
			})
			this.#messageGroups = [...this.#messageGroups, group]
		}
	}
}
