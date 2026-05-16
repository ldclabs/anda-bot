import { getPlainText } from '$lib/utils/markdown'

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
	incognito?: boolean
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

export interface VoiceCapabilities {
	transcription: string[]
	daemonTts: string[]
	chromeTts: boolean
}

export interface PromptSkill {
	name: string
	description?: string
}

export type VoiceProvider = 'chrome' | 'anda'

export interface VoiceRecordingInput {
	voiceProvider?: VoiceProvider
	transcript?: string
	audioBase64?: string
	fileName?: string
	mimeType?: string
	size?: number
	ttsEnabled: boolean
}

export interface ChatMessage {
	id: string
	role: MessageRole
	text: string
	thinkingText?: string
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
	voiceCapabilities: VoiceCapabilities
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
	i18n: {
		getMessage(messageName: string, substitutions?: string | string[]): string
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

interface KipToolOutput<Result> {
	output?: KipOutput<Result>
}

interface DirectToolOutput<Result> {
	output: Result
	artifacts?: ResourceInput[]
}

interface DaemonVoiceCapabilities {
	transcription?: boolean | string[]
	tts?: boolean | string[]
}

interface TranscriptionToolOutput {
	text: string
	provider: string
	file_name: string
}

interface TtsToolOutput {
	provider: string
	artifact: string
	mime_type: string
	format: string
	size: number
}

interface PageSpeechResult {
	available?: boolean
	started?: boolean
	transcript?: string
	canceled?: boolean
	error?: string
}

export interface PageAudioResult {
	available?: boolean
	started?: boolean
	audioBase64?: string
	mimeType?: string
	size?: number
	canceled?: boolean
	error?: string
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
	text?: string
	language?: string
	mimeType?: string
}

type ExtensionResponse<Result> =
	| { ok: true; result?: Result; status?: string }
	| { ok: false; error: string; status?: string }

type SnapshotListener = (snapshot: ClientSnapshot) => void

type NewPromptCommand = {
	prompt: string | null
}

const defaultSettings: SettingsState = {
	baseUrl: 'http://127.0.0.1:8042',
	token: ''
}

const browserSessionStorageKey = 'browserSessionId'
const pollingIntervalMs = 2000
const previousConversationPageSize = 8
const localConversationId = 'local-draft'
const voiceResponseTimeoutMs = 120_000
const voiceTtsChunkChars = 800
const voiceTtsShortChunkChars = 80
const voiceTtsMaxShortLines = 4

export class AndaSidePanelClient {
	private chrome: ChromeApi | null = null
	private destroyed = false
	private pollingConversation = false
	private messageCounter = 0
	private conversationParents = new Map<string, string>()
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
		voiceCapabilities: VoiceCapabilities
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
		syncing: false,
		voiceCapabilities: { transcription: [], daemonTts: [], chromeTts: false }
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
			await this.refreshVoiceCapabilities().catch(() => undefined)
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
			syncing: this.state.syncing,
			voiceCapabilities: { ...this.state.voiceCapabilities }
		}
	}

	async saveSettings(settings: SettingsState, options: { quiet?: boolean } = {}): Promise<void> {
		const chrome = this.requireChrome()
		this.state.settings = normalizeSettings(settings)
		await chrome.storage.local.set(this.state.settings)
		this.emit()
		if (!options.quiet) {
			this.appendSystemMessage(chrome.i18n.getMessage('settingsSaved'))
		}
		await this.syncServiceWorker().catch(() => undefined)
		if (this.state.settings.token) {
			void this.restoreSourceConversation()
				.then(() => this.startConversationPolling())
				.catch((error) => this.appendSystemMessage(errorToMessage(error)))
		}
		await this.refreshVoiceCapabilities().catch(() => undefined)
	}

