import {
  browserSession,
  defaultSettings,
  errorToError,
  errorToMessage,
  normalizeApprovalMode,
  normalizeSettings
} from '$lib/service-worker/settings'
import { SvelteMap } from 'svelte/reactivity'
import { BookmarksApi } from './bookmarks.svelte'
import { Channel, type API } from './channel.svelte'
import type { DaemonApi } from './daemon'
import { QuickPrompts } from './quick-prompts.svelte'
import { SkillsApi } from './skills'
import { VoiceSession } from './voice-session.svelte'
import {
  normalizeAbsoluteWorkspace,
  normalizeWorkspaceChannelSource,
  workspaceFromCliSource
} from './workspace'
import { getChromeApi } from '$lib/service-worker/chrome'
import { isImmediatePromptCommand, parsePromptCommand } from './commands'
import { getMessage, normalizeUiLanguage, uiLanguageStorageKey } from '$lib/i18n'
import type {
  AppearanceTheme,
  ActionApiOutput,
  ApprovalMode,
  BookmarkedMessage,
  ChatAttachment,
  ChromeApi,
  ChromeTabChangeInfo,
  ChromeTabInfo,
  DaemonModelState,
  ExtensionMessage,
  ExtensionResponse,
  ModelState,
  Resource,
  RpcOutput,
  SettingsState,
  SourceStateMap,
  ToolOutput,
  VoiceRecordingInput
} from './types'
import { normalTextForSpeech } from './voice'

const workspaceChannelSourcesStorageKey = 'workspaceChannelSources'
// The launcher persists language switches on disk; the daemon serves them via
// the `ui_language` RPC, so a modest poll keeps an open panel in sync.
const uiLanguageSyncIntervalMs = 30_000

export class AndaSidePanelClient extends EventTarget implements DaemonApi {
  readonly chrome: ChromeApi

  settings: SettingsState = $state({ ...defaultSettings })
  tab: ChromeTabInfo | null = $state<ChromeTabInfo | null>(null)
  sending = $state(false)
  activeChannel = $state<Channel | null>(null)
  channels = new SvelteMap<string, Channel>()
  status = $state('starting')
  systemMessage = $state<{ kind: 'info' | 'error'; text: string } | null>(null)
  modelState = $state<ModelState>(emptyModelState())

  /** Saved composer prompts, most-recently-used first. */
  readonly quickPrompts: QuickPrompts
  /** Speech in and out, plus the page-capture bridge. */
  readonly voice: VoiceSession

  /** Skill library verbs, and the `skills-changed` event views listen on. */
  readonly skills = new SkillsApi(this)
  /** Bookmark verbs plus the star state the transcript renders. */
  readonly bookmarks = new BookmarksApi(this, {
    activeSource: () => this.activeSource || '',
    bookmarkRequestMeta: (bookmark) => this.requestMetaForBookmark(bookmark),
    reportError: (error) => {
      this.systemMessage = { kind: 'error', text: errorToMessage(error) }
    }
  })

  #initPromise: Promise<void> | null = null
  #uiLanguageTimer: ReturnType<typeof setInterval> | null = null
  #resourceCache = new Map<number, Resource>()
  #resourceRequests = new Map<number, Promise<Resource>>()
  #localChannelSource = ''
  #workspaceChannelSources = new Set<string>()
  #channelSwitchEpoch = 0
  #tabActivatedListener?: (activeInfo: { tabId: number; windowId: number }) => void
  #tabUpdatedListener?: (tabId: number, changeInfo: ChromeTabChangeInfo, tab: ChromeTabInfo) => void

