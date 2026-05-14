export interface SettingsState {
	baseUrl: string
	token: string
}

interface StorageState extends Partial<SettingsState> {
	browserSessionId?: string
}

export interface ChromeTabInfo {
	id?: number
	windowId?: number
	title?: string
	url?: string
}

export type MessageRole = 'user' | 'assistant' | 'system' | 'tool'

export interface AttachmentSummary {
	id: string
	name: string
	type?: string
	size?: number
}

export interface ResourceInput {
	_id?: number
	tags: string[]
	name: string
	description?: string
	uri?: string
	mime_type?: string
	blob?: string
	size?: number
	metadata?: Record<string, unknown>
}

export interface ChatAttachment extends AttachmentSummary {
	resource: ResourceInput
}

export interface ChatMessage {
	id: string
	role: MessageRole
	text: string
	timestamp?: string | number | null
	conversationId?: string
	local?: boolean
	attachments?: AttachmentSummary[]
}

export interface ConversationGroup {
	id: string
	status?: string
	createdAt?: number | null
	updatedAt?: number | null
	current?: boolean
	messages: ChatMessage[]
}

export interface ClientSnapshot {
	settings: SettingsState
	tab: ChromeTabInfo | null
	session: string | null
	conversationId: string | null
	conversationGroups: ConversationGroup[]
	messages: ChatMessage[]
	sending: boolean
	status: string
	loadingPrevious: boolean
	hasPreviousConversations: boolean
	syncing: boolean
}

interface ChromeEvent<Listener extends (...args: never[]) => void> {
	addListener(listener: Listener): void
	removeListener(listener: Listener): void
}

interface ChromeTabChangeInfo {
	title?: string
	url?: string
}

interface ChromeApi {
	runtime: {
		sendMessage<Result>(message: ExtensionMessage): Promise<ExtensionResponse<Result>>
	}
	extension?: {
		inIncognitoContext?: boolean
	}
	storage: {
		local: {
			get(keys: string[]): Promise<StorageState>
			set(items: StorageState): Promise<void>
		}
	}
	tabs: {
		query(queryInfo: { active: boolean; lastFocusedWindow: boolean }): Promise<ChromeTabInfo[]>
		onActivated: ChromeEvent<(activeInfo: { tabId: number; windowId: number }) => void>
		onUpdated: ChromeEvent<
			(tabId: number, changeInfo: ChromeTabChangeInfo, tab: ChromeTabInfo) => void
		>
	}
}

interface AgentRunOutput {
	conversation?: string | number | null
	content?: string
	failed_reason?: string
}

interface RawConversationMessage {
	role?: string
	content?: unknown
	name?: string | null
	timestamp?: string | number | null
	resources?: ResourceInput[]
}

interface RawConversation {
	_id?: string | number
	messages?: RawConversationMessage[]
	artifacts?: unknown[]
	child?: string | number | null
	status?: string
	failed_reason?: string | null
	created_at?: number | null
	updated_at?: number | null
}

interface ConversationDelta {
	messages?: RawConversationMessage[]
	artifacts?: unknown[]
	child?: string | number | null
	status?: string
	failed_reason?: string | null
	updated_at?: number | null
}

interface SourceState {
	c?: string | number | null
	conv_id?: string | number | null
}

interface KipOutput<Result> {
	result?: Result
	next_cursor?: string | null
	error?: unknown
	Err?: unknown
}

interface ToolOutput<Result> {
	output?: KipOutput<Result>
}

interface RequestMeta {
	extra: Record<string, unknown>
}

interface AgentRunInput {
	name: string
	prompt: string
	resources?: ResourceInput[]
	meta: RequestMeta
}

interface ExtensionMessage {
	type: string
	settings: SettingsState
	method?: string
	params?: unknown[]
}

type ExtensionResponse<Result> =
	| { ok: true; result?: Result; status?: string }
	| { ok: false; error: string; status?: string }

type SnapshotListener = (snapshot: ClientSnapshot) => void

const defaultSettings: SettingsState = {
	baseUrl: 'http://127.0.0.1:8042',
	token: ''
}

const browserSessionStorageKey = 'browserSessionId'
const pollingIntervalMs = 2000
const previousConversationPageSize = 8
const localConversationId = 'local-draft'

