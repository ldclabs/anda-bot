import {
  browserSession,
  connectionKey,
  loadSettings,
  settingsKeys,
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
import { SkillsApi, skillsRevisionStorageKey } from './skills'
import { ResourceCache } from './resources'
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
  readonly skills = new SkillsApi(this, () => {
    void this.chrome.storage.local
      .set({ skillsRevision: crypto.randomUUID() })
      .catch(() => undefined)
  })
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
  #resources = new ResourceCache(this)
  #connectionEpoch = 0
  #chatEnabled = false
  #chatPromise: Promise<void> | null = null
  #destroyed = false
  #storageListener?: (changes: Record<string, { newValue?: unknown }>, area: string) => void
  #visibilityListener = () => {
    if (!document.hidden) for (const channel of this.channels.values()) channel.wakePolling()
  }
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

  async init(options: { conversations?: boolean } = {}): Promise<void> {
    if (!this.#initPromise) {
      this.#initPromise = this.#init()
    }
    await this.#initPromise
    if (options.conversations !== false && !this.#destroyed) {
      this.#chatPromise ||= this.#initChat()
      await this.#chatPromise
    }
  }

  async #init(): Promise<void> {
    this.applySettings(await loadSettings(this.chrome))
    if (this.#destroyed) return
    this.bindChromeEvents()
    this.updateStatus('ready', null)
    this.syncServiceWorker().catch(() => undefined)
    this.syncUiLanguage().catch(() => undefined)
    this.#uiLanguageTimer = setInterval(() => {
      this.syncUiLanguage().catch(() => undefined)
    }, uiLanguageSyncIntervalMs)
  }

  async #initChat(): Promise<void> {
    this.#chatEnabled = true
    await this.quickPrompts.load()
    await this.loadWorkspaceChannels()
    this.#localChannelSource = await browserSession(this.chrome)
    if (this.#destroyed) return
    this.activeChannel = this.ensureChannel(this.#localChannelSource)
    await this.refreshActiveTab()
    await this.refreshConnectionData()
  }

  private async refreshConnectionData(): Promise<void> {
    if (!this.#chatEnabled || !this.settings.token || this.#destroyed) return
    await Promise.allSettled([
      this.refreshModelState(),
      this.voice.refreshCapabilities(),
      this.refreshChannels()
    ])
    await this.activeChannel?.init()
  }

  private applySettings(settings: SettingsState): boolean {
    const next = normalizeSettings(settings)
    const changed = connectionKey(next) !== connectionKey(this.settings)
    this.settings = next
    if (changed) {
      this.#connectionEpoch++
      this.#channelSwitchEpoch++
      for (const channel of this.channels.values()) channel.destroy()
      this.channels.clear()
      this.activeChannel =
        this.#chatEnabled && this.#localChannelSource
          ? this.ensureChannel(this.#localChannelSource)
          : null
      this.bookmarks.clear()
      this.#resources.clear()
      this.modelState = emptyModelState()
      this.voice.capabilities = { transcription: [], daemonTts: [], chromeTts: false }
      this.sending = false
      this.skills.notifyChanged()
    }
    return changed
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
    this.#destroyed = true
    this.#connectionEpoch++
    this.#resources.clear()
    this.bookmarks.clear()
    if (this.#storageListener)
      this.chrome.storage.onChanged?.removeListener?.(this.#storageListener)
    if (typeof document !== 'undefined')
      document.removeEventListener('visibilitychange', this.#visibilityListener)
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

    for (const source of sources) this.ensureChannel(source).setSourceState(states?.[source])
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
    this.applySettings(settings)
    await this.chrome.storage.local.set(this.settings)
    if (!options.quiet) {
      this.systemMessage = { kind: 'info', text: getMessage('settingsSaved') }
    }
    await this.syncServiceWorker().catch(() => undefined)
    await this.refreshConnectionData()
    this.syncUiLanguage().catch(() => undefined)
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
    const epoch = this.#connectionEpoch
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
      if (ownsSendingFlag && epoch === this.#connectionEpoch) {
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
    const epoch = this.#connectionEpoch
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
      if (epoch !== this.#connectionEpoch) return
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

  loadResource(resource: Resource): Promise<Resource | null> {
    return this.#resources.load(resource)
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
    this.#storageListener = (changes, area) => {
      if (area !== 'local') return
      if (changes[skillsRevisionStorageKey]) this.skills.notifyChanged()
      if (settingsKeys.some((key) => key in changes)) {
        const next = { ...this.settings }
        for (const key of settingsKeys) {
          if (key in changes)
            Object.assign(next, { [key]: changes[key].newValue ?? defaultSettings[key] })
        }
        if (this.applySettings(next)) {
          void this.syncServiceWorker()
            .then(() => this.refreshConnectionData())
            .catch(() => undefined)
        }
      }
    }
    this.chrome.storage.onChanged?.addListener?.(this.#storageListener)
    if (typeof document !== 'undefined')
      document.addEventListener('visibilitychange', this.#visibilityListener)
    this.#tabActivatedListener = () => {
      if (!this.#chatEnabled) return
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
    const epoch = this.#connectionEpoch
    return {
      activeChannel: () =>
        typeof document !== 'undefined' && document.hidden ? null : this.activeSource,
      requestExtra: () => this.requestExtra(),
      rpc: <Result>(method: string, tupleArgs: unknown[]) => {
        if (epoch !== this.#connectionEpoch)
          return Promise.reject(new Error('Connection settings changed'))
        return this.rpc<Result>(method, tupleArgs)
      },
      updateStatus: (status, message) => {
        if (epoch === this.#connectionEpoch && this.activeChannel?.source === source) {
          this.updateStatus(status, message)
        }
      }
    }
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
    const epoch = this.#connectionEpoch
    const response = await this.chrome.runtime.sendMessage<Result>({
      type,
      settings: this.settings,
      ...message
    })
    if (epoch !== this.#connectionEpoch || this.#destroyed)
      throw new Error('Connection settings changed')
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

export const andaClient = new AndaSidePanelClient()
