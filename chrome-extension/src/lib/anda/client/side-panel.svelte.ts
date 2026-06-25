import {
  browserSession,
  defaultSettings,
  errorToError,
  errorToMessage,
  normalizeApprovalMode,
  normalizeSettings
} from '$lib/service-worker/settings'
import { SvelteMap, SvelteSet } from 'svelte/reactivity'
import { Channel, type API } from './channel.svelte'
import { getChromeApi } from './chrome'
import { isImmediatePromptCommand, parsePromptCommand } from './commands'
import { normalizeMessage } from './conversations'
import { normalizePromptSkills } from './helper'
import { getMessage, normalizeUiLanguage, uiLanguageStorageKey } from '$lib/i18n'
import type {
  AppearanceTheme,
  ActionApiOutput,
  ApprovalMode,
  Bookmark,
  BookmarkFolders,
  BookmarkedMessage,
  ChatAttachment,
  ChatMessage,
  ChromeApi,
  ChromeTabChangeInfo,
  ChromeTabInfo,
  Conversation,
  DaemonModelState,
  DaemonVoiceCapabilities,
  ExtensionMessage,
  ExtensionResponse,
  ManagedSkill,
  ManagedSkillDetail,
  ModelState,
  PageAudioResult,
  PageSpeechResult,
  PromptSkill,
  QuickPrompt,
  Resource,
  RpcOutput,
  SettingsState,
  SkillFileContent,
  SkillSourceInfo,
  SkillValidationResult,
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
export const quickPromptsStorageKey = 'quickPrompts'
export const quickPromptsMaxItems = 20
const quickPromptMaxTextChars = 2_000
// The launcher persists language switches on disk; the daemon serves them via
// the `ui_language` RPC, so a modest poll keeps an open panel in sync.
const uiLanguageSyncIntervalMs = 30_000

function emptyBookmarkFolders(): BookmarkFolders {
  return {
    version: 1,
    next_folder_id: 1,
    folders: {},
    updated_at: 0
  }
}

function numericTimestamp(value: unknown, fallback: number): number {
  return typeof value === 'number' && Number.isFinite(value) && value >= 0 ? value : fallback
}

function normalizeQuickPromptText(value: unknown): string {
  if (typeof value !== 'string') {
    return ''
  }
  return value.replace(/\r\n/g, '\n').trim().slice(0, quickPromptMaxTextChars).trim()
}

function quickPromptId(text: string): string {
  let hash = 2166136261
  for (let index = 0; index < text.length; index += 1) {
    hash ^= text.charCodeAt(index)
    hash = Math.imul(hash, 16777619)
  }
  return `qp-${(hash >>> 0).toString(36)}-${text.length.toString(36)}`
}

function sortQuickPromptsForDisplay(items: QuickPrompt[]): QuickPrompt[] {
  return [...items].sort(
    (left, right) =>
      right.usedAt - left.usedAt ||
      right.updatedAt - left.updatedAt ||
      right.useCount - left.useCount ||
      left.text.localeCompare(right.text)
  )
}

function limitQuickPrompts(items: QuickPrompt[]): QuickPrompt[] {
  if (items.length <= quickPromptsMaxItems) {
    return sortQuickPromptsForDisplay(items)
  }
  return sortQuickPromptsForDisplay(
    [...items]
      .sort(
        (left, right) =>
          right.useCount - left.useCount ||
          right.usedAt - left.usedAt ||
          right.updatedAt - left.updatedAt ||
          right.createdAt - left.createdAt
      )
      .slice(0, quickPromptsMaxItems)
  )
}

function normalizeQuickPrompts(value: unknown): QuickPrompt[] {
  if (!Array.isArray(value)) {
    return []
  }
  const byText = new Map<string, QuickPrompt>()
  for (const item of value) {
    const raw = item && typeof item === 'object' ? (item as Record<string, unknown>) : {}
    const text = normalizeQuickPromptText(raw.text)
    if (!text) {
      continue
    }
    const now = Date.now()
    const next: QuickPrompt = {
      id: quickPromptId(text),
      text,
      createdAt: numericTimestamp(raw.createdAt, now),
      updatedAt: numericTimestamp(raw.updatedAt, now),
      usedAt: numericTimestamp(raw.usedAt, 0),
      useCount: Math.max(0, Math.floor(numericTimestamp(raw.useCount, 0)))
    }
    const existing = byText.get(text)
    if (existing) {
      byText.set(text, {
        ...next,
        createdAt: Math.min(existing.createdAt, next.createdAt),
        updatedAt: Math.max(existing.updatedAt, next.updatedAt),
        usedAt: Math.max(existing.usedAt, next.usedAt),
        useCount: Math.max(existing.useCount, next.useCount)
      })
    } else {
      byText.set(text, next)
    }
  }
  return limitQuickPrompts(Array.from(byText.values()))
}

function quickPromptStorageSnapshot(items: QuickPrompt[]): QuickPrompt[] {
  return items.map((prompt) => ({
    id: prompt.id,
    text: prompt.text,
    createdAt: prompt.createdAt,
    updatedAt: prompt.updatedAt,
    usedAt: prompt.usedAt,
    useCount: prompt.useCount
  }))
}

function quickPromptsUpdateErrorMessage(error: unknown): string {
  const detail = errorToMessage(error)
  return (
    getMessage('quickPromptsUpdateFailed', [detail]) || `Could not update quick inputs: ${detail}`
  )
}

export class AndaSidePanelClient extends EventTarget {
  readonly chrome: ChromeApi

  settings: SettingsState = $state({ ...defaultSettings })
  tab: ChromeTabInfo | null = $state<ChromeTabInfo | null>(null)
  sending = $state(false)
  activeChannel = $state<Channel | null>(null)
  channels = new SvelteMap<string, Channel>()
  // Message ids the caller has bookmarked, kept in sync for star state.
  bookmarkedIds = new SvelteSet<string>()
  status = $state('starting')
  systemMessage = $state<{ kind: 'info' | 'error'; text: string } | null>(null)
  voiceCapabilities = $state<VoiceCapabilities>({
    transcription: [],
    daemonTts: [],
    chromeTts: false
  })
  modelState = $state<ModelState>(emptyModelState())
  quickPrompts = $state<QuickPrompt[]>([])

  #initPromise: Promise<void> | null = null
  #uiLanguageTimer: ReturnType<typeof setInterval> | null = null
  #resourceCache = new Map<number, Resource>()
  #resourceRequests = new Map<number, Promise<Resource>>()
  #bookmarkCache = new Map<number, Bookmark | null>()
  #bookmarkRequests = new Map<number, Promise<Bookmark | null>>()
  #quickPromptWrite: Promise<void> = Promise.resolve()
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
    await this.loadQuickPrompts()
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

  async saveApprovalMode(approvalMode: ApprovalMode): Promise<void> {
    const normalized = normalizeApprovalMode(approvalMode)
    if ((this.settings.approvalMode || defaultSettings.approvalMode) === normalized) {
      return
    }
    this.settings = normalizeSettings({ ...this.settings, approvalMode: normalized })
    await this.chrome.storage.local.set({ approvalMode: this.settings.approvalMode })
  }

  async loadQuickPrompts(): Promise<void> {
    const saved = await this.chrome.storage.local.get([quickPromptsStorageKey])
    this.quickPrompts = normalizeQuickPrompts(saved.quickPrompts)
  }

  isQuickPrompt(text: string): boolean {
    const normalized = normalizeQuickPromptText(text)
    return Boolean(normalized && this.quickPrompts.some((prompt) => prompt.text === normalized))
  }

  async toggleQuickPrompt(text: string): Promise<void> {
    if (this.isQuickPrompt(text)) {
      await this.removeQuickPrompt(text)
      return
    }
    await this.addQuickPrompt(text)
  }

  async addQuickPrompt(text: string): Promise<void> {
    const normalized = normalizeQuickPromptText(text)
    if (!normalized) {
      return
    }
    await this.applyQuickPromptUpdate((quickPrompts) => {
      const now = Date.now()
      const existing = quickPrompts.find((prompt) => prompt.text === normalized)
      const next: QuickPrompt = existing
        ? {
            ...existing,
            updatedAt: now
          }
        : {
            id: quickPromptId(normalized),
            text: normalized,
            createdAt: now,
            updatedAt: now,
            usedAt: now,
            useCount: 0
          }
      return limitQuickPrompts([
        next,
        ...quickPrompts.filter((prompt) => prompt.text !== normalized)
      ])
    })
  }

  async useQuickPrompt(text: string): Promise<void> {
    const normalized = normalizeQuickPromptText(text)
    if (!normalized) {
      return
    }
    await this.applyQuickPromptUpdate((quickPrompts) => {
      const prompt = quickPrompts.find((item) => item.text === normalized)
      if (!prompt) {
        return quickPrompts
      }
      const now = Date.now()
      return limitQuickPrompts([
        {
          ...prompt,
          usedAt: now,
          useCount: prompt.useCount + 1
        },
        ...quickPrompts.filter((item) => item.text !== normalized)
      ])
    })
  }

  async removeQuickPrompt(text: string): Promise<void> {
    const normalized = normalizeQuickPromptText(text)
    if (!normalized) {
      return
    }
    await this.applyQuickPromptUpdate((quickPrompts) =>
      quickPrompts.filter((prompt) => prompt.text !== normalized)
    )
  }

  async clearQuickPrompts(): Promise<void> {
    if (this.quickPrompts.length === 0) {
      return
    }
    await this.applyQuickPromptUpdate(() => [])
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

  async respondAction(input: {
    actionId: string
    approve?: boolean
    choiceId?: string
  }): Promise<ActionApiOutput> {
    if (!this.settings.token) {
      this.systemMessage = { kind: 'error', text: getMessage('pasteTokenFirst') }
      throw new Error(getMessage('pasteTokenFirst'))
    }

    const { output } = await this.toolCall<ActionApiOutput>('actions_api', {
      type: 'RespondAction',
      action_id: input.actionId,
      approve: input.approve ?? null,
      choice_id: input.choiceId ?? null
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

  async listSkillSources(): Promise<SkillSourceInfo[]> {
    if (!this.settings.token) {
      return []
    }
    const {
      output: { result }
    } = await this.toolCall<RpcOutput<SkillSourceInfo[]>>('skills_api', {
      type: 'ListSkillSources'
    })
    return Array.isArray(result) ? result : []
  }

  async listManagedSkills(includeInactive = true): Promise<ManagedSkill[]> {
    if (!this.settings.token) {
      return []
    }
    const {
      output: { result }
    } = await this.toolCall<RpcOutput<ManagedSkill[]>>('skills_api', {
      type: 'ListSkills',
      include_inactive: includeInactive
    })
    return Array.isArray(result) ? result : []
  }

  async getManagedSkill(id: string): Promise<ManagedSkillDetail> {
    const {
      output: { result }
    } = await this.toolCall<RpcOutput<ManagedSkillDetail>>('skills_api', {
      type: 'GetSkill',
      id
    })
    return result
  }

  async getManagedSkillFile(id: string, path: string): Promise<SkillFileContent> {
    const {
      output: { result }
    } = await this.toolCall<RpcOutput<SkillFileContent>>('skills_api', {
      type: 'GetSkillFile',
      id,
      path
    })
    return result
  }

  async cloneSkill(id: string, newName?: string): Promise<ManagedSkillDetail> {
    const {
      output: { result }
    } = await this.toolCall<RpcOutput<ManagedSkillDetail>>('skills_api', {
      type: 'CloneSkill',
      id,
      new_name: newName || null
    })
    this.emitSkillsChanged()
    return result
  }

  async setSkillEnabled(id: string, enabled: boolean): Promise<ManagedSkill[]> {
    const {
      output: { result }
    } = await this.toolCall<RpcOutput<ManagedSkill[]>>('skills_api', {
      type: 'SetSkillEnabled',
      id,
      enabled
    })
    this.emitSkillsChanged()
    return Array.isArray(result) ? result : []
  }

  async deletePersonalSkill(id: string): Promise<void> {
    await this.toolCall<RpcOutput<{ deleted: boolean }>>('skills_api', {
      type: 'DeletePersonalSkill',
      id
    })
    this.emitSkillsChanged()
  }

  async validateSkillContent(content: string): Promise<SkillValidationResult> {
    const {
      output: { result }
    } = await this.toolCall<RpcOutput<SkillValidationResult>>('skills_api', {
      type: 'ValidateSkill',
      content
    })
    return result
  }

  async reloadSkills(): Promise<ManagedSkill[]> {
    const {
      output: { result }
    } = await this.toolCall<RpcOutput<ManagedSkill[]>>('skills_api', {
      type: 'ReloadSkills'
    })
    this.emitSkillsChanged()
    return Array.isArray(result) ? result : []
  }

  private emitSkillsChanged(): void {
    this.dispatchEvent(new Event('skills-changed'))
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

  private async updateQuickPrompts(
    updater: (quickPrompts: QuickPrompt[]) => QuickPrompt[]
  ): Promise<void> {
    const write = this.#quickPromptWrite.then(async () => {
      const current = quickPromptStorageSnapshot(this.quickPrompts)
      const next = limitQuickPrompts(updater(current))
      if (JSON.stringify(next) === JSON.stringify(current)) {
        return
      }
      await this.persistQuickPrompts(next)
      this.quickPrompts = next
    })
    this.#quickPromptWrite = write.catch(() => undefined)
    await write
  }

  private async applyQuickPromptUpdate(
    updater: (quickPrompts: QuickPrompt[]) => QuickPrompt[]
  ): Promise<void> {
    try {
      await this.updateQuickPrompts(updater)
    } catch (error) {
      this.systemMessage = { kind: 'error', text: quickPromptsUpdateErrorMessage(error) }
    }
  }

  private async persistQuickPrompts(items: QuickPrompt[]): Promise<void> {
    await this.chrome.storage.local.set({
      [quickPromptsStorageKey]: quickPromptStorageSnapshot(items)
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

  /** Loads marked message ids for visible conversations into the star-state set. */
  async loadConversationBookmarks(
    conversations: number[],
    options: { force?: boolean } = {}
  ): Promise<void> {
    if (!this.settings.token) {
      return
    }
    const ids = Array.from(
      new Set(
        conversations.filter((conversation) => Number.isFinite(conversation) && conversation > 0)
      )
    )
    await Promise.all(
      ids.map((conversation) => this.loadConversationBookmark(conversation, options))
    )
  }

  async loadConversationBookmark(
    conversation: number,
    options: { force?: boolean } = {}
  ): Promise<Bookmark | null> {
    if (!this.settings.token || !Number.isFinite(conversation) || conversation <= 0) {
      return null
    }
    if (!options.force && this.#bookmarkCache.has(conversation)) {
      return this.#bookmarkCache.get(conversation) || null
    }

    let request = this.#bookmarkRequests.get(conversation)
    if (!request) {
      request = this.toolCall<RpcOutput<Bookmark | null>>('bookmarks_api', {
        type: 'GetConversationBookmark',
        conversation
      })
        .then(({ output: { result } }) => result || null)
        .finally(() => {
          this.#bookmarkRequests.delete(conversation)
        })
      this.#bookmarkRequests.set(conversation, request)
    }

    const bookmark = await request
    this.updateBookmarkCache(conversation, bookmark)
    return bookmark
  }

  isBookmarked(messageId: string): boolean {
    return this.bookmarkedIds.has(messageId)
  }

  async toggleBookmark(message: ChatMessage): Promise<void> {
    if (!this.settings.token || !message.id) {
      return
    }
    if (this.bookmarkedIds.has(message.id)) {
      await this.removeBookmark(message.id)
    } else {
      await this.addBookmark(message)
    }
  }

  async addBookmark(message: ChatMessage): Promise<void> {
    const messageId = message.id
    if (!this.settings.token || !messageId || this.bookmarkedIds.has(messageId)) {
      return
    }
    // Optimistic: show the star immediately, roll back if the daemon rejects.
    this.bookmarkedIds.add(messageId)
    try {
      await this.toolCall<RpcOutput<Bookmark>>('bookmarks_api', {
        type: 'AddBookmark',
        message_id: messageId,
        conversation: message.conversation,
        source: this.activeSource || '',
        role: message.role,
        text: message.text,
        folder_ids: []
      }).then(({ output: { result } }) => {
        this.updateBookmarkCache(result.conversation, result)
      })
    } catch (error) {
      this.bookmarkedIds.delete(messageId)
      this.systemMessage = { kind: 'error', text: errorToMessage(error) }
    }
  }

  async removeBookmark(messageId: string): Promise<boolean> {
    if (!this.settings.token || !messageId) {
      return false
    }
    const had = this.bookmarkedIds.delete(messageId)
    try {
      const {
        output: { result }
      } = await this.toolCall<
        RpcOutput<{ removed: boolean; conversation?: number; bookmark?: Bookmark | null }>
      >('bookmarks_api', {
        type: 'RemoveBookmark',
        message_id: messageId
      })
      const conversation = result.conversation || conversationFromMessageId(messageId)
      if (conversation > 0) {
        this.updateBookmarkCache(conversation, result.bookmark || null)
      }
      return true
    } catch (error) {
      if (had) {
        this.bookmarkedIds.add(messageId)
      }
      this.systemMessage = { kind: 'error', text: errorToMessage(error) }
      return false
    }
  }

  private updateBookmarkCache(conversation: number, bookmark: Bookmark | null): void {
    for (const id of Array.from(this.bookmarkedIds)) {
      if (conversationFromMessageId(id) === conversation) {
        this.bookmarkedIds.delete(id)
      }
    }
    this.#bookmarkCache.set(conversation, bookmark)
    if (!bookmark) {
      return
    }
    for (const message of bookmark.messages || []) {
      if (Number.isInteger(message.index) && message.index >= 0) {
        this.bookmarkedIds.add(`m-${bookmark.conversation}-${message.index}`)
      }
    }
  }

  /** Fetches one newest-first page of bookmarks for the panel. */
  async listBookmarks(
    cursor?: string,
    limit?: number
  ): Promise<{ items: Bookmark[]; nextCursor: string | null }> {
    if (!this.settings.token) {
      return { items: [], nextCursor: null }
    }
    const args: Record<string, unknown> = { type: 'ListBookmarks' }
    if (cursor) {
      args.cursor = cursor
    }
    if (limit) {
      args.limit = limit
    }
    const {
      output: { result, next_cursor }
    } = await this.toolCall<RpcOutput<Bookmark[]>>('bookmarks_api', args)
    return { items: result || [], nextCursor: next_cursor || null }
  }

  async listBookmarkFolders(): Promise<BookmarkFolders> {
    if (!this.settings.token) {
      return emptyBookmarkFolders()
    }
    const {
      output: { result }
    } = await this.toolCall<RpcOutput<BookmarkFolders>>('bookmarks_api', {
      type: 'ListBookmarkFolders'
    })
    return result || emptyBookmarkFolders()
  }

  async getConversationMarkdownForBookmark(bookmark: BookmarkedMessage): Promise<string> {
    if (
      !this.settings.token ||
      !Number.isFinite(bookmark.conversation) ||
      bookmark.conversation <= 0 ||
      !Number.isInteger(bookmark.message_index) ||
      bookmark.message_index < 0
    ) {
      return ''
    }

    const meta = await this.requestMetaForBookmark(bookmark)
    const {
      output: { result }
    } = await this.toolCall<RpcOutput<Conversation>>(
      'conversations_api',
      {
        type: 'GetConversation',
        _id: bookmark.conversation
      },
      [],
      meta
    )
    const rawMessage = result.messages?.[bookmark.message_index]
    if (!rawMessage) {
      return ''
    }

    const message = normalizeMessage(rawMessage, {
      conversation: result._id,
      index: bookmark.message_index,
      fallbackTimestamp: result.updated_at
    })
    return message?.text || ''
  }

  async createBookmarkFolder(
    name: string,
    parentId: number | null = null
  ): Promise<BookmarkFolders> {
    const {
      output: { result }
    } = await this.toolCall<RpcOutput<BookmarkFolders>>('bookmarks_api', {
      type: 'CreateBookmarkFolder',
      name,
      parent_id: parentId
    })
    return result || emptyBookmarkFolders()
  }

  async renameBookmarkFolder(folderId: number, name: string): Promise<BookmarkFolders> {
    const {
      output: { result }
    } = await this.toolCall<RpcOutput<BookmarkFolders>>('bookmarks_api', {
      type: 'RenameBookmarkFolder',
      folder_id: folderId,
      name
    })
    return result || emptyBookmarkFolders()
  }

  async deleteBookmarkFolder(folderId: number): Promise<BookmarkFolders> {
    const {
      output: { result }
    } = await this.toolCall<RpcOutput<BookmarkFolders>>('bookmarks_api', {
      type: 'DeleteBookmarkFolder',
      folder_id: folderId
    })
    return result || emptyBookmarkFolders()
  }

  async moveBookmarkFolder(
    folderId: number,
    parentId: number | null = null,
    order: number | null = null
  ): Promise<BookmarkFolders> {
    const {
      output: { result }
    } = await this.toolCall<RpcOutput<BookmarkFolders>>('bookmarks_api', {
      type: 'MoveBookmarkFolder',
      folder_id: folderId,
      parent_id: parentId,
      order
    })
    return result || emptyBookmarkFolders()
  }

  async setBookmarkFolders(messageId: string, folderIds: number[]): Promise<Bookmark> {
    const {
      output: { result }
    } = await this.toolCall<RpcOutput<Bookmark>>('bookmarks_api', {
      type: 'SetBookmarkFolders',
      message_id: messageId,
      folder_ids: folderIds
    })
    this.updateBookmarkCache(result.conversation, result)
    return result
  }

  async addBookmarkToFolder(messageId: string, folderId: number): Promise<Bookmark> {
    const {
      output: { result }
    } = await this.toolCall<RpcOutput<Bookmark>>('bookmarks_api', {
      type: 'AddBookmarkToFolder',
      message_id: messageId,
      folder_id: folderId
    })
    this.updateBookmarkCache(result.conversation, result)
    return result
  }

  async removeBookmarkFromFolder(messageId: string, folderId: number): Promise<Bookmark> {
    const {
      output: { result }
    } = await this.toolCall<RpcOutput<Bookmark>>('bookmarks_api', {
      type: 'RemoveBookmarkFromFolder',
      message_id: messageId,
      folder_id: folderId
    })
    this.updateBookmarkCache(result.conversation, result)
    return result
  }

  async listBookmarksInFolder(
    folderId: number,
    cursor?: string,
    limit?: number
  ): Promise<{ items: Bookmark[]; nextCursor: string | null }> {
    if (!this.settings.token) {
      return { items: [], nextCursor: null }
    }
    const args: Record<string, unknown> = {
      type: 'ListBookmarksInFolder',
      folder_id: folderId
    }
    if (cursor) {
      args.cursor = cursor
    }
    if (limit) {
      args.limit = limit
    }
    const {
      output: { result, next_cursor }
    } = await this.toolCall<RpcOutput<Bookmark[]>>('bookmarks_api', args)
    return { items: result || [], nextCursor: next_cursor || null }
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
      browser_client: 'chrome_extension',
      approval_mode: this.settings.approvalMode || defaultSettings.approvalMode
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

  private async requestMetaForBookmark(
    bookmark: BookmarkedMessage
  ): Promise<Record<string, unknown>> {
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

function conversationFromMessageId(messageId: string): number {
  const match = /^m-(\d+)-\d+$/.exec(messageId)
  return match ? Number(match[1]) : 0
}

function workspaceFromCliSource(source: string): string {
  if (!source.startsWith('cli:')) {
    return ''
  }

  const raw = source.slice(4).trim()
  return normalizeAbsoluteWorkspace(raw.startsWith('voice:') ? raw.slice(6) : raw)
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