	async testConnection(settings: SettingsState): Promise<void> {
		try {
			await this.saveSettings(settings, { quiet: true })
			await this.rpc('information', [])
			await this.refreshVoiceCapabilities().catch(() => undefined)
			this.updateStatus('connected')
			this.appendSystemMessage(chrome.i18n.getMessage('connectionTestPassed'))
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
		await this.runPrompt(prompt, attachments, { manageSending: true })
	}

	async sendVoiceTurn(recording: VoiceRecordingInput): Promise<void> {
		if (this.state.sending) {
			return
		}
		if (!this.state.settings.token) {
			this.appendSystemMessage(chrome.i18n.getMessage('pasteTokenFirst'))
			return
		}

		this.state.sending = true
		this.emit()
		try {
			await this.refreshActiveTab()
			const prompt = await this.voiceTurnPrompt(recording)
			if (!prompt) {
				this.appendSystemMessage(chrome.i18n.getMessage('noVoiceCaptured'))
				this.updateStatus('idle')
				return
			}

			const knownAssistantIds = new Set(
				flattenMessages(this.state.conversationGroups)
					.filter((message) => message.role === 'assistant')
					.map((message) => message.id)
			)
			const output = await this.runPrompt(prompt, [], { manageSending: false })
			if (!output || !recording.ttsEnabled) {
				return
			}

			const responseText =
				normalTextForSpeech(output.content) ||
				(await this.waitForNextAssistantText(knownAssistantIds))
			if (!responseText?.trim()) {
				return
			}

			this.updateStatus('speaking')
			const spokenBy = await this.speakAssistantText(
				responseText,
				recording.voiceProvider || 'chrome'
			)
			if (!spokenBy) {
				const service =
					recording.voiceProvider === 'anda'
						? chrome.i18n.getMessage('andaVoiceService')
						: chrome.i18n.getMessage('chromeVoiceService')
				this.appendSystemMessage(chrome.i18n.getMessage('playbackUnavailable') + `: ${service}`)
			}
		} catch (error) {
			this.appendSystemMessage(errorToMessage(error))
			this.updateStatus('voice failed')
		} finally {
			this.state.sending = false
			if (this.state.status === 'transcribing' || this.state.status === 'speaking') {
				this.state.status = 'idle'
			}
			this.emit()
		}
	}

	async startBrowserSpeechRecognition(language: string): Promise<void> {
		const response = await this.serviceWorkerMessage<PageSpeechResult>('anda_page_speech_start', {
			language
		})
		const result = response.result || {}
		if (result.error || result.started === false) {
			throw new Error(result.error || chrome.i18n.getMessage('browserSpeechStartFailed'))
		}
	}

	async stopBrowserSpeechRecognition(): Promise<string> {
		const response = await this.serviceWorkerMessage<PageSpeechResult>('anda_page_speech_stop')
		const result = response.result || {}
		if (result.error) {
			throw new Error(result.error)
		}
		return result.transcript?.trim() || ''
	}

	async cancelBrowserSpeechRecognition(): Promise<void> {
		await this.serviceWorkerMessage<PageSpeechResult>('anda_page_speech_cancel').catch(
			() => undefined
		)
	}

	async startBrowserAudioCapture(mimeType?: string): Promise<void> {
		const response = await this.serviceWorkerMessage<PageAudioResult>('anda_page_audio_start', {
			mimeType
		})
		const result = response.result || {}
		if (result.error || result.started === false) {
			throw new Error(result.error || chrome.i18n.getMessage('andaVoiceStartFailed'))
		}
	}

	async stopBrowserAudioCapture(): Promise<PageAudioResult> {
		const response = await this.serviceWorkerMessage<PageAudioResult>('anda_page_audio_stop')
		const result = response.result || {}
		if (result.error) {
			throw new Error(result.error)
		}
		if (!result.audioBase64 || !result.mimeType) {
			throw new Error(chrome.i18n.getMessage('noVoiceCaptured'))
		}
		return result
	}

	async cancelBrowserAudioCapture(): Promise<void> {
		await this.serviceWorkerMessage<PageAudioResult>('anda_page_audio_cancel').catch(
			() => undefined
		)
	}

	async refreshVoiceCapabilities(): Promise<VoiceCapabilities> {
		const chromeTts = await this.chromeTtsAvailable().catch(() => false)
		let next: VoiceCapabilities = { transcription: [], daemonTts: [], chromeTts }
		if (this.state.settings.token) {
			const daemon = await this.rpc<DaemonVoiceCapabilities>('capabilities', [])
			next = {
				transcription: normalizeCapabilityFormats(daemon.transcription, ['wav']),
				daemonTts: normalizeCapabilityFormats(daemon.tts, ['mp3']),
				chromeTts
			}
		}
		this.state.voiceCapabilities = next
		this.emit()
		return next
	}

	async listPromptSkills(): Promise<PromptSkill[]> {
		if (!this.state.settings.token) {
			return []
		}
		const meta = await this.requestMeta()
		const toolOutput = await this.toolCall<PromptSkill[]>(
			'anda_bot_api',
			{ type: 'ListSkills' },
			meta
		)
		return normalizePromptSkills(kipResult(toolOutput))
	}

	private async voiceTurnPrompt(recording: VoiceRecordingInput): Promise<string> {
		const transcript = recording.transcript?.trim()
		if (transcript) {
			return transcript
		}

		this.updateStatus('transcribing')
		const meta = await this.requestMeta()
		const transcription = await this.transcribeVoiceRecording(recording, meta)
		return transcription.text.trim()
	}

	private async runPrompt(
		prompt: string,
		attachments: ChatAttachment[],
		options: { manageSending: boolean }
	): Promise<AgentRunOutput | null> {
		if (!prompt && attachments.length === 0) {
			return null
		}
		if (!this.state.settings.token) {
			this.appendSystemMessage(chrome.i18n.getMessage('pasteTokenFirst'))
			return null
		}
		if (options.manageSending) {
			this.state.sending = true
			this.emit()
		}

		await this.refreshActiveTab()
		const resources = attachments.map((attachment) => attachment.resource)
		const effectivePrompt = prompt || 'Please review the attached files.'
		const newCommand = parseNewPromptCommand(prompt)

		try {
			const meta = await this.requestMeta()
			if (newCommand) {
				this.clearConversationDisplay()
				if (newCommand.prompt) {
					this.appendMessage(
						{
							role: 'user',
							text: newCommand.prompt,
							local: true,
							attachments: attachments.map(({ resource: _resource, ...attachment }) => attachment)
						},
						localConversationId
					)
				}
			} else {
				this.appendMessage(
					{
						role: 'user',
						text: effectivePrompt,
						local: true,
						attachments: attachments.map(({ resource: _resource, ...attachment }) => attachment)
					},
					this.state.conversationId || localConversationId
				)
			}
			this.updateStatus('sending')
			const output = await this.agentRun({ name: '', prompt: effectivePrompt, resources, meta })
			const outputConversationId = normalizeId(output.conversation)
			if (newCommand && !newCommand.prompt) {
				this.state.conversationId = null
				this.state.messageOffset = 0
				this.state.artifactOffset = 0
				this.updateStatus('ready')
			} else if (outputConversationId) {
				this.promoteLocalConversation(outputConversationId)
				if (this.state.conversationId !== outputConversationId) {
					this.state.messageOffset = 0
					this.state.artifactOffset = 0
				}
				this.state.conversationId = outputConversationId
			}
			if (output.content && output.content.trim()) {
				const content = splitLegacyThoughtText(output.content)
				this.appendMessage(
					{
						role: 'assistant',
						text: content.text,
						thinkingText: content.thinkingText || undefined
					},
					this.state.conversationId
				)
			}
			if (output.failed_reason) {
				this.appendConversationFailureMessage(
					this.state.conversationId || localConversationId,
					output.failed_reason
				)
			}
			this.startConversationPolling()
			return output
		} catch (error) {
			this.appendSystemMessage(errorToMessage(error))
			this.updateStatus('request failed')
			return null
		} finally {
			if (options.manageSending) {
				this.state.sending = false
				this.emit()
			}
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
	): Promise<KipToolOutput<Result>> {
		return this.rpc<KipToolOutput<Result>>('tool_call', [{ name, args, meta }])
	}

	private async directToolCall<Result>(
		name: string,
		args: Record<string, unknown>,
		meta: RequestMeta,
		resources: ResourceInput[] = []
	): Promise<DirectToolOutput<Result>> {
		const payload = resources.length ? { name, args, resources, meta } : { name, args, meta }
		return this.rpc<DirectToolOutput<Result>>('tool_call', [payload])
	}

	private async transcribeVoiceRecording(
		recording: VoiceRecordingInput,
		meta: RequestMeta
	): Promise<TranscriptionToolOutput> {
		if (this.state.voiceCapabilities.transcription.length === 0) {
			await this.refreshVoiceCapabilities()
		}
		if (this.state.voiceCapabilities.transcription.length === 0) {
			throw new Error(chrome.i18n.getMessage('voiceTranscriptionNotConfigured'))
		}
		if (!recording.audioBase64 || !recording.fileName) {
			throw new Error(chrome.i18n.getMessage('audioCaptureMissingData'))
		}
		const normalizedRecording = await normalizeVoiceRecordingAudio(
			recording,
			this.state.voiceCapabilities.transcription
		)
		const result = await this.directToolCall<TranscriptionToolOutput>(
			'transcribe_audio',
			{
				file_name: normalizedRecording.fileName,
				audio_base64: normalizedRecording.audioBase64
			},
			meta
		)
		return result.output
	}

	private async speakAssistantText(
		text: string,
		preferredProvider: VoiceProvider
	): Promise<'chrome' | 'anda' | null> {
		const speechText = prepareVoiceTtsText(text)
		const chunks = splitVoiceTtsText(speechText, voiceTtsChunkChars)
		if (!chunks.length) {
			return null
		}

		if (preferredProvider === 'anda') {
			return (await this.trySpeakWithAndaTts(chunks)) ? 'anda' : null
		}
		return (await this.trySpeakWithChromeTts(chunks)) ? 'chrome' : null
	}

	private async trySpeakWithChromeTts(chunks: string[]): Promise<boolean> {
		if (!this.state.voiceCapabilities.chromeTts) {
			await this.refreshVoiceCapabilities().catch(() => undefined)
		}
		if (!this.state.voiceCapabilities.chromeTts) {
			return false
		}
		try {
			for (const chunk of chunks) {
				await this.speakWithChromeTts(chunk)
			}
			return true
		} catch (_error) {
			await this.serviceWorkerMessage('anda_chrome_tts_stop').catch(() => undefined)
			return false
		}
	}

	private async trySpeakWithAndaTts(chunks: string[]): Promise<boolean> {
		if (this.state.voiceCapabilities.daemonTts.length === 0) {
			await this.refreshVoiceCapabilities().catch(() => undefined)
		}
		if (this.state.voiceCapabilities.daemonTts.length === 0) {
			return false
		}

		const meta = await this.requestMeta()
		try {
			for (const [index, chunk] of chunks.entries()) {
				const result = await this.directToolCall<TtsToolOutput>(
					'synthesize_speech',
					{
						text: chunk,
						artifact_name: `anda_chrome_voice_${Date.now()}_${index + 1}`
					},
					meta
				)
				const artifact = result.artifacts?.find(isAudioResource)
				if (!artifact?.blob) {
					throw new Error('Anda TTS did not return playable audio.')
				}
				await playAudioArtifact(artifact)
			}
			return true
		} catch (_error) {
			return false
		}
	}

	private async speakWithChromeTts(text: string): Promise<void> {
		await this.serviceWorkerMessage('anda_chrome_tts_speak', { text })
	}

	private async chromeTtsAvailable(): Promise<boolean> {
		const response = await this.serviceWorkerMessage<{ available?: boolean }>(
			'anda_chrome_tts_available'
		)
		return Boolean(response.result?.available)
	}

	private async waitForNextAssistantText(knownIds: Set<string>): Promise<string | null> {
		const startedAt = Date.now()
		while (!this.destroyed && Date.now() - startedAt < voiceResponseTimeoutMs) {
			const message = flattenMessages(this.state.conversationGroups).find(
				(message) =>
					message.role === 'assistant' && !knownIds.has(message.id) && Boolean(message.text.trim())
			)
			if (message) {
				return message.text
			}
			this.startConversationPolling()
			await delay(300)
		}
		return null
	}

	private async rpc<Result>(method: string, tupleArgs: unknown[]): Promise<Result> {
		if (!this.state.settings.token) {
			throw new Error(chrome.i18n.getMessage('tokenMissing'))
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
			throw new Error(response?.error || chrome.i18n.getMessage('extensionError'))
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

			let failedReason = delta.failed_reason?.trim() || ''
			if (delta.status === 'failed' && !failedReason) {
				const failedConversation = await this.fetchConversation(conversationId).catch(() => null)
				failedReason = failedConversation?.failed_reason?.trim() || ''
			}
			if (delta.status === 'failed' || failedReason) {
				this.appendConversationFailureMessage(conversationId, failedReason, delta.updated_at)
			}

			const childId = normalizeId(delta.child)
			if (childId && childId !== this.state.conversationId) {
				this.conversationParents.set(childId, conversationId)
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
			extra.tab = {
				id: this.state.tab.id,
				url: this.state.tab.url,
				title: this.state.tab.title,
				incognito: this.state.tab.incognito
			}
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
		const groups = compactConversationChainGroups(conversations.map(conversationToGroup))
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
		this.state.conversationId = normalizeId(latestConversation?._id) || latestGroup?.id
		this.state.messageOffset =
			latestConversation?.messages?.length || latestGroup?.messages.length || 0
		this.state.artifactOffset = latestConversation?.artifacts?.length || 0
		this.emit()
	}

	private clearConversationDisplay(): void {
		this.state.conversationId = null
		this.state.conversationGroups = []
		this.state.messageOffset = 0
		this.state.artifactOffset = 0
		this.state.previousCursor = undefined
		this.conversationParents.clear()
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
		const existing = this.state.conversationGroups.find((group) => group.id === conversationId)
		const incomingMessages = existing?.messages.length
			? incoming
			: this.withoutParentDuplicatePrompt(conversationId, incoming)
		if (!incomingMessages.length && !status && !updatedAt) {
			return
		}
		const group = existing || this.ensureConversationGroup(conversationId)
		if (status) {
			group.status = status
		}
		if (updatedAt) {
			group.updatedAt = updatedAt
		}
		const overlap = displayedSuffixPrefixOverlap(group.messages, incomingMessages)
		for (const message of incomingMessages.slice(overlap)) {
			group.messages = [...group.messages, message]
		}
		this.emit()
	}

	private withoutParentDuplicatePrompt(
		conversationId: string,
		incoming: ChatMessage[]
	): ChatMessage[] {
		const parentId = this.conversationParents.get(conversationId)
		const parent = parentId
			? this.state.conversationGroups.find((group) => group.id === parentId)
			: undefined
		const parentTail = parent?.messages[parent.messages.length - 1]
		if (parentTail && sameUserPromptMessage(parentTail, incoming[0])) {
			return incoming.slice(1)
		}
		return incoming
	}

	private appendSystemMessage(text: string): void {
		this.appendMessage({ role: 'system', text }, this.state.conversationId || localConversationId)
	}

	private appendConversationFailureMessage(
		conversationId: string,
		reason?: string | null,
		timestamp?: string | number | null
	): void {
		const group = this.ensureConversationGroup(conversationId || localConversationId)
		const text = failureReasonMessage(reason)
		const id = `${group.id}-failed`
		if (group.messages.some((message) => message.id === id || message.text === text)) {
			return
		}
		group.messages = [
			...group.messages,
			{
				id,
				conversationId: group.id,
				role: 'system',
				text,
				timestamp: timestamp ?? Date.now()
			}
		]
		this.emit()
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

export function getChromeApi(): ChromeApi {
	const chromeApi = (globalThis as typeof globalThis & { chrome?: ChromeApi }).chrome
	if (!chromeApi?.runtime || !chromeApi.storage?.local || !chromeApi.tabs || !chromeApi.i18n) {
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

	if (conversation.status === 'failed') {
		const text = failureReasonMessage(conversation.failed_reason)
		if (!messages.some((message) => message.role === 'system' && message.text === text)) {
			messages.push({
				id: `${conversationId}-failed`,
				conversationId,
				role: 'system',
				text,
				timestamp: conversation.updated_at || Date.now()
			})
		}
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

function compactConversationChainGroups(groups: ConversationGroup[]): ConversationGroup[] {
	const compacted: ConversationGroup[] = []
	let previousTail: ChatMessage | null = null

	for (const group of groups) {
		let messages = group.messages
		if (previousTail && sameUserPromptMessage(previousTail, messages[0])) {
			messages = messages.slice(1)
		}
		if (!messages.length) {
			continue
		}
		compacted.push({ ...group, messages })
		previousTail = messages[messages.length - 1]
	}

	return compacted
}

function failureReasonMessage(reason?: string | null): string {
	const trimmed = reason?.trim()
	return trimmed
		? chrome.i18n.getMessage('conversationFailed', [trimmed])
		: chrome.i18n.getMessage('conversationFailedNoReason')
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
	const content = contentToMessageContent(raw.content)
	if (!content.text && !content.thinkingText) {
		return null
	}
	return {
		id: `${context.conversationId}-${context.index}`,
		conversationId: context.conversationId,
		role: raw.role,
		text: content.text,
		thinkingText: content.thinkingText || undefined,
		timestamp: raw.timestamp || context.fallbackTimestamp || null,
		attachments: raw.resources?.map(resourceToAttachmentSummary)
	}
}

function contentToMessageContent(content: unknown): { text: string; thinkingText: string } {
	if (typeof content === 'string') {
		return splitLegacyThoughtText(content)
	}
	if (!Array.isArray(content)) {
		return { text: '', thinkingText: '' }
	}
	const textParts: string[] = []
	const thinkingParts: string[] = []
	for (const part of content) {
		if (typeof part === 'string') {
			const split = splitLegacyThoughtText(part)
			if (split.text) {
				textParts.push(split.text)
			}
			if (split.thinkingText) {
				thinkingParts.push(split.thinkingText)
			}
			continue
		}
		if (!isContentPart(part)) {
			continue
		}
		if (part.type === 'Text' && typeof part.text === 'string') {
			const split = splitLegacyThoughtText(part.text)
			if (split.text) {
				textParts.push(split.text)
			}
			if (split.thinkingText) {
				thinkingParts.push(split.thinkingText)
			}
			continue
		}
		if (part.type === 'Reasoning' && typeof part.text === 'string') {
			thinkingParts.push(part.text)
			continue
		}
		if (part.type === 'ToolOutput') {
			thinkingParts.push(formatToolDetail('Tool output', part.output))
			continue
		}
		if (part.type === 'ToolCall') {
			thinkingParts.push(
				formatToolDetail(`Tool call${part.name ? `: ${part.name}` : ''}`, part.args)
			)
			continue
		}
	}
	return {
		text: textParts.filter(Boolean).join('\n\n').trim(),
		thinkingText: thinkingParts.filter(Boolean).join('\n\n').trim()
	}
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

function formatToolDetail(title: string, value: unknown): string {
	const body = fencedJson(value)
	return body ? `**${title}**\n\n${body}` : `**${title}**`
}

function splitLegacyThoughtText(content: string): { text: string; thinkingText: string } {
	const thinkingParts: string[] = []
	const text = content
		.replace(/<think(?:ing)?\b[^>]*>([\s\S]*?)<\/think(?:ing)?>/gi, (_match, thinking) => {
			if (typeof thinking === 'string' && thinking.trim()) {
				thinkingParts.push(thinking.trim())
			}
			return ''
		})
		.trim()
	return { text, thinkingText: thinkingParts.join('\n\n').trim() }
}

function isMessageRole(role: unknown): role is MessageRole {
	return role === 'user' || role === 'assistant' || role === 'system' || role === 'tool'
}

function isChatMessageInput(value: ChatMessage | null): value is ChatMessage {
	return Boolean(value)
}

function kipResult<Result>(toolOutput: KipToolOutput<Result>): Result {
	return kipResultWithCursor(toolOutput).result
}

function kipResultWithCursor<Result>(toolOutput: KipToolOutput<Result>): {
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
	throw new Error(chrome.i18n.getMessage('rpcError'))
}

function normalizePromptSkills(skills: PromptSkill[] | undefined): PromptSkill[] {
	return (skills || [])
		.filter((skill) => typeof skill?.name === 'string' && Boolean(skill.name.trim()))
		.map((skill) => ({
			name: skill.name.trim(),
			description: skill.description?.trim() || undefined
		}))
		.sort((left, right) => left.name.localeCompare(right.name))
}

type NormalizedVoiceRecording = {
	audioBase64: string
	fileName: string
}

function normalizeCapabilityFormats(
	value: boolean | string[] | undefined,
	legacyFallback: string[]
): string[] {
	if (Array.isArray(value)) {
		return normalizeAudioFormats(value)
	}
	return value ? normalizeAudioFormats(legacyFallback) : []
}

async function normalizeVoiceRecordingAudio(
	recording: VoiceRecordingInput,
	acceptedFormats: string[]
): Promise<NormalizedVoiceRecording> {
	const audioBase64 = recording.audioBase64 || ''
	const fileName = recording.fileName || 'chrome_voice.webm'
	const accepted = normalizeAudioFormats(acceptedFormats)
	const sourceFormat = recordingAudioFormat(fileName, recording.mimeType)
	if (!sourceFormat || accepted.includes(sourceFormat)) {
		return { audioBase64, fileName }
	}
	if (!accepted.includes('wav')) {
		throw new Error(
			chrome.i18n.getMessage('audioFormatNotSupported', [
				sourceFormat,
				accepted.join(', ') || 'none'
			])
		)
	}

	const mimeType = recording.mimeType || audioMimeFromName(fileName) || 'audio/webm'
	const sourceBytes = base64ToUint8Array(audioBase64)
	const sourceBlob = new Blob([sourceBytes], { type: mimeType })
	const wavBytes = await audioBlobToWavBytes(sourceBlob)
	return {
		audioBase64: uint8ArrayToBase64(wavBytes),
		fileName: `${fileStem(fileName) || 'chrome_voice'}.wav`
	}
}

function normalizeAudioFormats(formats: string[]): string[] {
	const seen = new Set<string>()
	const normalized: string[] = []
	for (const format of formats) {
		const next = normalizeAudioFormat(format)
		if (next && !seen.has(next)) {
			seen.add(next)
			normalized.push(next)
		}
	}
	return normalized
}

function recordingAudioFormat(fileName: string, mimeType?: string): string {
	return normalizeAudioFormat(mimeType || '') || normalizeAudioFormat(fileExtension(fileName))
}

function normalizeAudioFormat(format: string): string {
	const normalized = format.trim().toLowerCase().split(';', 1)[0]
	switch (normalized) {
		case 'audio/webm':
		case 'webm':
			return 'webm'
		case 'audio/ogg':
		case 'audio/oga':
		case 'oga':
		case 'ogg':
			return 'ogg'
		case 'audio/mp4':
		case 'mp4':
			return 'mp4'
		case 'audio/x-m4a':
		case 'm4a':
			return 'm4a'
		case 'audio/mpeg':
		case 'audio/mp3':
		case 'mpeg':
		case 'mpga':
		case 'mp3':
			return 'mp3'
		case 'audio/wav':
		case 'audio/x-wav':
		case 'wav':
			return 'wav'
		case 'audio/flac':
		case 'flac':
			return 'flac'
		case 'audio/opus':
		case 'opus':
			return 'opus'
		case 'audio/pcm':
		case 'audio/l16':
		case 'pcm':
			return 'pcm'
		default:
			return ''
	}
}

async function audioBlobToWavBytes(blob: Blob): Promise<Uint8Array> {
	const AudioContextCtor = globalThis.AudioContext
	if (!AudioContextCtor) {
		throw new Error(chrome.i18n.getMessage('audioConversionFailed'))
	}
	const context = new AudioContextCtor()
	try {
		const audioBuffer = await context.decodeAudioData(await blob.arrayBuffer())
		return audioBufferToWavBytes(audioBuffer)
	} catch (error) {
		throw new Error(
			chrome.i18n.getMessage('wavConversionFailed', [
				error instanceof Error ? error.message : String(error)
			])
		)
	} finally {
		await context.close().catch(() => undefined)
	}
}

function audioBufferToWavBytes(audioBuffer: AudioBuffer): Uint8Array<ArrayBuffer> {
	const channelCount = Math.min(audioBuffer.numberOfChannels || 1, 2)
	const sampleRate = audioBuffer.sampleRate
	const bytesPerSample = 2
	const blockAlign = channelCount * bytesPerSample
	const dataSize = audioBuffer.length * blockAlign
	const bytes = new Uint8Array(44 + dataSize)
	const view = new DataView(bytes.buffer)
	let offset = 0

	const writeString = (value: string) => {
		for (let index = 0; index < value.length; index += 1) {
			view.setUint8(offset, value.charCodeAt(index))
			offset += 1
		}
	}

	writeString('RIFF')
	view.setUint32(offset, 36 + dataSize, true)
	offset += 4
	writeString('WAVE')
	writeString('fmt ')
	view.setUint32(offset, 16, true)
	offset += 4
	view.setUint16(offset, 1, true)
	offset += 2
	view.setUint16(offset, channelCount, true)
	offset += 2
	view.setUint32(offset, sampleRate, true)
	offset += 4
	view.setUint32(offset, sampleRate * blockAlign, true)
	offset += 4
	view.setUint16(offset, blockAlign, true)
	offset += 2
	view.setUint16(offset, bytesPerSample * 8, true)
	offset += 2
	writeString('data')
	view.setUint32(offset, dataSize, true)
	offset += 4

	const channels = Array.from({ length: channelCount }, (_unused, index) =>
		audioBuffer.getChannelData(index)
	)
	for (let sampleIndex = 0; sampleIndex < audioBuffer.length; sampleIndex += 1) {
		for (let channelIndex = 0; channelIndex < channelCount; channelIndex += 1) {
			const sample = Math.max(-1, Math.min(1, channels[channelIndex][sampleIndex] || 0))
			view.setInt16(offset, sample < 0 ? sample * 0x8000 : sample * 0x7fff, true)
			offset += bytesPerSample
		}
	}

	return bytes
}

function base64ToUint8Array(base64: string): Uint8Array<ArrayBuffer> {
	const binary = atob(base64)
	const bytes = new Uint8Array(binary.length)
	for (let index = 0; index < binary.length; index += 1) {
		bytes[index] = binary.charCodeAt(index)
	}
	return bytes
}

function uint8ArrayToBase64(bytes: Uint8Array): string {
	const chunkSize = 0x8000
	let binary = ''
	for (let offset = 0; offset < bytes.length; offset += chunkSize) {
		binary += String.fromCharCode(...bytes.subarray(offset, offset + chunkSize))
	}
	return btoa(binary)
}

function fileExtension(fileName: string): string {
	return fileName.includes('.') ? fileName.split('.').pop()?.toLowerCase() || '' : ''
}

function fileStem(fileName: string): string {
	return fileName.includes('.') ? fileName.slice(0, fileName.lastIndexOf('.')) : fileName
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
	return Boolean(
		right &&
		left.role === right.role &&
		left.text === right.text &&
		(left.thinkingText || '') === (right.thinkingText || '') &&
		sameAttachmentSummaries(left.attachments, right.attachments)
	)
}

function sameUserPromptMessage(
	left: ChatMessage | undefined,
	right: ChatMessage | undefined
): boolean {
	return Boolean(
		left &&
		right &&
		left.role === 'user' &&
		right.role === 'user' &&
		sameDisplayMessage(left, right)
	)
}

function sameAttachmentSummaries(
	left: AttachmentSummary[] | undefined,
	right: AttachmentSummary[] | undefined
): boolean {
	const leftAttachments = left || []
	const rightAttachments = right || []
	return (
		leftAttachments.length === rightAttachments.length &&
		leftAttachments.every((attachment, index) => {
			const other = rightAttachments[index]
			return (
				attachment.name === other?.name &&
				attachment.type === other?.type &&
				(attachment.size || 0) === (other?.size || 0)
			)
		})
	)
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

function isAudioResource(resource: ResourceInput): boolean {
	return (
		resource.tags.some((tag) => tag.toLowerCase() === 'audio') ||
		Boolean(resource.mime_type?.toLowerCase().startsWith('audio/'))
	)
}

async function playAudioArtifact(resource: ResourceInput): Promise<void> {
	if (!resource.blob) {
		throw new Error('Audio artifact is missing inline data.')
	}
	const mimeType = resource.mime_type || audioMimeFromName(resource.name) || 'audio/mpeg'
	const audio = new Audio(`data:${mimeType};base64,${resource.blob}`)
	await new Promise<void>((resolve, reject) => {
		let settled = false
		const settle = (error?: unknown) => {
			if (settled) {
				return
			}
			settled = true
			if (error) {
				reject(error instanceof Error ? error : new Error(String(error)))
			} else {
				resolve()
			}
		}
		audio.onended = () => settle()
		audio.onerror = () => settle(new Error(chrome.i18n.getMessage('audioPlaybackFailed')))
		void audio.play().catch(settle)
	})
}

function audioMimeFromName(name: string): string | null {
	const extension = name.includes('.') ? name.split('.').pop()?.toLowerCase() : ''
	switch (extension) {
		case 'flac':
			return 'audio/flac'
		case 'm4a':
		case 'mp4':
			return 'audio/mp4'
		case 'ogg':
		case 'oga':
			return 'audio/ogg'
		case 'opus':
			return 'audio/opus'
		case 'wav':
			return 'audio/wav'
		case 'webm':
			return 'audio/webm'
		case 'mp3':
		case 'mpeg':
		case 'mpga':
			return 'audio/mpeg'
		default:
			return null
	}
}

function prepareVoiceTtsText(text: string): string {
	return text
		.split(/\r?\n/)
		.map(getPlainText)
		.map((line) =>
			Array.from(line)
				.map(normalizeVoiceTtsCharacter)
				.filter((character): character is string => Boolean(character))
				.join('')
				.replace(/\s+/g, ' ')
				.trim()
		)
		.filter(Boolean)
		.join('\n')
}

function normalTextForSpeech(text: string | undefined): string {
	return text ? splitLegacyThoughtText(text).text.trim() : ''
}

function normalizeVoiceTtsCharacter(character: string): string | null {
	const codePoint = character.codePointAt(0) || 0
	if (
		codePoint === 0x200d ||
		codePoint === 0xfe0e ||
		codePoint === 0xfe0f ||
		(codePoint >= 0x2600 && codePoint <= 0x27bf) ||
		(codePoint >= 0x1f000 && codePoint <= 0x1faff) ||
		(codePoint >= 0xe0020 && codePoint <= 0xe007f)
	) {
		return null
	}
	if (character === '`' || character === '*' || character === '_' || character === '#') {
		return null
	}
	if (character === '\r' || character === '\u00a0') {
		return ' '
	}
	if (codePoint === 0x2013 || codePoint === 0x2014) {
		return ','
	}
	return character
}

function splitVoiceTtsText(text: string, maxChars: number): string[] {
	if (maxChars <= 0) {
		return []
	}
	const chunks: string[] = []
	let currentLines: string[] = []
	let currentChars = 0
	for (const line of text
		.split(/\r?\n/)
		.map((value) => value.trim())
		.filter(Boolean)) {
		const lineChars = Array.from(line).length
		if (lineChars > maxChars) {
			pushVoiceTtsLines(chunks, currentLines)
			currentLines = []
			currentChars = 0
			chunks.push(...splitLongVoiceTtsLine(line, maxChars))
			continue
		}

		const separatorChars = currentLines.length ? 1 : 0
		const nextChars = currentChars + separatorChars + lineChars
		if (
			currentLines.length &&
			(nextChars > maxChars ||
				currentLines.length >= voiceTtsMaxShortLines ||
				(currentLines.length >= 2 && currentChars >= voiceTtsShortChunkChars))
		) {
			pushVoiceTtsLines(chunks, currentLines)
			currentLines = []
			currentChars = 0
		}

		currentChars += (currentLines.length ? 1 : 0) + lineChars
		currentLines.push(line)
	}
	pushVoiceTtsLines(chunks, currentLines)
	return chunks
}

function pushVoiceTtsLines(chunks: string[], lines: string[]): void {
	if (lines.length) {
		chunks.push(lines.join('\n'))
	}
}

function splitLongVoiceTtsLine(line: string, maxChars: number): string[] {
	const chunks: string[] = []
	let current = ''
	for (const character of Array.from(line)) {
		if (Array.from(current).length >= maxChars) {
			chunks.push(current.trim())
			current = ''
		}
		current += character
		if (isTtsSentenceBoundary(character) && Array.from(current).length >= maxChars / 2) {
			chunks.push(current.trim())
			current = ''
		}
	}
	if (current.trim()) {
		chunks.push(current.trim())
	}
	return chunks
}

function isTtsSentenceBoundary(character: string): boolean {
	return ['.', '!', '?', '。', '！', '？'].includes(character)
}

function sourceStateConversationId(state: SourceState): string | null {
	return normalizeId(state.c ?? state.conv_id)
}

function parseNewPromptCommand(prompt: string): NewPromptCommand | null {
	const trimmed = prompt.trim()
	if (!trimmed.startsWith('/')) {
		return null
	}

	const body = trimmed.slice(1)
	const commandEnd = body.search(/\s/)
	const command = (commandEnd === -1 ? body : body.slice(0, commandEnd)).toLowerCase()
	if (command !== 'new' && command !== 'clear') {
		return null
	}

	const rest = commandEnd === -1 ? '' : body.slice(commandEnd).trim()
	return { prompt: rest || null }
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