export class AndaSidePanelClient {
	private chrome: ChromeApi | null = null
	private destroyed = false
	private pollingConversation = false
	private messageCounter = 0
	private tabActivatedListener?: (activeInfo: { tabId: number; windowId: number }) => void
	private tabUpdatedListener?: (
		tabId: number,
		changeInfo: ChromeTabChangeInfo,
		tab: ChromeTabInfo
	) => void

	private state: Omit<ClientSnapshot, 'messages' | 'conversationGroups'> & {
		messageOffset: number
		artifactOffset: number
		conversationGroups: ConversationGroup[]
		previousCursor?: string | null
	} = {
		settings: { ...defaultSettings },
		tab: null,
		session: null,
		conversationId: null,
		conversationGroups: [],
		messageOffset: 0,
		artifactOffset: 0,
		previousCursor: undefined,
		sending: false,
		status: 'starting',
		loadingPrevious: false,
		hasPreviousConversations: false,
		syncing: false
	}

	constructor(private readonly onSnapshot: SnapshotListener) {}

	async init(): Promise<void> {
		this.chrome = getChromeApi()
		await this.loadSettings()
		this.state.session = await browserSession(this.chrome)
		this.emit()
		this.bindChromeEvents()
		await this.refreshActiveTab()
		this.updateStatus('ready')
		void this.syncServiceWorker().catch(() => undefined)
		if (this.state.settings.token) {
			await this.restoreSourceConversation().catch((error) => {
				this.appendSystemMessage(errorToMessage(error))
				this.updateStatus('restore failed')
			})
			this.startConversationPolling()
		}
	}

	destroy(): void {
		this.destroyed = true
		if (this.chrome && this.tabActivatedListener) {
			this.chrome.tabs.onActivated.removeListener(this.tabActivatedListener)
		}
		if (this.chrome && this.tabUpdatedListener) {
			this.chrome.tabs.onUpdated.removeListener(this.tabUpdatedListener)
		}
	}

	getSnapshot(): ClientSnapshot {
		const conversationGroups = this.state.conversationGroups.map((group) => ({
			...group,
			messages: group.messages.map((message) => ({
				...message,
				attachments: message.attachments?.map((attachment) => ({ ...attachment }))
			}))
		}))
		return {
			settings: { ...this.state.settings },
			tab: this.state.tab ? { ...this.state.tab } : null,
			session: this.state.session,
			conversationId: this.state.conversationId,
			conversationGroups,
			messages: flattenMessages(conversationGroups),
			sending: this.state.sending,
			status: this.state.status,
			loadingPrevious: this.state.loadingPrevious,
			hasPreviousConversations: this.state.hasPreviousConversations,
			syncing: this.state.syncing
		}
	}

	async saveSettings(settings: SettingsState, options: { quiet?: boolean } = {}): Promise<void> {
		const chrome = this.requireChrome()
		this.state.settings = normalizeSettings(settings)
		await chrome.storage.local.set(this.state.settings)
		this.emit()
		if (!options.quiet) {
			this.appendSystemMessage('Settings saved.')
		}
		await this.syncServiceWorker().catch(() => undefined)
		if (this.state.settings.token) {
			void this.restoreSourceConversation()
				.then(() => this.startConversationPolling())
				.catch((error) => this.appendSystemMessage(errorToMessage(error)))
		}
	}

	async testConnection(settings: SettingsState): Promise<void> {
		try {
			await this.saveSettings(settings, { quiet: true })
			await this.rpc('information', [])
			this.updateStatus('connected')
			this.appendSystemMessage('Connection test passed.')
		} catch (error) {
			this.updateStatus('connection failed')
			this.appendSystemMessage(errorToMessage(error))
			throw error
		}
	}

