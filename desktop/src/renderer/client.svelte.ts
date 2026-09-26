import { SvelteMap } from 'svelte/reactivity'
import { Channel, type AgentSubmission } from '$lib/anda/client/channel.svelte'
import { SkillsApi } from '$lib/anda/client/skills'
import { BookmarksApi } from '$lib/anda/client/bookmarks.svelte'
import { QuickPrompts } from '$lib/anda/client/quick-prompts.svelte'
import { ResourceCache } from '$lib/anda/client/resources'
import { VoiceSession } from '$lib/anda/client/voice-session.svelte'
import { parsePromptCommand } from '$lib/anda/client/commands'
import { workspaceFromCliSource } from '$lib/anda/client/workspace'
import { normalizeSettings, errorToError } from '$lib/service-worker/settings'
import { setClientPlatform } from '$lib/anda/client/platform'
import { normalizeUiLanguage, setNativeMessages } from '$lib/i18n'
import type { DaemonApi } from '$lib/anda/client/daemon'
import type {
  ActionApiOutput,
  AgentInput,
  AgentOutput,
  ChatAttachment,
  Resource,
  RpcOutput,
  SettingsState,
  SourceStateMap,
  ToolOutput,
  ModelState,
  DaemonModelState,
  VoiceRecordingInput
} from '$lib/anda/client/types'
import type { Preferences, DaemonView, PendingSubmission, ChatEntry } from '../shared/contract'
import en from '../../../chrome-extension/public/_locales/en/messages.json'
import zh from '../../../chrome-extension/public/_locales/zh_CN/messages.json'
import fr from '../../../chrome-extension/public/_locales/fr/messages.json'
import es from '../../../chrome-extension/public/_locales/es/messages.json'
import ru from '../../../chrome-extension/public/_locales/ru/messages.json'
import ar from '../../../chrome-extension/public/_locales/ar/messages.json'

const translations = { en, zh_CN: zh, fr, es, ru, ar }
const initialPreferences: Preferences = {
  theme: 'system',
  language: 'en',
  approvalMode: 'on_risk',
  submitKeyMode: 'enter',
  notifications: true,
  launchAtLogin: false,
  chats: [],
  projects: [],
  drafts: {}
}

