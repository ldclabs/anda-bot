import {
  browserSession,
  defaultSettings,
  errorToError,
  errorToMessage,
  normalizeSettings
} from '$lib/service-worker/settings'
import { SvelteMap } from 'svelte/reactivity'
import { Channel, type API } from './channel.svelte'
import { getChromeApi } from './chrome'
import { isImmediatePromptCommand, parsePromptCommand } from './commands'
import { normalizePromptSkills } from './helper'
import { getMessage, normalizeUiLanguage, uiLanguageStorageKey } from '$lib/i18n'
import type {
  AppearanceTheme,
  ChatAttachment,
  ChromeApi,
  ChromeTabChangeInfo,
  ChromeTabInfo,
  DaemonModelState,
  DaemonVoiceCapabilities,
  ExtensionMessage,
  ExtensionResponse,
  ModelState,
  PageAudioResult,
  PageSpeechResult,
  PromptSkill,
  Resource,
  RpcOutput,
  SettingsState,
  SourceStateMap,
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
  playVoiceTtsPipeline,
  prepareVoiceTtsText,
  splitVoiceTtsText,
  voiceTtsChunkChars
} from './voice'

const workspaceChannelSourcesStorageKey = 'workspaceChannelSources'
// The launcher persists language switches on disk; the daemon serves them via
// the `ui_language` RPC, so a modest poll keeps an open panel in sync.
const uiLanguageSyncIntervalMs = 30_000

export class AndaSidePanelClient extends EventTarget {
  readonly chrome: ChromeApi

  settings: SettingsState = $state({ ...defaultSettings })
  tab: ChromeTabInfo | null = $state<ChromeTabInfo | null>(null)
  sending = $state(false)
  activeChannel = $state<Channel | null>(null)
  channels = new SvelteMap<string, Channel>()
  status = $state('starting')
  systemMessage = $state<{ kind: 'info' | 'error'; text: string } | null>(null)
  voiceCapabilities = $state<VoiceCapabilities>({
    transcription: [],
    daemonTts: [],
    chromeTts: false
  })
  modelState = $state<ModelState>(emptyModelState())

  #initPromise: Promise<void> | null = null
  #uiLanguageTimer: ReturnType<typeof setInterval> | null = null
  #resourceCache = new Map<number, Resource>()
  #resourceRequests = new Map<number, Promise<Resource>>()
  #localChannelSource = ''
  #workspaceChannelSources = new Set<string>()
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
    await this.loadWorkspaceChannels()
    const localChannel = await browserSession(this.chrome)
    this.#localChannelSource = localChannel
    const channel = this.ensureChannel(localChannel)
    this.activeChannel = channel

    this.bindChromeEvents()
    await this.refreshActiveTab()
    this.updateStatus('ready', null)
    this.syncServiceWorker().catch(() => undefined)