	async sendPrompt(text: string, attachments: ChatAttachment[] = []): Promise<void> {
		const prompt = text.trim()
		if ((!prompt && attachments.length === 0) || this.state.sending) {
			return
		}
		if (!this.state.settings.token) {
			this.appendSystemMessage('Paste a bearer token generated by `anda chrome token` first.')
			return
		}

		await this.refreshActiveTab()
		const resources = attachments.map((attachment) => attachment.resource)
		const effectivePrompt = prompt || 'Please review the attached files.'
		this.state.sending = true
		this.emit()
		this.appendMessage(
			{
				role: 'user',
				text: effectivePrompt,
				local: true,
				attachments: attachments.map(({ resource: _resource, ...attachment }) => attachment)
			},
			this.state.conversationId || localConversationId
		)
		this.updateStatus('sending')

		try {
			const meta = await this.requestMeta()
			const output = await this.agentRun({ name: '', prompt: effectivePrompt, resources, meta })
			const outputConversationId = normalizeId(output.conversation)
			if (outputConversationId) {
				this.promoteLocalConversation(outputConversationId)
				if (this.state.conversationId !== outputConversationId) {
					this.state.messageOffset = 0
					this.state.artifactOffset = 0
				}
				this.state.conversationId = outputConversationId
			}
			if (output.content && output.content.trim()) {
				this.appendMessage({ role: 'assistant', text: output.content }, this.state.conversationId)
			}
			if (output.failed_reason) {
				this.appendSystemMessage(output.failed_reason)
			}
			this.startConversationPolling()
		} catch (error) {
			this.appendSystemMessage(errorToMessage(error))
			this.updateStatus('request failed')
		} finally {
			this.state.sending = false
			this.emit()
		}
	}

	async restoreSourceConversation(): Promise<boolean> {
		if (!this.state.settings.token) {
			return false
		}

		this.state.syncing = true
		this.emit()
		try {
			const meta = await this.requestMeta()
			const toolOutput = await this.toolCall<SourceState>(
				'conversations_api',
				{ type: 'GetSourceState' },
				meta
			)
			const state = kipResult(toolOutput)
			const sourceConversationId = sourceStateConversationId(state)
			if (!sourceConversationId) {
				this.state.hasPreviousConversations = true
				return false
			}

			const conversations = await this.fetchConversationChain(sourceConversationId)
			if (conversations.length === 0) {
				this.state.hasPreviousConversations = true
				return false
			}

			this.replaceCurrentConversationChain(conversations)
			this.state.hasPreviousConversations = true
			this.updateStatus(
				lastDefined(conversations.map((conversation) => conversation.status)) || 'idle'
			)
			return true
		} finally {
			this.state.syncing = false
			this.emit()
		}
	}

	async loadPreviousConversations(): Promise<boolean> {
		if (
			!this.state.settings.token ||
			this.state.loadingPrevious ||
			this.state.hasPreviousConversations === false
		) {
			return false
		}

		this.state.loadingPrevious = true
		this.emit()
		try {
			const meta = await this.requestMeta()
			const toolOutput = await this.toolCall<RawConversation[]>(
				'conversations_api',
				{
					type: 'ListPrevConversations',
					cursor: this.state.previousCursor ?? null,
					limit: previousConversationPageSize
				},
				meta
			)
			const { result: conversations, nextCursor } = kipResultWithCursor(toolOutput)
			const groups = conversations.map(conversationToGroup).filter((group) => group.messages.length)
			const insertedCount = this.prependConversationGroups(groups)
			this.state.previousCursor = nextCursor ?? null
			this.state.hasPreviousConversations = Boolean(nextCursor)
			return insertedCount > 0
		} catch (error) {
			this.appendSystemMessage(errorToMessage(error))
			this.updateStatus('history failed')
			return false
		} finally {
			this.state.loadingPrevious = false
			this.emit()
		}
	}

	private bindChromeEvents(): void {
		const chrome = this.requireChrome()
		this.tabActivatedListener = () => {
			void this.refreshActiveTab().catch(() => undefined)
		}
		this.tabUpdatedListener = (tabId, changeInfo, tab) => {
			if (this.state.tab && tabId === this.state.tab.id && (changeInfo.title || changeInfo.url)) {
				this.state.tab = { ...this.state.tab, ...tab }
				this.emit()
				void this.registerBrowserSession().catch(() => undefined)
			}
		}
		chrome.tabs.onActivated.addListener(this.tabActivatedListener)
		chrome.tabs.onUpdated.addListener(this.tabUpdatedListener)
	}

	private async loadSettings(): Promise<void> {
		const chrome = this.requireChrome()
		const saved = await chrome.storage.local.get(['baseUrl', 'token'])
		this.state.settings = normalizeSettings({
			baseUrl: saved.baseUrl || defaultSettings.baseUrl,
			token: saved.token || ''
		})
		this.emit()
	}

	private async refreshActiveTab(): Promise<void> {
		const chrome = this.requireChrome()
		const [tab] = await chrome.tabs.query({ active: true, lastFocusedWindow: true })
		if (!this.state.session) {
			this.state.session = await browserSession(chrome)
		}
		this.state.tab = tab || null
		this.emit()
		await this.registerBrowserSession().catch(() => undefined)
	}

