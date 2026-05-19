import {
	browserSession,
	defaultSettings,
	errorToError,
	errorToMessage,
	normalizeSettings
} from '$lib/service-worker/settings'
import { SvelteMap } from 'svelte/reactivity'
import { Channel } from './channel.svelte'
import { getChromeApi } from './chrome'
import { normalizePromptSkills } from './helper'
import type {
	ChatAttachment,
	ChromeApi,
	ChromeTabChangeInfo,
	ChromeTabInfo,
	DaemonVoiceCapabilities,
	ExtensionMessage,
	ExtensionResponse,
	PageAudioResult,
	PageSpeechResult,
	PromptSkill,
	Resource,
	RpcOutput,
	SettingsState,
	ToolOutput,
	TranscriptionToolOutput,
	TtsToolOutput,
	VoiceCapabilities,
	VoiceProvider,
	VoiceRecordingInput
} from './types'
import {
	isAudioResource,
	normalTextForSpeech,
	normalizeCapabilityFormats,
	normalizeVoiceRecordingAudio,
	playAudioArtifact,
	prepareVoiceTtsText,
	splitVoiceTtsText,
	voiceTtsChunkChars
} from './voice'

export class AndaSidePanelClient extends EventTarget {
	readonly chrome: ChromeApi

	settings: SettingsState = $state({ ...defaultSettings })
	tab: ChromeTabInfo | null = $state<ChromeTabInfo | null>(null)
	sending = $state(false)
	activeChannel = $state<Channel | null>(null)
	channels = new SvelteMap()
	status = $state('starting')
	systemMessage = $state<{ kind: 'info' | 'error'; text: string } | null>(null)
	voiceCapabilities = $state<VoiceCapabilities>({
		transcription: [],
		daemonTts: [],
		chromeTts: false
	})

	#initPromise: Promise<void> | null = null
	#tabActivatedListener?: (activeInfo: { tabId: number; windowId: number }) => void
	#tabUpdatedListener?: (tabId: number, changeInfo: ChromeTabChangeInfo, tab: ChromeTabInfo) => void

	constructor() {
		super()
		this.chrome = getChromeApi()
	}

	async init(): Promise<void> {
		if (!this.#initPromise) {
			this.#initPromise = this.#init()
			;(globalThis as any).__andaClient = this
		}
		return this.#initPromise
	}

	async #init(): Promise<void> {
		await this.loadSettings()
		const localChannel = await browserSession(this.chrome)
		const channel = new Channel(localChannel, this)
		this.activeChannel = channel
		this.channels.set(localChannel, channel)

		channel.addEventListener('ChannelInitialized', (event) => {
			const detail = (event as CustomEvent<{ source: string }>).detail
			if (detail.source === this.activeChannel?.source) {
				this.dispatchEvent(new CustomEvent('ChannelInitialized', { detail }))
			}
		})

		this.bindChromeEvents()
		await this.refreshActiveTab()
		this.updateStatus('ready', null)
		this.syncServiceWorker().catch(() => undefined)