  constructor() {
    super()
    this.chrome = getChromeApi()
    this.quickPrompts = new QuickPrompts(this.chrome.storage.local, (text) => {
      this.systemMessage = { kind: 'error', text }
    })
    this.voice = new VoiceSession(this, {
      send: (type, message) => this.serviceWorkerMessage(type, message)
    })
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
    await this.quickPrompts.load()
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
      await this.voice.refreshCapabilities().catch(() => undefined)
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

    const epoch = ++this.#channelSwitchEpoch
    const workspace = workspaceFromCliSource(nextSource)
    if (workspace) {
      try {
        await this.rpc('register_workspace', [workspace])
      } catch (error) {
        if (epoch === this.#channelSwitchEpoch) {
          this.updateStatus('open folder failed', { kind: 'error', text: errorToMessage(error) })
        }
        return
      }
      if (epoch !== this.#channelSwitchEpoch) {
        return
      }
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
      // A directory registration may still be in flight for this channel.
      ++this.#channelSwitchEpoch

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
      await this.switchChannel(source)
      if (this.activeSource === source) {
        await this.saveWorkspaceChannelSource(source)
      }
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
    await this.voice.refreshCapabilities().catch(() => undefined)
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

  async saveApprovalMode(approvalMode: ApprovalMode): Promise<void> {
    const normalized = normalizeApprovalMode(approvalMode)
    if ((this.settings.approvalMode || defaultSettings.approvalMode) === normalized) {
      return
    }
    this.settings = normalizeSettings({ ...this.settings, approvalMode: normalized })
    await this.chrome.storage.local.set({ approvalMode: this.settings.approvalMode })
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

  async sendPrompt(
    text: string,
    attachments: ChatAttachment[] = [],
    memoryMode?: 'standard' | 'no_store' | 'off'
  ): Promise<void> {
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
      const poller = memoryMode
        ? await channel.sendPrompt(prompt, attachments, memoryMode)
        : await channel.sendPrompt(prompt, attachments)
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

  async respondAction(input: {
    actionId: string
    approve?: boolean
    choiceId?: string
    choiceText?: string
  }): Promise<ActionApiOutput> {
    if (!this.settings.token) {
      this.systemMessage = { kind: 'error', text: getMessage('pasteTokenFirst') }
      throw new Error(getMessage('pasteTokenFirst'))
    }

    const { output } = await this.toolCall<ActionApiOutput>('actions_api', {
      type: 'RespondAction',
      action_id: input.actionId,
      approve: input.approve ?? null,
      choice_id: input.choiceId ?? null,
      choice_text: input.choiceText ?? null
    })
    this.activeChannel?.applyActionResponse(output)
    this.activeChannel?.wakePolling()
    return output
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
        const spokenBy = await this.voice.speak(responseText, recording.voiceProvider || 'chrome')
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
    const transcription = await this.voice.transcribe(recording)
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
    ++this.#channelSwitchEpoch
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
      'appearanceTheme',
      'approvalMode'
    ])
    this.settings = normalizeSettings({
      baseUrl: saved.baseUrl || defaultSettings.baseUrl,
      token: saved.token || '',
      submitKeyMode: saved.submitKeyMode || defaultSettings.submitKeyMode,
      appearanceTheme: saved.appearanceTheme || defaultSettings.appearanceTheme,
      approvalMode: saved.approvalMode || defaultSettings.approvalMode
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

  /** True once a daemon token is configured; see `DaemonApi`. */
  get authorized(): boolean {
    return Boolean(this.settings.token)
  }

  async toolCall<Result>(
    name: string,
    args: Record<string, unknown>,
    resources: Resource[] = [],
    meta?: Record<string, unknown>
  ): Promise<ToolOutput<Result>> {
    const input: Record<string, unknown> = { name, args, resources }
    if (meta) {
      input.meta = meta
    }
    const rt = await this.rpc<ToolOutput<Result>>('tool_call', [input])
    const error = (rt.output as any).error
    if (error != null) {
      throw errorToError(error)
    }
    return rt
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
      browser_client: 'chrome_extension',
      approval_mode: this.settings.approvalMode || defaultSettings.approvalMode
    }
    const language = await this.requestLanguage()
    if (language) {
      extra.language = language
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

  private async requestLanguage(): Promise<string> {
    try {
      const saved = await this.chrome.storage.local.get([uiLanguageStorageKey])
      const storedLanguage = normalizeUiLanguage(saved?.[uiLanguageStorageKey])
      if (storedLanguage) {
        return storedLanguage
      }
    } catch {
      // Keep request metadata available when extension storage is temporarily unavailable.
    }

    const navigatorLanguage = globalThis.navigator?.language?.trim()
    if (navigatorLanguage) {
      return navigatorLanguage
    }

    try {
      return normalizeUiLanguage(this.chrome.i18n.getUILanguage?.())
    } catch {
      return ''
    }
  }

  async requestMetaForBookmark(bookmark: BookmarkedMessage): Promise<Record<string, unknown>> {
    const extra = await this.requestExtra()
    extra.source = bookmark.source
    const workspace = workspaceFromCliSource(bookmark.source)
    if (workspace) {
      extra.workspace = workspace
    }
    extra.conversation = bookmark.conversation
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