	private async agentRun(input: AgentRunInput): Promise<AgentRunOutput> {
		const payload = input.resources?.length ? input : { ...input, resources: undefined }
		return this.rpc<AgentRunOutput>('agent_run', [payload])
	}

	private async toolCall<Result>(
		name: string,
		args: Record<string, unknown>,
		meta: RequestMeta
	): Promise<ToolOutput<Result>> {
		return this.rpc<ToolOutput<Result>>('tool_call', [{ name, args, meta }])
	}

	private async rpc<Result>(method: string, tupleArgs: unknown[]): Promise<Result> {
		if (!this.state.settings.token) {
			throw new Error('missing bearer token')
		}
		const response = await this.serviceWorkerMessage<Result>('anda_rpc', {
			method,
			params: tupleArgs
		})
		return response.result as Result
	}

	private async syncServiceWorker(): Promise<void> {
		await this.serviceWorkerMessage('anda_settings_changed')
	}

	private async serviceWorkerMessage<Result = unknown>(
		type: string,
		message: Partial<ExtensionMessage> = {}
	): Promise<Extract<ExtensionResponse<Result>, { ok: true }>> {
		const chrome = this.requireChrome()
		const response = await chrome.runtime.sendMessage<Result>({
			type,
			settings: this.state.settings,
			...message
		})
		if (!response?.ok) {
			throw new Error(response?.error || 'extension service worker returned an error')
		}
		return response
	}

	private startConversationPolling(): void {
		if (this.pollingConversation || !this.state.conversationId) {
			return
		}
		this.pollingConversation = true
		void this.pollConversationLoop().finally(() => {
			this.pollingConversation = false
		})
	}

	private async pollConversationLoop(): Promise<void> {
		while (!this.destroyed && this.state.conversationId) {
			const keepPolling = await this.pollConversationOnce()
			if (!keepPolling) {
				return
			}
			await delay(pollingIntervalMs)
		}
	}

	private async pollConversationOnce(): Promise<boolean> {
		if (!this.state.conversationId) {
			return false
		}

		try {
			const conversationId = this.state.conversationId
			const meta = await this.requestMeta()
			meta.extra.conversation = numericConversationId(conversationId)
			const toolOutput = await this.toolCall<ConversationDelta>(
				'conversations_api',
				{
					type: 'GetConversationDelta',
					_id: numericConversationId(conversationId),
					messages_offset: this.state.messageOffset,
					artifacts_offset: this.state.artifactOffset
				},
				meta
			)
			const delta = kipResult(toolOutput)
			const rawMessages = delta.messages || []
			const incoming = rawMessages
				.map((message, index) =>
					normalizeMessage(message, {
						conversationId,
						index: this.state.messageOffset + index,
						fallbackTimestamp: delta.updated_at
					})
				)
				.filter(isChatMessageInput)
			this.mergeIncomingMessages(conversationId, incoming, delta.status, delta.updated_at)
			this.state.messageOffset += rawMessages.length
			this.state.artifactOffset += (delta.artifacts || []).length

			const childId = normalizeId(delta.child)
			if (childId && childId !== this.state.conversationId) {
				this.state.conversationId = childId
				this.state.messageOffset = 0
				this.state.artifactOffset = 0
				this.emit()
				return true
			}

			this.updateStatus(delta.status || 'idle')
			return ['submitted', 'working', 'idle'].includes(delta.status || '')
		} catch (error) {
			this.appendSystemMessage(errorToMessage(error))
			this.updateStatus('poll failed')
			return false
		}
	}

	private async registerBrowserSession(): Promise<void> {
		const chrome = this.requireChrome()
		if (!this.state.session) {
			this.state.session = await browserSession(chrome)
			this.emit()
		}
		if (!this.state.settings.token) {
			return
		}

		const response = await this.serviceWorkerMessage<{ session?: string }>('anda_register')
		if (response.result?.session && response.result.session !== this.state.session) {
			this.state.session = response.result.session
			this.emit()
		}
	}