    if (this.settings.token) {
      await this.refreshModelState().catch(() => undefined)
      await this.refreshVoiceCapabilities().catch(() => undefined)
      await this.refreshChannels().catch(() => undefined)
      await channel.init().catch(() => undefined)
      this.syncUiLanguage().catch(() => undefined)
    }
    this.#uiLanguageTimer = setInterval(() => {
      this.syncUiLanguage().catch(() => undefined)
    }, uiLanguageSyncIntervalMs)
  }

  /**
   * Follows the language selected in the Anda launcher: persists it for
   * initI18n(). Every extension page watches the stored value (via
   * watchUiLanguage) and reloads itself so all rendered strings switch.
   */
  async syncUiLanguage(): Promise<void> {
    if (!this.settings.token) {
      return
    }
    const result = await this.rpc<{ language?: string | null }>('ui_language', [])
    const language = normalizeUiLanguage(result?.language)
    if (!language) {
      return
    }
    const saved = await this.chrome.storage.local.get([uiLanguageStorageKey])
    if (normalizeUiLanguage(saved?.[uiLanguageStorageKey]) === language) {
      return
    }
    await this.chrome.storage.local.set({ [uiLanguageStorageKey]: language })
  }

  get channelList(): Channel[] {
    return Array.from(this.channels.values()).sort((a, b) => {
      return b.latestActivityAt - a.latestActivityAt || a.source.localeCompare(b.source)
    })
  }

  get activeSource(): string | null {
    return this.activeChannel?.source || null
  }

  destroy(): void {
    if (this.#uiLanguageTimer) {
      clearInterval(this.#uiLanguageTimer)
      this.#uiLanguageTimer = null
    }
    if (this.chrome && this.#tabActivatedListener) {
      this.chrome.tabs.onActivated.removeListener(this.#tabActivatedListener)
    }
    if (this.chrome && this.#tabUpdatedListener) {
      this.chrome.tabs.onUpdated.removeListener(this.#tabUpdatedListener)
    }
    for (const channel of this.channels.values()) {
      channel.destroy()
    }
    console.warn('AndaSidePanelClient destroyed')
  }

  async refreshChannels(): Promise<void> {
    if (!this.settings.token) {
      return
    }

    const {
      output: { result: states }
    } = await this.toolCall<RpcOutput<SourceStateMap>>('conversations_api', {
      type: 'ListSourceState'
    })
    const sources = new Set<string>()
    if (this.#localChannelSource) {
      sources.add(this.#localChannelSource)
    }
    for (const source of this.#workspaceChannelSources) {
      sources.add(source)
    }
    for (const source of Object.keys(states || {})) {
      if (source.trim()) {
        sources.add(source)
      }
    }

    const initTasks = Array.from(sources).map((source) =>
      this.ensureChannel(source)
        .init()
        .catch(() => undefined)
    )
    await Promise.all(initTasks)
  }

  async switchChannel(source: string): Promise<void> {
    const nextSource = source.trim()
    if (!nextSource) {
      return
    }

    const channel = this.ensureChannel(nextSource)
    this.activeChannel = channel
    this.updateStatus(channel.status, null)
    // A background channel polls at a slow cadence; skip the remaining sleep
    // so the just-activated channel refreshes immediately.
    channel.wakePolling()
    if (this.settings.token) {
      await channel.init().catch(() => undefined)
    }
  }

  async deleteChannel(source: string): Promise<void> {
    const sourceKey = source.trim()
    if (!sourceKey || this.sending) {
      return
    }

    if (!this.settings.token) {
      this.systemMessage = { kind: 'error', text: getMessage('pasteTokenFirst') }
      return
    }

    const channel = this.channels.get(sourceKey)
    if (channel?.sending) {
      return
    }

    try {
      await this.toolCall<RpcOutput<{ deleted: boolean }>>('conversations_api', {
        type: 'DeleteSourceState',
        source: sourceKey
      })
      await this.removeWorkspaceChannelSource(sourceKey)

      const wasActive = this.activeChannel?.source === sourceKey
      if (sourceKey === this.#localChannelSource) {
        const localChannel = this.ensureChannel(sourceKey)
        localChannel.clearConversation()
        if (wasActive) {
          this.activeChannel = localChannel
          this.updateStatus('ready', null)
        }
      } else {
        channel?.destroy()
        this.channels.delete(sourceKey)
        if (wasActive) {
          await this.switchToFallbackChannel()
        }
      }

      this.systemMessage = { kind: 'info', text: getMessage('channelDeleted') }
    } catch (error) {
      this.updateStatus('delete failed', { kind: 'error', text: errorToMessage(error) })
    }
  }

  async openWorkspaceChannel(): Promise<void> {
    if (this.sending) {
      return
    }

    if (!this.settings.token) {
      this.systemMessage = { kind: 'error', text: getMessage('pasteTokenFirst') }
      return
    }

    try {
      const result = await this.rpc<{ path?: string | null }>('pick_workspace', [])
      const workspace = normalizeAbsoluteWorkspace(result?.path)
      if (!workspace) {
        return
      }

      const source = `cli:${workspace}`
      await this.saveWorkspaceChannelSource(source)
      await this.switchChannel(source)
    } catch (error) {
      this.updateStatus('open folder failed', { kind: 'error', text: errorToMessage(error) })
    }
  }

  async saveSettings(settings: SettingsState, options: { quiet?: boolean } = {}): Promise<void> {
    this.settings = normalizeSettings(settings)
    await this.chrome.storage.local.set(this.settings)
    if (!options.quiet) {
      this.systemMessage = { kind: 'info', text: getMessage('settingsSaved') }
    }
    await this.syncServiceWorker().catch(() => undefined)
    if (this.settings.token) {
      this.refreshChannels().catch(() => undefined)
      this.refreshModelState().catch(() => undefined)
      this.syncUiLanguage().catch(() => undefined)
    } else {
      this.modelState = emptyModelState()
    }
    await this.refreshVoiceCapabilities().catch(() => undefined)
  }

  async saveAppearanceTheme(appearanceTheme: AppearanceTheme): Promise<void> {
    const previousTheme = this.settings.appearanceTheme
    this.settings = normalizeSettings({ ...this.settings, appearanceTheme })
    if (this.settings.appearanceTheme === previousTheme) {
      return
    }
    await this.chrome.storage.local.set({ appearanceTheme: this.settings.appearanceTheme })
    this.syncServiceWorker().catch(() => undefined)
  }

  async testConnection(settings: SettingsState): Promise<void> {
    try {
      await this.saveSettings(settings, { quiet: true })
      await this.rpc('information', [])
      await this.refreshModelState()
      this.updateStatus('connected', {
        kind: 'info',
        text: getMessage('connectionTestPassed')
      })
    } catch (error) {
      this.updateStatus('connection failed', { kind: 'error', text: errorToMessage(error) })
    }
  }

  async sendPrompt(text: string, attachments: ChatAttachment[] = []): Promise<void> {
    const prompt = text.trim()
    const channel = this.activeChannel
    const command = parsePromptCommand(prompt)
    const immediate = isImmediatePromptCommand(command)
    if ((!prompt && attachments.length === 0) || (this.sending && !immediate) || !channel) {
      return
    }

    if (!this.settings.token) {
      this.systemMessage = { kind: 'error', text: getMessage('pasteTokenFirst') }
      // Throw so the composer restores the draft instead of dropping it.
      throw new Error(getMessage('pasteTokenFirst'))
    }

    // /side runs a detached subagent inline on the daemon and can take a long
    // time; it must not hold the global sending flag and block the composer.
    const ownsSendingFlag = !this.sending && command?.kind !== 'side'
    if (ownsSendingFlag) {
      this.sending = true
    }
    try {
      await this.refreshActiveTab()
      const poller = await channel.sendPrompt(prompt, attachments)
      // No consumer here; close so the polling loop does not buffer messages indefinitely.
      poller?.close()
    } catch (error) {
      this.updateStatus('send failed', { kind: 'error', text: errorToMessage(error) })
      // Propagate so the composer can restore the unsent draft.
      throw error
    } finally {
      if (ownsSendingFlag) {
        this.sending = false
      }
    }
  }

  async stopActiveTask(): Promise<void> {
    const channel = this.activeChannel
    if (!channel) {
      return
    }

    if (!this.settings.token) {
      this.systemMessage = { kind: 'error', text: getMessage('pasteTokenFirst') }
      return
    }

    try {
      const poller = await channel.sendPrompt('/stop', [])
      poller?.close()
    } catch (error) {
      this.updateStatus('stop failed', { kind: 'error', text: errorToMessage(error) })
    }
  }

  async sendVoiceTurn(recording: VoiceRecordingInput): Promise<void> {
    const channel = this.activeChannel
    if (this.sending || !channel) {
      return
    }

    if (!this.settings.token) {
      this.systemMessage = { kind: 'error', text: getMessage('pasteTokenFirst') }
      return
    }

    this.sending = true
    try {
      const prompt = await this.voiceTurnPrompt(recording)
      if (!prompt) {
        this.updateStatus('idle', {
          kind: 'error',
          text: getMessage('noVoiceCaptured')
        })
        return
      }

      await this.refreshActiveTab()
      const poller = await channel.sendPrompt(prompt, [])
      if (!poller) {
        return
      }
      if (!recording.ttsEnabled) {
        poller.close()
        return
      }

      for await (const message of poller) {
        let responseText = normalTextForSpeech(message?.text)
        if (!responseText?.trim()) {
          continue
        }

        this.updateStatus('speaking', null)
        const spokenBy = await this.speakAssistantText(
          responseText,
          recording.voiceProvider || 'chrome'
        )
        if (!spokenBy) {
          const service =
            recording.voiceProvider === 'anda'
              ? getMessage('andaVoiceService')
              : getMessage('browserVoiceService')
          this.updateStatus('playback failed', {
            kind: 'error',
            text: getMessage('playbackUnavailable') + `: ${service}`
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
      throw new Error(result.error || getMessage('browserSpeechStartFailed'))
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
      throw new Error(result.error || getMessage('andaVoiceStartFailed'))
    }
  }

  async stopBrowserAudioCapture(): Promise<PageAudioResult> {
    const response = await this.serviceWorkerMessage<PageAudioResult>('anda_page_audio_stop')
    const result = response.result || {}
    if (result.error) {
      throw new Error(result.error)
    }
    if (!result.audioBase64 || !result.mimeType) {
      throw new Error(getMessage('noVoiceCaptured'))
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

  async refreshModelState(options: { reload?: boolean } = {}): Promise<ModelState> {
    if (!this.settings.token) {
      this.modelState = emptyModelState()
      return this.modelState
    }

    const method = options.reload ? 'reload_models' : 'model_names'
    const daemonState = await this.rpc<DaemonModelState>(method, [])
    this.modelState = normalizeModelState(daemonState)
    return this.modelState
  }

  async loadResource(resource: Resource): Promise<Resource | null> {
    const id = resource._id || 0
    if (!id) {
      return resource.blob ? resource : null
    }
    if (resource.blob) {
      return resource
    }

    const cached = this.#resourceCache.get(id)
    if (cached) {
      return mergeResource(resource, cached)
    }

    let request = this.#resourceRequests.get(id)
    if (!request) {
      request = this.toolCall<RpcOutput<Resource>>('resources_api', {
        type: 'GetResource',
        _id: id
      })
        .then(({ output: { result } }) => {
          this.#resourceCache.set(id, result)
          return result
        })
        .finally(() => {
          this.#resourceRequests.delete(id)
        })
      this.#resourceRequests.set(id, request)
    }

    return mergeResource(resource, await request)
  }

  async setActiveModel(modelName: string): Promise<ModelState> {
    const nextModel = modelName.trim()
    if (!nextModel) {
      return this.modelState
    }

    if (!this.settings.token) {
      this.systemMessage = { kind: 'error', text: getMessage('pasteTokenFirst') }
      return this.modelState
    }

    try {
      const daemonState = await this.rpc<DaemonModelState>('set_model', [nextModel])
      this.modelState = normalizeModelState(daemonState)
      this.systemMessage = { kind: 'info', text: getMessage('modelUpdated') }
      return this.modelState
    } catch (error) {
      this.systemMessage = { kind: 'error', text: errorToMessage(error) }
      throw error
    }
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
      if (!this.tab || tabId !== this.tab.id || (!changeInfo.title && !changeInfo.url)) {
        return
      }

      this.tab = { ...this.tab, ...tab }
      this.registerBrowserSession().catch(() => undefined)
    }
    this.chrome.tabs.onActivated.addListener(this.#tabActivatedListener)
    this.chrome.tabs.onUpdated.addListener(this.#tabUpdatedListener)
  }

  private ensureChannel(source: string): Channel {
    let channel = this.channels.get(source)
    if (!channel) {
      channel = new Channel(source, this.channelApi(source))
      this.channels.set(source, channel)
    }
    return channel
  }

  private async switchToFallbackChannel(): Promise<void> {
    const next =
      this.channelList[0] ||
      (this.#localChannelSource ? this.ensureChannel(this.#localChannelSource) : null)
    if (!next) {
      this.activeChannel = null
      this.updateStatus('ready', null)
      return
    }

    this.activeChannel = next
    this.updateStatus(next.status, null)
    if (this.settings.token) {
      await next.init().catch(() => undefined)
    }
  }

  private channelApi(source: string): API {
    return {
      activeChannel: () => this.activeSource,
      requestExtra: () => this.requestExtra(),
      rpc: <Result>(method: string, tupleArgs: unknown[]) => this.rpc<Result>(method, tupleArgs),
      updateStatus: (status, message) => {
        if (this.activeChannel?.source === source) {
          this.updateStatus(status, message)
        }
      }
    }
  }

  private async loadSettings(): Promise<void> {
    const saved = await this.chrome.storage.local.get([
      'baseUrl',
      'token',
      'submitKeyMode',
      'appearanceTheme'
    ])
    this.settings = normalizeSettings({
      baseUrl: saved.baseUrl || defaultSettings.baseUrl,
      token: saved.token || '',
      submitKeyMode: saved.submitKeyMode || defaultSettings.submitKeyMode,
      appearanceTheme: saved.appearanceTheme || defaultSettings.appearanceTheme
    })
  }

  private async loadWorkspaceChannels(): Promise<void> {
    const saved = await this.chrome.storage.local.get([workspaceChannelSourcesStorageKey])
    const sources = normalizeWorkspaceChannelSources(saved.workspaceChannelSources)
    this.#workspaceChannelSources = new Set(sources)
    for (const source of sources) {
      this.ensureChannel(source)
    }
  }

  private async saveWorkspaceChannelSource(source: string): Promise<void> {
    const normalized = normalizeWorkspaceChannelSource(source)
    if (!normalized || this.#workspaceChannelSources.has(normalized)) {
      return
    }

    this.#workspaceChannelSources.add(normalized)
    this.ensureChannel(normalized)
    await this.persistWorkspaceChannelSources()
  }

  private async removeWorkspaceChannelSource(source: string): Promise<void> {
    const normalized = normalizeWorkspaceChannelSource(source)
    if (!normalized || !this.#workspaceChannelSources.delete(normalized)) {
      return
    }
    await this.persistWorkspaceChannelSources()
  }

  private async persistWorkspaceChannelSources(): Promise<void> {
    await this.chrome.storage.local.set({
      workspaceChannelSources: Array.from(this.#workspaceChannelSources).sort((left, right) =>
        left.localeCompare(right)
      )
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
      throw new Error(getMessage('voiceTranscriptionNotConfigured'))
    }
    if (!recording.audioBase64 || !recording.fileName) {
      throw new Error(getMessage('audioCaptureMissingData'))
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
      await playVoiceTtsPipeline(
        chunks,
        async (chunk, index) => {
          const result = await this.toolCall<TtsToolOutput>('synthesize_speech', {
            text: chunk,
            artifact_name: `anda_chrome_voice_${Date.now()}_${index + 1}`
          })
          const artifact = result.artifacts?.find(isAudioResource)
          if (!artifact?.blob) {
            throw new Error('Anda TTS did not return playable audio.')
          }
          return artifact
        },
        (artifact) => playAudioArtifact(artifact)
      )
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
      throw new Error(response?.error || getMessage('extensionError'))
    }
    return response
  }

  private async registerBrowserSession(): Promise<void> {
    if (!this.settings.token) {
      return
    }

    await this.serviceWorkerMessage<{ session?: string }>('anda_register')
  }

  async requestExtra(): Promise<Record<string, unknown>> {
    await this.refreshActiveTab()
    const extra: Record<string, unknown> = {
      conversation: 0,
      browser_client: 'chrome_extension'
    }

    if (this.tab) {
      extra.tab = {
        id: this.tab.id,
        url: this.tab.url,
        title: this.tab.title,
        incognito: this.tab.incognito,
        window: this.tab.windowId
      }
    }

    return extra
  }

  async rpc<Result>(method: string, tupleArgs: unknown[]): Promise<Result> {
    if (!this.settings.token) {
      throw new Error(getMessage('tokenMissing'))
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

function emptyModelState(): ModelState {
  return { activeModel: null, modelNames: [] }
}

function normalizeWorkspaceChannelSources(value: unknown): string[] {
  if (!Array.isArray(value)) {
    return []
  }

  const sources = new Set<string>()
  for (const item of value) {
    const source = normalizeWorkspaceChannelSource(String(item || ''))
    if (source) {
      sources.add(source)
    }
  }
  return Array.from(sources)
}

function normalizeWorkspaceChannelSource(source: string): string {
  const trimmed = source.trim()
  if (!trimmed.startsWith('cli:')) {
    return ''
  }

  if (trimmed.startsWith('cli:voice:')) {
    const workspace = normalizeAbsoluteWorkspace(trimmed.slice('cli:voice:'.length))
    return workspace ? `cli:voice:${workspace}` : ''
  }

  const workspace = normalizeAbsoluteWorkspace(trimmed.slice('cli:'.length))
  return workspace ? `cli:${workspace}` : ''
}

function normalizeAbsoluteWorkspace(value: unknown): string {
  const trimmed = String(value || '').trim()
  if (!isAbsoluteWorkspacePath(trimmed)) {
    return ''
  }

  let normalized = trimmed
  while (
    normalized.length > 1 &&
    /[\\/]$/.test(normalized) &&
    normalized !== '/' &&
    !/^[A-Za-z]:[\\/]$/.test(normalized)
  ) {
    normalized = normalized.slice(0, -1)
  }
  return normalized
}

function isAbsoluteWorkspacePath(value: string): boolean {
  return value.startsWith('/') || /^[A-Za-z]:[\\/]/.test(value) || value.startsWith('\\\\')
}

function normalizeModelState(state: DaemonModelState | null | undefined): ModelState {
  const seen = new Set<string>()
  const modelNames = (Array.isArray(state?.model_names) ? state.model_names : [])
    .map((name) => String(name || '').trim())
    .filter((name) => {
      if (!name || seen.has(name)) {
        return false
      }
      seen.add(name)
      return true
    })
  const activeModel = typeof state?.active_model === 'string' ? state.active_model.trim() : ''
  if (activeModel && !seen.has(activeModel)) {
    modelNames.push(activeModel)
  }
  return {
    activeModel: activeModel || null,
    modelNames
  }
}

function mergeResource(summary: Resource, full: Resource): Resource {
  return {
    ...summary,
    ...full,
    tags: full.tags?.length ? full.tags : summary.tags,
    metadata: {
      ...(summary.metadata || {}),
      ...(full.metadata || {})
    },
    blob: full.blob || summary.blob,
    description: full.description || summary.description
  }
}

export const andaClient = new AndaSidePanelClient()