export class DesktopClient extends EventTarget implements DaemonApi {
  preferences = $state<Preferences>(structuredClone(initialPreferences))
  connection = $state<DaemonView>({
    connected: false,
    home: '',
    baseUrl: '',
    managed: false,
    binary: null
  })
  pending = $state<PendingSubmission[]>([])
  platform = $state('darwin')
  ready = $state(false)
  sending = $state(false)
  status = $state('connecting')
  systemMessage = $state<{ kind: 'info' | 'error'; text: string } | null>(null)
  activeChannel = $state<Channel | null>(null)
  modelState = $state<ModelState>({ activeModel: null, modelNames: [] })
  view = $state('chat')
  jumpMessage = $state('')
  incomingDraft = $state<{
    id: string
    text: string
    createdAt: number
  } | null>(null)
  readonly channels = new SvelteMap<string, Channel>()
  readonly skills = new SkillsApi(this)
  readonly resources = new ResourceCache(this)
  readonly voice = new VoiceSession(this, {
    send: async () => ({ result: { available: false } as never })
  })
  readonly quickPrompts = new QuickPrompts(
    {
      get: (keys) => window.anda.storageGet(keys),
      set: (items) => window.anda.storageSet($state.snapshot(items))
    },
    (text) => this.fail(text)
  )
  readonly bookmarks = new BookmarksApi(this, {
    activeSource: () => this.activeSource,
    bookmarkRequestMeta: async (bookmark) => ({
      ...this.requestExtra(bookmark.source),
      source: bookmark.source,
      conversation: bookmark.conversation
    }),
    reportError: (error) => this.fail(error)
  })
  private refreshTimer?: ReturnType<typeof setInterval>
  private eventRevision = 0
  private eventRefresh?: ReturnType<typeof setTimeout>
  private unsubscribe?: () => void
  private prefWrites: Promise<unknown> = Promise.resolve()
  private seenStatuses = new Map<string, string>()
  private receiptRecovery: Promise<void> = Promise.resolve()
  private activeSubmissions = new Set<string>()
  private ephemeralWorkspace?: string
  private draftTimer?: ReturnType<typeof setTimeout>
  private drafts = new Map<string, { text: string; attachments: ChatAttachment[] }>()
  getDraft(source: string): { text: string; attachments: ChatAttachment[] } {
    return (
      this.drafts.get(source) || {
        text: this.preferences.drafts[source] || '',
        attachments: []
      }
    )
  }
  saveDraft(source: string, draft: { text: string; attachments: ChatAttachment[] }): void {
    const previous = this.drafts.get(source)
    if (previous?.text === draft.text && previous.attachments === draft.attachments) return
    this.drafts.set(source, draft)
    this.preferences.drafts[source] = draft.text
    clearTimeout(this.draftTimer)
    this.draftTimer = setTimeout(() => {
      void this.savePreferences({
        drafts: { ...this.preferences.drafts }
      }).catch((error) => this.fail(error))
    }, 350)
  }
  get authorized(): boolean {
    return this.connection.connected
  }
  get activeSource(): string {
    return this.activeChannel?.source || ''
  }
  get readOnly(): boolean {
    return this.activeSource.includes(':reply_target:')
  }
  get workspace(): string | undefined {
    return (
      this.preferences.chats.find((c) => c.source === this.activeSource)?.workspace ||
      workspaceFromCliSource(this.activeSource) ||
      this.ephemeralWorkspace
    )
  }
  get settings(): SettingsState {
    // A capability marker for shared views; the actual bearer never leaves Main.
    return normalizeSettings({
      baseUrl: this.connection.baseUrl || 'http://127.0.0.1:8042',
      token: this.authorized ? 'native-session' : '',
      appearanceTheme: this.preferences.theme,
      submitKeyMode: this.preferences.submitKeyMode,
      approvalMode: this.preferences.approvalMode
    })
  }
  private initPromise?: Promise<void>
  init(_options?: { conversations?: boolean }): Promise<void> {
    return (this.initPromise ||= this.initialize())
  }
  private async initialize(): Promise<void> {
    const bootstrap = await window.anda.bootstrap()
    this.preferences = bootstrap.preferences
    this.platform = bootstrap.platform
    document.documentElement.dataset.platform = bootstrap.platform
    this.connection = bootstrap.daemon
    this.pending = bootstrap.pending
    const language = normalizeUiLanguage(this.preferences.language || navigator.language) || 'en'
    this.preferences.language = language
    setNativeMessages(language, translations[language])
    setClientPlatform({
      settings: async () => this.settings,
      saveSettings: async (settings) => {
        await this.savePreferences({
          theme: settings.appearanceTheme,
          approvalMode: settings.approvalMode || 'on_risk',
          submitKeyMode: settings.submitKeyMode
        })
      },
      rpc: (method, params) => this.rpc(method, params),
      config: (method, content, revision) => window.anda.config(method, content, revision),
      storage: {
        get: (keys) => window.anda.storageGet(keys),
        set: (items) => window.anda.storageSet($state.snapshot(items))
      },
      printHtml: (html) => window.anda.printHtml(html),
      openChat: () => this.consumeNavigation()
    })
    this.unsubscribe = window.anda.onEvent((event) => {
      if (event.type === 'connection') {
        const was = this.authorized
        this.connection = event.value as DaemonView
        this.eventRevision++
        for (const channel of this.channels.values()) channel.wakePolling()
        if (this.authorized && !was) void this.refresh().catch((error) => this.fail(error))
      } else if (event.type === 'update') {
        this.systemMessage = { kind: 'info', text: String(event.value) }
      } else if (event.type === 'state') {
        this.eventRevision++
        for (const channel of this.channels.values()) channel.wakePolling()
        clearTimeout(this.eventRefresh)
        this.eventRefresh = setTimeout(() => {
          void this.refreshChannels()
            .then(() => this.activeChannel?.syncChangedState())
            .catch((error) => this.fail(error))
        }, 80)
      } else if (event.type === 'submissions') {
        this.pending = event.value as PendingSubmission[]
        if (this.ready) void this.restoreReceipts()
      } else if (event.type === 'navigate' && typeof event.value === 'string')
        void this.switchChannel(event.value)
      else if (event.type === 'menu') {
        if (event.value === 'new-chat') this.newChat()
        if (event.value === 'settings') this.view = 'settings'
      }
    })
    await this.quickPrompts.load()
    for (const chat of this.preferences.chats) this.ensureChannel(chat.source)
    if (this.preferences.activeSource) await this.switchChannel(this.preferences.activeSource)
    else this.newChat()
    if (this.authorized) await this.refresh()
    this.ready = true
    await this.restoreReceipts()
    this.refreshTimer = setInterval(() => {
      if (this.authorized && !this.connection.liveEvents)
        void this.refreshChannels().catch(() => {})
    }, 15_000)
  }
  private requestExtra(source = this.activeSource): Record<string, unknown> {
    const workspace =
      this.preferences.chats.find((c) => c.source === source)?.workspace ||
      workspaceFromCliSource(source) ||
      (source === this.activeSource ? this.ephemeralWorkspace : undefined)
    return {
      source,
      browser_client: 'anda_desktop',
      language: this.preferences.language,
      approval_mode: this.preferences.approvalMode,
      ...(workspace ? { workspace } : {})
    }
  }
  private restoreReceipts(): Promise<void> {
    const recovery = this.receiptRecovery.catch(() => {}).then(() => this.consumeReceipts())
    this.receiptRecovery = recovery
    return recovery
  }
  private async consumeReceipts(): Promise<void> {
    for (const pending of this.pending) {
      if (this.activeSubmissions.has(pending.id)) continue
      if (!['completed', 'failed'].includes(pending.state)) continue
      try {
        const receipt = await window.anda.readSubmission(pending.id)
        if (!receipt || !['completed', 'failed'].includes(receipt.state)) continue
        const output =
          receipt.state === 'failed'
            ? { failed_reason: receipt.error || 'Submission failed' }
            : (receipt.result as Partial<AgentOutput>)
        await this.ensureChannel(pending.source).restoreSubmission(
          pending.id,
          output || {},
          pending.time
        )
        await window.anda.acknowledgeSubmission(pending.id)
        this.pending = this.pending.filter((p) => p.id !== pending.id)
        if (this.systemMessage?.text.includes('SUBMISSION_UNKNOWN')) this.systemMessage = null
      } catch (error) {
        this.fail(error)
      }
    }
  }
  private async submit(input: AgentInput): Promise<AgentSubmission> {
    const id = crypto.randomUUID()
    this.activeSubmissions.add(id)
    const release = () => {
      this.activeSubmissions.delete(id)
      void this.restoreReceipts()
    }
    try {
      const output = await window.anda.rpc<AgentOutput>('agent_run', [$state.snapshot(input)], id)
      return {
        id,
        output,
        finish: async (applied) => {
          try {
            if (applied) await window.anda.acknowledgeSubmission(id)
          } finally {
            release()
          }
        }
      }
    } catch (error) {
      release()
      throw error
    }
  }
  private ensureChannel(source: string): Channel {
    let channel = this.channels.get(source)
    if (!channel) {
      channel = new Channel(source, {
        stateRevision: () =>
          this.authorized && this.connection.liveEvents ? this.eventRevision : undefined,
        activeChannel: () => (document.hidden ? null : this.activeSource),
        requestExtra: async () => this.requestExtra(source),
        agentRun: (input) => this.submit(input),
        rpc: (method, params) => this.rpc(method, params),
        updateStatus: (status, message) => {
          const previous = this.seenStatuses.get(source)
          this.seenStatuses.set(source, status)
          if (this.activeSource === source) {
            this.status = status
            if (message) this.systemMessage = message
          }
          if (
            previous &&
            ['working', 'submitted'].includes(previous) &&
            ['completed', 'failed', 'idle'].includes(status)
          ) {
            void window.anda.notify(
              source,
              this.title(source),
              status === 'failed' ? 'Task needs attention' : 'Anda has a new response'
            )
          }
        }
      })
      this.channels.set(source, channel)
    }
    return channel
  }
  title(source: string): string {
    const title = this.preferences.chats.find((c) => c.source === source)?.title
    if (title) return title
    const workspace = workspaceFromCliSource(source)
    return workspace
      ? workspace.split(/[\\/]/).filter(Boolean).at(-1) || workspace
      : source.startsWith('desktop:')
        ? 'New chat'
        : source
  }
  newChat(workspace?: string): void {
    this.incomingDraft = null
    this.ephemeralWorkspace = workspace
    this.activeChannel = this.ensureChannel(`desktop:${crypto.randomUUID()}`)
    this.status = 'ready'
    this.view = 'chat'
    this.systemMessage = null
  }
  async switchChannel(source: string): Promise<void> {
    this.incomingDraft = null
    this.ephemeralWorkspace = undefined
    this.view = 'chat'
    this.activeChannel = this.ensureChannel(source)
    this.status = this.activeChannel.status
    this.systemMessage = null
    const workspace =
      this.preferences.chats.find((c) => c.source === source)?.workspace ||
      workspaceFromCliSource(source)
    if (workspace && this.authorized) await this.rpc('register_workspace', [workspace])
    await this.savePreferences({ activeSource: source })
    if (this.authorized) {
      await this.activeChannel.init()
      this.activeChannel.wakePolling()
    }
  }
  async refresh(): Promise<void> {
    await Promise.allSettled([
      this.refreshChannels(),
      this.refreshModelState(),
      this.voice.refreshCapabilities()
    ])
    await this.activeChannel?.init({ force: true })
  }
  async refreshChannels(): Promise<void> {
    const {
      output: { result: states }
    } = await this.toolCall<RpcOutput<SourceStateMap>>('conversations_api', {
      type: 'ListSourceState'
    })
    const entries = [...this.preferences.chats]
    let changed = false
    for (const [source, state] of Object.entries(states || {})) {
      const channel = this.ensureChannel(source)
      channel.setSourceState(state)
      if (
        this.connection.liveEvents &&
        source !== this.activeSource &&
        ['working', 'submitted'].includes(state.s || state.status || '')
      )
        void channel.syncChangedState().catch(() => {})
      if (!entries.some((c) => c.source === source)) {
        entries.push({
          source,
          title: this.title(source),
          updatedAt: Date.now()
        })
        changed = true
      }
    }
    if (changed) await this.savePreferences({ chats: entries })
  }
  async savePreferences(patch: Partial<Preferences>): Promise<void> {
    this.preferences = { ...this.preferences, ...patch }
    const snapshot = $state.snapshot(patch)
    const write = this.prefWrites.catch(() => {}).then(() => window.anda.preferences(snapshot))
    this.prefWrites = write
    await write
  }
  async updateChat(source: string, patch: Partial<ChatEntry>): Promise<void> {
    const chats = this.preferences.chats.map((c) => (c.source === source ? { ...c, ...patch } : c))
    await this.savePreferences({ chats })
  }
  async sendPrompt(
    prompt: string,
    attachments: ChatAttachment[],
    memoryMode?: 'standard' | 'no_store' | 'off',
    speak = false
  ): Promise<void> {
    if (!this.activeChannel || !this.authorized)
      throw new Error('Connect to the local daemon before sending')
    const channel = this.activeChannel
    if (!this.preferences.chats.some((c) => c.source === channel.source)) {
      await this.savePreferences({
        chats: [
          ...this.preferences.chats,
          {
            source: channel.source,
            title: prompt.replace(/^\/new\s+/, '').slice(0, 70) || 'New chat',
            workspace: this.ephemeralWorkspace,
            updatedAt: Date.now()
          }
        ],
        activeSource: channel.source
      })
    }
    const workspace = this.preferences.chats.find((c) => c.source === channel.source)?.workspace
    if (workspace) await this.rpc('register_workspace', [workspace])
    const ownsSending = !this.sending && parsePromptCommand(prompt)?.kind !== 'side'
    if (ownsSending) this.sending = true
    try {
      const text = memoryMode ? `/new ${prompt}` : prompt
      const poll = await channel.sendPrompt(text, attachments, memoryMode)
      if (speak && poll) {
        for await (const message of poll) {
          if (message.role === 'assistant' && message.text.trim()) {
            const played = await this.voice.speak(message.text, 'anda')
            if (!played)
              this.fail(
                'Speech playback is unavailable. Configure a daemon TTS provider in Settings.'
              )
          }
        }
      } else poll?.close()
      await this.updateChat(channel.source, { updatedAt: Date.now() })
    } catch (error) {
      this.fail(error)
      if (String(error).includes('SUBMISSION_UNKNOWN')) {
        this.pending = (await window.anda.bootstrap()).pending
        channel.clearConversation()
        await channel.init()
      }
      throw error
    } finally {
      if (ownsSending) this.sending = false
    }
  }
  async stopActiveTask(): Promise<void> {
    this.voice.stopSpeaking()
    if (
      this.sending ||
      this.activeChannel?.sending ||
      ['working', 'submitted', 'sending'].includes(this.activeChannel?.status || '')
    )
      (await this.activeChannel?.sendPrompt('/stop', []))?.close()
  }
  async respondAction(input: {
    actionId: string
    approve?: boolean
    choiceId?: string
    choiceText?: string
  }): Promise<ActionApiOutput> {
    const { output } = await this.toolCall<ActionApiOutput>(
      'actions_api',
      {
        type: 'RespondAction',
        action_id: input.actionId,
        approve: input.approve ?? null,
        choice_id: input.choiceId ?? null,
        choice_text: input.choiceText ?? null
      },
      [],
      {
        ...this.requestExtra(),
        conversation: this.activeChannel?.conversationId || 0
      }
    )
    for (const channel of this.channels.values()) {
      channel.applyActionResponse(output)
      channel.wakePolling()
    }
    return output
  }
  loadResource(resource: Resource): Promise<Resource | null> {
    return this.resources.load(resource)
  }
  async refreshModelState(): Promise<void> {
    const state = await this.rpc<DaemonModelState>('model_names', [])
    this.modelState = {
      activeModel: state.active_model || null,
      modelNames: state.model_names || []
    }
  }
  async setActiveModel(name: string): Promise<void> {
    await this.rpc('set_model', [name])
    await this.refreshModelState()
  }
  async sendVoiceTurn(recording: VoiceRecordingInput): Promise<void> {
    const text =
      recording.transcript?.trim() || (await this.voice.transcribe(recording)).text.trim()
    if (text) await this.sendPrompt(text, [], undefined, Boolean(recording.ttsEnabled))
  }
  async toolCall<Result>(
    name: string,
    args: Record<string, unknown>,
    resources: Resource[] = [],
    meta?: Record<string, unknown>
  ): Promise<ToolOutput<Result>> {
    const result = await this.rpc<ToolOutput<Result>>('tool_call', [
      { name, args, resources, ...(meta ? { meta } : {}) }
    ])
    const error = (result.output as { error?: unknown })?.error
    if (error != null) throw errorToError(error)
    return result
  }
  rpc<Result>(method: string, params: unknown[]): Promise<Result> {
    return window.anda.rpc<Result>(method, $state.snapshot(params))
  }
  fail(error: unknown): void {
    this.systemMessage = {
      kind: 'error',
      text: error instanceof Error ? error.message : String(error)
    }
  }
  async consumeNavigation(): Promise<void> {
    const state = await window.anda.storageGet([
      'andaBookmarkJumpRequest',
      'andaPromptDraftRequest'
    ])
    this.view = 'chat'
    const request = state.andaBookmarkJumpRequest as
      | {
          bookmark?: {
            source: string
            message_id: string
            conversation: number
          }
          createdAt?: number
        }
      | undefined
    const bookmark = request?.bookmark
    if (bookmark && (request?.createdAt || 0) > Date.now() - 5 * 60_000) {
      await this.switchChannel(bookmark.source)
      await this.activeChannel?.loadConversationForJump(bookmark.conversation)
      this.jumpMessage = bookmark.message_id
    }
    const draft = state.andaPromptDraftRequest as
      | { id: string; text: string; createdAt: number }
      | undefined
    if (draft && draft.createdAt > Date.now() - 5 * 60_000) this.incomingDraft = draft
    await window.anda.storageSet({ andaBookmarkJumpRequest: null, andaPromptDraftRequest: null })
  }
  dispose(): void {
    this.voice.stopSpeaking()
    clearTimeout(this.eventRefresh)
    clearTimeout(this.draftTimer)
    clearInterval(this.refreshTimer)
    this.unsubscribe?.()
    for (const channel of this.channels.values()) channel.destroy()
  }
}