	private async requestMeta(): Promise<RequestMeta> {
		if (!this.state.tab) {
			await this.refreshActiveTab()
		}

		const chrome = this.requireChrome()
		const session = this.state.session || (await browserSession(chrome))
		this.state.session = session
		const extra: Record<string, unknown> = {
			source: session,
			browser_client: 'chrome_extension'
		}

		if (this.state.conversationId && numericConversationId(this.state.conversationId) > 0) {
			extra.conversation = numericConversationId(this.state.conversationId)
		}
		if (this.state.tab) {
			extra.tab = this.state.tab
		}

		return { extra }
	}

	private async fetchConversation(conversationId: string): Promise<RawConversation> {
		const meta = await this.requestMeta()
		meta.extra.conversation = numericConversationId(conversationId)
		const toolOutput = await this.toolCall<RawConversation>(
			'conversations_api',
			{ type: 'GetConversation', _id: numericConversationId(conversationId) },
			meta
		)
		return kipResult(toolOutput)
	}

	private async fetchConversationChain(conversationId: string): Promise<RawConversation[]> {
		const conversations: RawConversation[] = []
		const seen = new Set<string>()
		let nextId: string | null = conversationId

		while (nextId && conversations.length < 64) {
			if (seen.has(nextId)) {
				break
			}
			seen.add(nextId)
			const conversation = await this.fetchConversation(nextId)
			conversations.push(conversation)
			nextId = normalizeId(conversation.child)
		}

		return conversations
	}

	private replaceCurrentConversationChain(conversations: RawConversation[]): void {
		const groups = conversations.map(conversationToGroup).filter((group) => group.messages.length)
		const currentIds = new Set(groups.map((group) => group.id))
		const preservedGroups = this.state.conversationGroups.filter((group) => !group.current)
		const mergedPreserved = preservedGroups.filter((group) => !currentIds.has(group.id))
		const currentGroups = groups.map((group, index) => ({
			...group,
			current: index === groups.length - 1
		}))
		this.state.conversationGroups = [...mergedPreserved, ...currentGroups]

		const latestConversation = conversations[conversations.length - 1]
		const latestGroup = currentGroups[currentGroups.length - 1]
		this.state.conversationId = latestGroup?.id || normalizeId(latestConversation?._id)
		this.state.messageOffset =
			latestConversation?.messages?.length || latestGroup?.messages.length || 0
		this.state.artifactOffset = latestConversation?.artifacts?.length || 0
		this.emit()
	}

	private prependConversationGroups(groups: ConversationGroup[]): number {
		if (!groups.length) {
			return 0
		}
		const existingIds = new Set(this.state.conversationGroups.map((group) => group.id))
		const uniqueGroups = groups.filter((group) => !existingIds.has(group.id))
		if (!uniqueGroups.length) {
			return 0
		}
		const ordered = uniqueGroups.sort(compareConversationGroups)
		this.state.conversationGroups = [...ordered, ...this.state.conversationGroups]
		this.emit()
		return uniqueGroups.length
	}

	private mergeIncomingMessages(
		conversationId: string,
		incoming: ChatMessage[],
		status?: string,
		updatedAt?: number | null
	): void {
		if (!incoming.length && !status) {
			return
		}
		const group = this.ensureConversationGroup(conversationId)
		if (status) {
			group.status = status
		}
		if (updatedAt) {
			group.updatedAt = updatedAt
		}
		const overlap = displayedSuffixPrefixOverlap(group.messages, incoming)
		for (const message of incoming.slice(overlap)) {
			group.messages = [...group.messages, message]
		}
		this.emit()
	}

	private appendSystemMessage(text: string): void {
		this.appendMessage({ role: 'system', text }, this.state.conversationId || localConversationId)
	}

	private appendMessage(message: Omit<ChatMessage, 'id'>, conversationId?: string | null): void {
		const group = this.ensureConversationGroup(conversationId || localConversationId)
		this.messageCounter += 1
		group.messages = [
			...group.messages,
			{
				...message,
				conversationId: group.id,
				id: `${group.id}-local-${Date.now()}-${this.messageCounter}`,
				timestamp: message.timestamp ?? Date.now()
			}
		]
		this.emit()
	}

	private ensureConversationGroup(conversationId: string): ConversationGroup {
		const existing = this.state.conversationGroups.find((group) => group.id === conversationId)
		if (existing) {
			return existing
		}
		const group: ConversationGroup = {
			id: conversationId,
			status: conversationId === localConversationId ? 'local' : undefined,
			createdAt: Date.now(),
			updatedAt: Date.now(),
			current: conversationId !== localConversationId,
			messages: []
		}
		this.state.conversationGroups = [...this.state.conversationGroups, group]
		return group
	}