		if (this.settings.token) {
			await this.refreshVoiceCapabilities().catch(() => undefined)
			await channel.init().catch(() => undefined)
		}
	}

	destroy(): void {
		if (this.chrome && this.#tabActivatedListener) {
			this.chrome.tabs.onActivated.removeListener(this.#tabActivatedListener)
		}
		if (this.chrome && this.#tabUpdatedListener) {
			this.chrome.tabs.onUpdated.removeListener(this.#tabUpdatedListener)
		}
		console.warn('AndaSidePanelClient destroyed')
	}

	async saveSettings(settings: SettingsState, options: { quiet?: boolean } = {}): Promise<void> {
		this.settings = normalizeSettings(settings)
		await this.chrome.storage.local.set(this.settings)
		if (!options.quiet) {
			this.systemMessage = { kind: 'info', text: chrome.i18n.getMessage('settingsSaved') }
		}
		await this.syncServiceWorker().catch(() => undefined)
		if (this.settings.token) {
			this.activeChannel?.init().catch(() => undefined)
		}
		await this.refreshVoiceCapabilities().catch(() => undefined)
	}

	async testConnection(settings: SettingsState): Promise<void> {
		try {
			await this.saveSettings(settings, { quiet: true })
			await this.rpc('information', [])
			this.updateStatus('connected', {
				kind: 'info',
				text: chrome.i18n.getMessage('connectionTestPassed')
			})
		} catch (error) {
			this.updateStatus('connection failed', { kind: 'error', text: errorToMessage(error) })
		}
	}

	async sendPrompt(text: string, attachments: ChatAttachment[] = []): Promise<void> {
		const prompt = text.trim()
		const channel = this.activeChannel
		if ((!prompt && attachments.length === 0) || this.sending || !channel) {
			return
		}

		if (!this.settings.token) {
			this.systemMessage = { kind: 'error', text: chrome.i18n.getMessage('pasteTokenFirst') }
			return
		}

		this.sending = true
		try {
			await this.refreshActiveTab()
			await channel.sendPrompt(prompt, attachments)
		} catch (error) {
			this.updateStatus('send failed', { kind: 'error', text: errorToMessage(error) })
		} finally {
			this.sending = false
		}
	}

	async sendVoiceTurn(recording: VoiceRecordingInput): Promise<void> {
		const channel = this.activeChannel
		if (this.sending || !channel) {
			return
		}

		if (!this.settings.token) {
			this.systemMessage = { kind: 'error', text: chrome.i18n.getMessage('pasteTokenFirst') }
			return
		}

		this.sending = true
		try {
			const prompt = await this.voiceTurnPrompt(recording)
			if (!prompt) {
				this.updateStatus('idle', {
					kind: 'error',
					text: chrome.i18n.getMessage('noVoiceCaptured')
				})
				return
			}

			await this.refreshActiveTab()
			const poller = await channel.sendPrompt(prompt, [])
			if (!poller || !recording.ttsEnabled) {
				return
			}

			for await (const message of poller) {
				let responseText = normalTextForSpeech(message?.text)
				if (!responseText?.trim()) {
					return
				}

				this.updateStatus('speaking', null)
				const spokenBy = await this.speakAssistantText(
					responseText,
					recording.voiceProvider || 'chrome'
				)
				if (!spokenBy) {
					const service =
						recording.voiceProvider === 'anda'
							? chrome.i18n.getMessage('andaVoiceService')
							: chrome.i18n.getMessage('chromeVoiceService')
					this.updateStatus('playback failed', {
						kind: 'error',
						text: chrome.i18n.getMessage('playbackUnavailable') + `: ${service}`
					})
					return
				}
			}
		} catch (error) {
			this.updateStatus('voice failed', { kind: 'error', text: errorToMessage(error) })
		} finally {
			this.sending = false
			if (this.status === 'transcribing' || this.status === 'speaking') {
				this.updateStatus('idle', null)
			}
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
		if (this.settings.token) {
			const daemon = await this.rpc<DaemonVoiceCapabilities>('capabilities', [])
			next = {
				transcription: normalizeCapabilityFormats(daemon.transcription, ['wav']),
				daemonTts: normalizeCapabilityFormats(daemon.tts, ['mp3']),
				chromeTts
			}
		}
		this.voiceCapabilities = next
		return next
	}

	async listPromptSkills(): Promise<PromptSkill[]> {
		if (!this.settings.token) {
			return []
		}
		const {
			output: { result }
		} = await this.toolCall<RpcOutput<PromptSkill[]>>('anda_bot_api', {
			type: 'ListSkills'
		})
		return normalizePromptSkills(result)
	}

	private async voiceTurnPrompt(recording: VoiceRecordingInput): Promise<string> {
		const transcript = recording.transcript?.trim()
		if (transcript) {
			return transcript
		}

		this.updateStatus('transcribing', null)
		const transcription = await this.transcribeVoiceRecording(recording)
		return transcription.text.trim()
	}

	private bindChromeEvents(): void {
		this.#tabActivatedListener = () => {
			this.refreshActiveTab().catch(() => undefined)
		}
		this.#tabUpdatedListener = (tabId, changeInfo, tab) => {
			if (
				!this.tab ||
				(this.tab && tabId === this.tab.id && (changeInfo.title || changeInfo.url))
			) {
				this.tab = { ...this.tab, ...tab }
				this.registerBrowserSession().catch(() => undefined)
			}
		}
		this.chrome.tabs.onActivated.addListener(this.#tabActivatedListener)
		this.chrome.tabs.onUpdated.addListener(this.#tabUpdatedListener)
	}

	private async loadSettings(): Promise<void> {
		const saved = await this.chrome.storage.local.get(['baseUrl', 'token', 'submitKeyMode'])
		this.settings = normalizeSettings({
			baseUrl: saved.baseUrl || defaultSettings.baseUrl,
			token: saved.token || '',
			submitKeyMode: saved.submitKeyMode || defaultSettings.submitKeyMode
		})
	}

	private async refreshActiveTab(): Promise<ChromeTabInfo | null> {
		const [tab] = await this.chrome.tabs.query({ active: true, lastFocusedWindow: true })
		this.tab = tab || null
		return tab || null
	}

	private async toolCall<Result>(
		name: string,
		args: Record<string, unknown>,
		resources: Resource[] = []
	): Promise<ToolOutput<Result>> {
		const rt = await this.rpc<ToolOutput<Result>>('tool_call', [{ name, args, resources }])
		const error = (rt.output as any).error
		if (error != null) {
			throw errorToError(error)
		}
		return rt
	}

	private async transcribeVoiceRecording(
		recording: VoiceRecordingInput
	): Promise<TranscriptionToolOutput> {
		if (this.voiceCapabilities.transcription.length === 0) {
			await this.refreshVoiceCapabilities()
		}
		if (this.voiceCapabilities.transcription.length === 0) {
			throw new Error(chrome.i18n.getMessage('voiceTranscriptionNotConfigured'))
		}
		if (!recording.audioBase64 || !recording.fileName) {
			throw new Error(chrome.i18n.getMessage('audioCaptureMissingData'))
		}
		const normalizedRecording = await normalizeVoiceRecordingAudio(
			recording,
			this.voiceCapabilities.transcription
		)
		const { output } = await this.toolCall<TranscriptionToolOutput>('transcribe_audio', {
			file_name: normalizedRecording.fileName,
			audio_base64: normalizedRecording.audioBase64
		})
		return output
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
		if (!this.voiceCapabilities.chromeTts) {
			await this.refreshVoiceCapabilities().catch(() => undefined)
		}
		if (!this.voiceCapabilities.chromeTts) {
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
		if (this.voiceCapabilities.daemonTts.length === 0) {
			await this.refreshVoiceCapabilities().catch(() => undefined)
		}
		if (this.voiceCapabilities.daemonTts.length === 0) {
			return false
		}

		try {
			for (const [index, chunk] of chunks.entries()) {
				const result = await this.toolCall<TtsToolOutput>('synthesize_speech', {
					text: chunk,
					artifact_name: `anda_chrome_voice_${Date.now()}_${index + 1}`
				})
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

	private async syncServiceWorker(): Promise<void> {
		await this.serviceWorkerMessage('anda_settings_changed')
	}

	private async serviceWorkerMessage<Result = unknown>(
		type: string,
		message: Partial<ExtensionMessage> = {}
	): Promise<Extract<ExtensionResponse<Result>, { ok: true }>> {
		const response = await this.chrome.runtime.sendMessage<Result>({
			type,
			settings: this.settings,
			...message
		})
		if (!response?.ok) {
			throw new Error(response?.error || chrome.i18n.getMessage('extensionError'))
		}
		return response
	}

	private async registerBrowserSession(): Promise<void> {
		if (!this.settings.token) {
			return
		}

		const response = await this.serviceWorkerMessage<{ session?: string }>('anda_register')
		console.log('Registered browser session:', response)
	}

	async requestExtra(): Promise<Record<string, unknown>> {
		if (!this.tab) {
			await this.refreshActiveTab()
		}

		const extra: Record<string, unknown> = {
			conversation: 0,
			browser_client: 'chrome_extension'
		}

		if (this.tab) {
			extra.tab = {
				id: this.tab.id,
				url: this.tab.url,
				title: this.tab.title,
				incognito: this.tab.incognito
			}
		}

		return extra
	}

	async rpc<Result>(method: string, tupleArgs: unknown[]): Promise<Result> {
		if (!this.settings.token) {
			throw new Error(chrome.i18n.getMessage('tokenMissing'))
		}
		const response = await this.serviceWorkerMessage<Result>('anda_rpc', {
			method,
			params: tupleArgs
		})
		return response.result as Result
	}

	updateStatus(status: string, message: { kind: 'info' | 'error'; text: string } | null): void {
		this.status = status
		this.systemMessage = message
	}
}

export const andaClient = new AndaSidePanelClient()