	private promoteLocalConversation(conversationId: string): void {
		const localGroup = this.state.conversationGroups.find(
			(group) => group.id === localConversationId
		)
		if (!localGroup) {
			return
		}
		const existing = this.state.conversationGroups.find((group) => group.id === conversationId)
		if (existing) {
			existing.messages = [...existing.messages, ...localGroup.messages]
			this.state.conversationGroups = this.state.conversationGroups.filter(
				(group) => group.id !== localConversationId
			)
			return
		}
		localGroup.id = conversationId
		localGroup.current = true
		localGroup.status = 'submitted'
		localGroup.messages = localGroup.messages.map((message) => ({ ...message, conversationId }))
	}

	private updateStatus(status: string): void {
		this.state.status = status
		this.emit()
	}

	private emit(): void {
		if (!this.destroyed) {
			this.onSnapshot(this.getSnapshot())
		}
	}

	private requireChrome(): ChromeApi {
		if (!this.chrome) {
			throw new Error('Chrome extension APIs are unavailable. Load the built extension in Chrome.')
		}
		return this.chrome
	}
}

function getChromeApi(): ChromeApi {
	const chromeApi = (globalThis as typeof globalThis & { chrome?: ChromeApi }).chrome
	if (!chromeApi?.runtime || !chromeApi.storage?.local || !chromeApi.tabs) {
		throw new Error('Chrome extension APIs are unavailable. Load the built extension in Chrome.')
	}
	return chromeApi
}

function conversationToGroup(conversation: RawConversation): ConversationGroup {
	const conversationId = normalizeId(conversation._id) || localConversationId
	const messages = (conversation.messages || [])
		.map((message, index) =>
			normalizeMessage(message, {
				conversationId,
				index,
				fallbackTimestamp: conversation.updated_at
			})
		)
		.filter(isChatMessageInput)

	if (conversation.status === 'failed' && conversation.failed_reason) {
		messages.push({
			id: `${conversationId}-failed`,
			conversationId,
			role: 'system',
			text: `An error occurred:\n\n\`\`\`\n${conversation.failed_reason}\n\`\`\``,
			timestamp: conversation.updated_at || Date.now()
		})
	}

	return {
		id: conversationId,
		status: conversation.status,
		createdAt: conversation.created_at ?? null,
		updatedAt: conversation.updated_at ?? null,
		current: false,
		messages
	}
}

function normalizeMessage(
	raw: RawConversationMessage,
	context: { conversationId: string; index: number; fallbackTimestamp?: string | number | null }
): ChatMessage | null {
	if (!raw || !isMessageRole(raw.role)) {
		return null
	}
	if (raw.name?.startsWith('$')) {
		return null
	}
	const text = contentToText(raw.content).trim()
	if (!text) {
		return null
	}
	return {
		id: `${context.conversationId}-${context.index}`,
		conversationId: context.conversationId,
		role: raw.role,
		text,
		timestamp: raw.timestamp || context.fallbackTimestamp || null,
		attachments: raw.resources?.map(resourceToAttachmentSummary)
	}
}

function contentToText(content: unknown): string {
	if (typeof content === 'string') {
		return content
	}
	if (!Array.isArray(content)) {
		return ''
	}
	return content
		.map((part) => {
			if (typeof part === 'string') {
				return part
			}
			if (!isContentPart(part)) {
				return ''
			}
			if ((part.type === 'Text' || part.type === 'Reasoning') && typeof part.text === 'string') {
				return part.text
			}
			if (part.type === 'ToolOutput') {
				return fencedJson(part.output)
			}
			if (part.type === 'ToolCall') {
				return fencedJson({ name: part.name, args: part.args })
			}
			return ''
		})
		.filter(Boolean)
		.join('\n\n')
}

function isContentPart(value: unknown): value is Record<string, unknown> & { type?: string } {
	return Boolean(value && typeof value === 'object')
}

function fencedJson(value: unknown): string {
	if (value === undefined || value === null) {
		return ''
	}
	if (typeof value === 'string') {
		return value
	}
	return `\`\`\`json\n${JSON.stringify(value, null, 2)}\n\`\`\``
}

function isMessageRole(role: unknown): role is MessageRole {
	return role === 'user' || role === 'assistant' || role === 'system' || role === 'tool'
}

function isChatMessageInput(value: ChatMessage | null): value is ChatMessage {
	return Boolean(value)
}

function kipResult<Result>(toolOutput: ToolOutput<Result>): Result {
	return kipResultWithCursor(toolOutput).result
}

function kipResultWithCursor<Result>(toolOutput: ToolOutput<Result>): {
	result: Result
	nextCursor?: string | null
} {
	const output = toolOutput && toolOutput.output
	if (output && Object.prototype.hasOwnProperty.call(output, 'result')) {
		return { result: output.result as Result, nextCursor: output.next_cursor }
	}
	if (output && (Object.prototype.hasOwnProperty.call(output, 'error') || output.Err)) {
		throw new Error(JSON.stringify(output.error || output.Err))
	}
	throw new Error('tool returned an unknown RPC response')
}

function displayedSuffixPrefixOverlap(displayed: ChatMessage[], incoming: ChatMessage[]): number {
	const maxLength = Math.min(displayed.length, incoming.length)
	for (let length = maxLength; length > 0; length -= 1) {
		const displayedSuffix = displayed.slice(displayed.length - length)
		const incomingPrefix = incoming.slice(0, length)
		if (
			displayedSuffix.every((message, index) => sameDisplayMessage(message, incomingPrefix[index]))
		) {
			return length
		}
	}
	return 0
}

function sameDisplayMessage(left: ChatMessage, right: ChatMessage | undefined): boolean {
	return Boolean(right && left.role === right.role && left.text === right.text)
}

function flattenMessages(conversationGroups: ConversationGroup[]): ChatMessage[] {
	return conversationGroups.flatMap((group) => group.messages)
}

function compareConversationGroups(left: ConversationGroup, right: ConversationGroup): number {
	return groupTime(left) - groupTime(right)
}

function groupTime(group: ConversationGroup): number {
	return group.createdAt || group.updatedAt || firstMessageTime(group.messages) || 0
}

function firstMessageTime(messages: ChatMessage[]): number {
	for (const message of messages) {
		const time = Number(message.timestamp || 0)
		if (Number.isFinite(time) && time > 0) {
			return time
		}
	}
	return 0
}

function resourceToAttachmentSummary(resource: ResourceInput): AttachmentSummary {
	return {
		id: `${resource.name}-${resource.size || 0}`,
		name: resource.name,
		type: resource.mime_type,
		size: resource.size
	}
}

function sourceStateConversationId(state: SourceState): string | null {
	return normalizeId(state.c ?? state.conv_id)
}

function normalizeId(value: unknown): string | null {
	if (typeof value === 'number' && Number.isFinite(value) && value > 0) {
		return String(value)
	}
	if (typeof value === 'string' && value.trim() && value !== '0') {
		return value.trim()
	}
	return null
}

function numericConversationId(conversationId: string): number {
	const numeric = Number(conversationId)
	if (Number.isFinite(numeric) && numeric > 0) {
		return numeric
	}
	return 0
}

function lastDefined(values: Array<string | undefined>): string | undefined {
	for (let index = values.length - 1; index >= 0; index -= 1) {
		if (values[index]) {
			return values[index]
		}
	}
	return undefined
}

async function browserSession(chrome: ChromeApi): Promise<string> {
	const saved = await chrome.storage.local.get([browserSessionStorageKey])
	let id = saved.browserSessionId || '0'
	if (parseInt(id, 10) < 1000) {
		id = Date.now().toString()
		await chrome.storage.local.set({ browserSessionId: id })
	}
	return `browser:${browserSessionScope(chrome)}:${id}`
}

function browserSessionScope(chrome: ChromeApi): string {
	return chrome.extension?.inIncognitoContext ? 'incognito' : 'chrome'
}

function normalizeSettings(settings: SettingsState): SettingsState {
	return {
		baseUrl: trimTrailingSlash(settings.baseUrl.trim() || defaultSettings.baseUrl),
		token: settings.token.trim()
	}
}

function trimTrailingSlash(value: string): string {
	return String(value || '').replace(/\/+$/, '')
}

function delay(ms: number): Promise<void> {
	return new Promise((resolve) => setTimeout(resolve, ms))
}

function errorToMessage(error: unknown): string {
	return error instanceof Error ? error.message : String(error)
}
