<script lang="ts">
  import { focusDialog } from './dialog'
  import { onDestroy, tick, untrack } from 'svelte'
  import { provideAndaClient } from '$lib/anda/client/context'
  import { applyAppearanceTheme } from '$lib/anda/theme'
  import pandaLogo from '../../../anda_bot/assets/logo.png'
  import ChatComposer from '$lib/anda/ChatComposer.svelte'
  import ChatMessageItem from '$lib/anda/ChatMessageItem.svelte'
  import ActionDock from '$lib/anda/ActionDock.svelte'
  import { displayMessages } from '$lib/anda/chat/message-display'
  import { actionPending } from '$lib/anda/chat/action-view'
  import { firstLine, toolCallStatus, toolCallSummary } from '$lib/anda/chat/tool-view'
  import DropdownMenu from '$lib/anda/DropdownMenu.svelte'
  import Modal from '$lib/anda/Modal.svelte'
  import { delay } from '$lib/utils/async'
  import { getMessage } from '$lib/i18n'
  import { openChatGptUrl, usageUrl } from '$lib/anda/chatgpt/api'
  import MemoryWorkspace from '$lib/anda/memory/MemoryWorkspace.svelte'
  import SkillsWorkspace from '$lib/anda/dashboard/SkillsWorkspace.svelte'
  import BookmarksWorkspace from '$lib/anda/dashboard/BookmarksWorkspace.svelte'
  import {
    ArrowDown,
    ArrowLeft,
    ArrowRight,
    ArrowUpRight,
    BrainCircuit,
    ChevronDown,
    Circle,
    Folder,
    Gauge,
    GitBranch,
    GitCompare,
    Globe,
    LoaderCircle,
    PanelLeft,
    Paperclip,
    Search,
    Sparkles,
    SquareTerminal,
    X
  } from '@lucide/svelte'
  import type { DesktopClient } from './client.svelte'
  import { defaultPreferences, type ChatEntry } from '../shared/contract'
  import type { GitBranchInfo } from '../shared/workbench'
  import { shortcutLabel, type MenuAction } from '../shared/shortcuts'
  import type { ChatAttachment, ChatMessage, Conversation, RpcOutput } from '$lib/anda/client/types'
  import { label, type Label } from './labels'
  import { chatWorkspace, folderName, groupChats, isUnread, type ChatState } from './chat-list'
  import {
    conversationMarkdown,
    editedFiles,
    fileTarget,
    workspaceRelative,
    type EditedFile,
    type FileTarget
  } from './transcript'
  import {
    cacheHitPercent,
    contextPercent,
    formatElapsed,
    formatTokens,
    matchesShortcut,
    modelLabel
  } from './presentation'
  import { tip } from './tooltip'
  import Automations from './Automations.svelte'
  import Sidebar, { type RuntimeActions } from './Sidebar.svelte'
  import WorkbenchPanel, { type FileRequest, type PanelTab } from './WorkbenchPanel.svelte'
  import SettingsPage, { type SettingsCategory } from './SettingsPage.svelte'
  import EditedFiles from './EditedFiles.svelte'
  import TurnIndex from './TurnIndex.svelte'
  import UpdateDialog from './UpdateDialog.svelte'
  import ModelSetup from './ModelSetup.svelte'
  let { client }: { client: DesktopClient } = $props()
  provideAndaClient(untrack(() => client))
  const t = (key: Label, values?: Record<string, string>) => {
    let text = label(client.preferences.language, key)
    for (const [name, value] of Object.entries(values || {}))
      text = text.replaceAll(`{${name}}`, value)
    return text
  }
  const keys = (action: MenuAction) => shortcutLabel(action, client.platform)
  async function checkUpdate(): Promise<void> {
    client.updateDialogOpen = true
    try {
      await window.anda.checkUpdate()
    } catch (error) {
      client.updateStatus = {
        phase: 'error',
        message: error instanceof Error ? error.message : String(error)
      }
    }
  }
  /** The status bar's update button: a download stays inline, a restart shows its progress. */
  async function continueUpdate(): Promise<void> {
    const running = client.updateStatus?.phase === 'running'
    if (running || client.updateOffer?.ready) client.updateDialogOpen = true
    if (running) return
    try {
      if ((await window.anda.continueUpdate())?.phase === 'error') client.updateDialogOpen = true
    } catch (error) {
      client.updateStatus = {
        phase: 'error',
        message: error instanceof Error ? error.message : String(error)
      }
      client.updateDialogOpen = true
    }
  }
  let pageVisible = $state(!document.hidden)
  let collapsed = $state(false)
  let rightOpen = $state(false)
  let rightTab = $state<PanelTab>('resources')
  let panelMaximized = $state(false)
  let searchOpen = $state(false)
  let query = $state('')
  let searchBusy = $state(false)
  let searchResults = $state<Conversation[]>([])
  let showArchived = $state(false)
  let sidebarFilter = $state('')
  let renameSource = $state('')
  let renameTitle = $state('')
  let renameWhere = $state<'sidebar' | 'header'>('sidebar')
  let settingsCategory = $state<SettingsCategory>('general')
  let reloadingModels = $state(false)
  let confirmStopOpen = $state(false)
  let dark = $state(document.documentElement.classList.contains('dark'))
  let panel = $state<WorkbenchPanel | null>(null)
  let dock = $state<ActionDock | null>(null)
  let fileRequest = $state<FileRequest | null>(null)
  let changeFocus = $state<{ id: number; path: string } | null>(null)
  let requestId = 0
  let branchInfo = $state<GitBranchInfo>({ repository: false, branch: '' })
  // The sidebar's width is dragged from its edge and saved on release; until
  // someone resizes it the narrow-window defaults in style.css apply.
  const SIDEBAR_MIN = 200
  const SIDEBAR_MAX = 400
  const PANEL_MIN = 300
  const MAIN_MIN = 340
  let sidebar = $state<HTMLElement | null>(null)
  let panelElement = $state<HTMLElement | null>(null)
  let draggedWidth = $state<number | null>(null)
  let draggedPanelWidth = $state<number | null>(null)
  const sidebarWidth = $derived(draggedWidth ?? client.preferences.sidebarWidth)
  const panelWidth = $derived(draggedPanelWidth ?? client.preferences.panelWidth)
  let scrollArea = $state<HTMLElement | null>(null)
  let following = $state(true)
  let scrollPositions = new Map<string, number>()
  let renderedSource = ''
  let activePrompt = $state('')
  let now = $state(Date.now())
  const workingSince = new Map<string, number>()
  const channel = $derived(client.activeChannel)
  const groups = $derived(channel?.messageGroups || [])
  const messages = $derived([
    ...groups.flatMap((group) => group.messages),
    ...(channel?.sideMessages || [])
  ])
  const resources = $derived(messages.flatMap((message) => message.attachments || []))
  // Each agent turn reads as one flow: tool results fold into their calls.
  const transcriptGroups = $derived(
    groups.map((group) => ({ ...group, messages: displayMessages(group.messages) }))
  )
  const transcriptSideMessages = $derived(displayMessages(channel?.sideMessages || []))
  const submitting = $derived(client.sending || Boolean(channel?.sending))
  const working = $derived(
    ['working', 'submitted', 'sending'].includes(channel?.status || '') || client.sending
  )
  const isEmpty = $derived(!messages.length && !channel?.syncing)
  const activeEntry = $derived(
    client.preferences.chats.find((c) => c.source === client.activeSource)
  )
  const currentPending = $derived(
    client.pending.filter((p) => p.source === client.activeSource && p.state === 'unknown')
  )
  // Sidebar lists: a title filter searches every chat; otherwise pinned chats
  // sit apart and the rest follow the chosen grouping.
  const filterText = $derived(sidebarFilter.trim().toLowerCase())
  const listedChats = $derived(
    [...client.preferences.chats]
      .filter((c) =>
        filterText
          ? c.title.toLowerCase().includes(filterText)
          : Boolean(c.archived) === showArchived ||
            (!showArchived &&
              ['working', 'submitted'].includes(client.channels.get(c.source)?.status || ''))
      )
      .sort((a, b) => b.updatedAt - a.updatedAt)
  )
  const pinnedChats = $derived(
    filterText || showArchived ? [] : listedChats.filter((c) => c.pinned)
  )
  const chatSections = $derived(
    groupChats(
      pinnedChats.length ? listedChats.filter((c) => !c.pinned) : listedChats,
      filterText ? 'none' : client.preferences.sidebarGroup
    )
  )
  const navigableChats = $derived([...pinnedChats, ...chatSections.flatMap((s) => s.chats)])
  const searchChats = $derived(
    client.preferences.chats.filter((c) => c.title.toLowerCase().includes(query.toLowerCase()))
  )
  const pendingActions = $derived(
    messages.flatMap((message) => (message.actions || []).filter(actionPending))
  )
  const prompts = $derived(
    transcriptGroups.flatMap((group) =>
      group.messages
        .filter((message) => message.role === 'user' && message.text.trim())
        .map((message) => ({ id: message.id, text: firstLine(message.text) }))
    )
  )
  // The files each finished turn edited, keyed by the turn's last message. A
  // conversation can hold several prompts, so a turn runs to the next prompt.
  const turnFiles = $derived.by(() => {
    const files = new Map<string, EditedFile[]>()
    for (const group of transcriptGroups) {
      let turn: ChatMessage[] = []
      const finish = () => {
        const edited = turn.length ? editedFiles(turn) : []
        if (edited.length) files.set(turn.at(-1)!.id, edited)
      }
      for (const message of group.messages) {
        if (message.role === 'user' && turn.length) {
          finish()
          turn = []
        }
        turn.push(message)
      }
      if (!['working', 'submitted'].includes(group.status)) finish()
    }
    return files
  })
  const sessionFiles = $derived([
    ...new Set(
      [...turnFiles.values()].flat().map((file) => workspaceRelative(file.path, client.workspace))
    )
  ])
  const currentStep = $derived.by(() => {
    if (!working) return ''
    const tools = (transcriptGroups.at(-1)?.messages || []).flatMap((m) => m.tools || [])
    const tool = tools.findLast((call) => toolCallStatus(call) === 'running')
    if (!tool) return ''
    const summary = toolCallSummary(tool)
    return summary ? `${tool.name} · ${summary}` : tool.name
  })
  const elapsed = $derived.by(() => {
    const started = working ? workingSince.get(client.activeSource) : undefined
    return started ? formatElapsed(now - started) : ''
  })
  const usage = $derived(channel?.usage)
  const contextUsage = $derived(channel?.contextUsage)
  const recentFolders = $derived(
    [
      ...new Set(
        [
          ...[...client.preferences.chats]
            .sort((a, b) => b.updatedAt - a.updatedAt)
            .map(chatWorkspace),
          ...client.preferences.projects.map((p) => p.path)
        ].filter(Boolean)
      )
    ].slice(0, 6)
  )
  const CHOOSE_FOLDER = '\u0000choose'
  const NO_FOLDER = '\u0000none'
  const folderItems = $derived([
    ...recentFolders.map((path) => ({ value: path, label: folderName(path), description: path })),
    { value: CHOOSE_FOLDER, label: t('chooseFolder'), separator: recentFolders.length > 0 },
    ...(client.isNewChat && client.workspace ? [{ value: NO_FOLDER, label: t('noFolder') }] : [])
  ])
  const RELOAD_MODELS = '\u0000reload'
  const MANAGE_USAGE = '\u0000usage'
  const modelItems = $derived([
    ...client.modelState.modelNames.map((name) => ({
      value: name,
      label: modelLabel(name),
      description: modelLabel(name) === name ? undefined : name
    })),
    ...(client.authorized
      ? [
          {
            value: RELOAD_MODELS,
            label: t('reloadModels'),
            separator: client.modelState.modelNames.length > 0
          }
        ]
      : []),
    ...(client.modelState.activeModel?.startsWith('chatgpt:')
      ? [{ value: MANAGE_USAGE, label: getMessage('chatgptManageUsage') }]
      : [])
  ])
  const panelTabs: { id: PanelTab; label: Label; icon: typeof Paperclip; action: MenuAction }[] = [
    { id: 'resources', label: 'resources', icon: Paperclip, action: 'panel:resources' },
    { id: 'changes', label: 'changes', icon: GitCompare, action: 'panel:changes' },
    { id: 'terminal', label: 'terminal', icon: SquareTerminal, action: 'panel:terminal' },
    { id: 'browser', label: 'browser', icon: Globe, action: 'panel:browser' }
  ]
  type TitleAction = 'rename' | 'pin' | 'archive' | 'markdown' | 'reveal'
  const titleItems = $derived<{ value: TitleAction; label: string; separator?: boolean }[]>(
    activeEntry
      ? [
          { value: 'rename', label: t('rename') },
          { value: 'pin', label: activeEntry.pinned ? t('unpin') : t('pin') },
          { value: 'archive', label: activeEntry.archived ? t('restore') : t('archive') },
          { value: 'markdown', label: t('copyMarkdown'), separator: true },
          ...(client.workspace ? [{ value: 'reveal' as const, label: t('revealWorkspace') }] : [])
        ]
      : []
  )

  $effect(() => applyAppearanceTheme(client.preferences.theme))
  $effect(() => {
    const observer = new MutationObserver(
      () => (dark = document.documentElement.classList.contains('dark'))
    )
    observer.observe(document.documentElement, { attributes: true, attributeFilter: ['class'] })
    return () => observer.disconnect()
  })
  $effect(() => {
    if (
      client.ready &&
      client.needsModelSetup &&
      !client.modelSetupAcknowledged &&
      !client.updateDialogOpen
    )
      client.modelSetupOpen = true
  })
  $effect(() => {
    const source = client.activeSource
    if (source !== renderedSource) {
      if (renderedSource && scrollArea) scrollPositions.set(renderedSource, scrollArea.scrollTop)
      renderedSource = source
      following = true
      void tick().then(() => {
        if (!scrollArea) return
        scrollArea.scrollTop = scrollPositions.get(source) ?? scrollArea.scrollHeight
        // A chat shorter than the window has no content below it.
        following = nearBottom(scrollArea)
        updateActivePrompt()
      })
    }
  })
  $effect(() => {
    const _length = messages.length
    const _text = messages.at(-1)?.text
    if (following)
      void tick().then(() => {
        if (scrollArea) scrollArea.scrollTop = scrollArea.scrollHeight
        updateActivePrompt()
      })
  })
  $effect(() => {
    const ids = groups.map((g) => g._id).filter((id) => id > 0)
    if (client.authorized && ids.length)
      void client.bookmarks.loadConversations(ids).catch(() => {})
  })
  $effect(() => {
    const target = client.jumpMessage
    if (target)
      void tick().then(() =>
        document
          .querySelector(`[data-message-id="${CSS.escape(target)}"]`)
          ?.scrollIntoView({ block: 'center' })
      )
  })
  // A chat on screen is read.
  $effect(() => {
    const entry = activeEntry
    if (entry && client.view === 'chat' && pageVisible && isUnread(entry))
      untrack(() => client.markRead(entry.source))
  })
  // The header's branch chip; refreshed when the folder changes or the window returns.
  $effect(() => {
    const workspace = client.workspace
    const _visible = pageVisible
    branchInfo = { repository: false, branch: '' }
    if (!workspace) return
    let current = true
    void window.anda
      .git<GitBranchInfo>({ action: 'branch', workspace })
      .then((info) => {
        if (current) branchInfo = info
      })
      .catch(() => {})
    return () => {
      current = false
    }
  })
  // Elapsed time of the running turn.
  $effect(() => {
    const source = client.activeSource
    if (!working) {
      workingSince.delete(source)
      return
    }
    if (!workingSince.has(source)) workingSince.set(source, Date.now())
    now = Date.now()
    const timer = setInterval(() => (now = Date.now()), 1000)
    return () => clearInterval(timer)
  })

  // Back and forward walk the chats and pages visited in this window.
  let visits = $state<{ view: string; source: string }[]>([])
  let visitIndex = $state(-1)
  let traveling = false
  $effect(() => {
    const view = client.view
    const visit = { view, source: view === 'chat' ? client.activeSource : '' }
    untrack(() => {
      if (traveling) {
        traveling = false
        return
      }
      const current = visits[visitIndex]
      if (current?.view === visit.view && current.source === visit.source) return
      visits = [...visits.slice(0, visitIndex + 1), visit].slice(-50)
      visitIndex = visits.length - 1
    })
  })
  function travel(step: number) {
    const target = visits[visitIndex + step]
    if (!target) return
    traveling = true
    visitIndex += step
    if (target.view === 'chat') void client.switchChannel(target.source)
    else client.view = target.view
  }
  function stepChat(step: number) {
    const list = navigableChats
    if (!list.length) return
    const index = list.findIndex((c) => c.source === client.activeSource)
    const next =
      list[
        index < 0 ? (step > 0 ? 0 : list.length - 1) : (index + step + list.length) % list.length
      ]
    if (next) void client.switchChannel(next.source)
  }
  function togglePanel(tab: PanelTab) {
    if (client.view !== 'chat') {
      client.view = 'chat'
      rightOpen = true
    } else rightOpen = !(rightOpen && rightTab === tab)
    rightTab = tab
    if (!rightOpen) panelMaximized = false
  }
  function menuCommand(action: string) {
    if (action === 'search') openSearch(sidebarFilter)
    else if (action === 'toggle-sidebar') collapsed = !collapsed
    else if (action === 'back') travel(-1)
    else if (action === 'forward') travel(1)
    else if (action === 'previous-chat') stepChat(-1)
    else if (action === 'next-chat') stepChat(1)
    else if (action === 'find') panel?.find()
    else if (action.startsWith('panel:')) togglePanel(action.slice(6) as PanelTab)
  }
  const stopMenu = window.anda.onEvent((event) => {
    if (event.type === 'menu') menuCommand(String(event.value))
    // The agent opened a page in this chat's browser: show it.
    else if (
      event.type === 'browser-reveal' &&
      client.view === 'chat' &&
      event.value === client.activeSource
    ) {
      rightOpen = true
      rightTab = 'browser'
    }
  })
  onDestroy(() => {
    stopMenu()
    client.dispose()
  })

  function keydown(event: KeyboardEvent) {
    if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === 'k') {
      event.preventDefault()
      if (searchOpen) searchOpen = false
      else openSearch(sidebarFilter)
    }
    if (pendingActions.length && client.view === 'chat') {
      if (matchesShortcut(event, 'approve', client.platform)) {
        event.preventDefault()
        void dock?.respond(true)
      } else if (matchesShortcut(event, 'deny', client.platform)) {
        event.preventDefault()
        void dock?.respond(false)
      }
    }
    if (event.key === 'Escape') {
      searchOpen = false
      renameSource = ''
    }
  }
  function openSearch(text: string) {
    query = text.trim()
    searchResults = []
    searchOpen = true
    if (query) void searchHistory()
  }
  async function addProject() {
    try {
      const path = await window.anda.chooseWorkspace()
      if (!path) return
      if (!client.preferences.projects.some((p) => p.path === path))
        await client.savePreferences({
          projects: [
            ...client.preferences.projects,
            { id: crypto.randomUUID(), path, name: folderName(path) }
          ]
        })
      client.setNewChatWorkspace(path)
    } catch (error) {
      client.fail(error)
    }
  }
  function chooseFolder(value: string) {
    if (value === CHOOSE_FOLDER) void addProject()
    else client.setNewChatWorkspace(value === NO_FOLDER ? undefined : value)
  }
  async function send(payload: { text: string; attachments: ChatAttachment[] }) {
    try {
      await client.sendPrompt(payload.text, payload.attachments)
    } catch (error) {
      client.fail(error)
      throw error
    }
    following = true
  }
  function openAgentConfig() {
    client.view = 'settings'
    settingsCategory = 'config'
  }
  async function searchHistory() {
    if (!query.trim() || !client.authorized) return
    searchBusy = true
    try {
      searchResults =
        (
          await client.toolCall<RpcOutput<Conversation[]>>('conversations_api', {
            type: 'SearchConversations',
            query,
            limit: 30
          })
        ).output.result || []
    } catch (error) {
      client.fail(error)
    } finally {
      searchBusy = false
    }
  }
  async function openHistory(conversation: Conversation) {
    const source =
      typeof conversation.extra?.source === 'string'
        ? conversation.extra.source
        : `desktop:history:${conversation._id}`
    await client.switchChannel(source)
    await client.activeChannel?.loadConversationForJump(conversation._id)
    searchOpen = false
  }
  async function acknowledge(id: string) {
    await window.anda.acknowledgeSubmission(id)
    client.pending = client.pending.filter((p) => p.id !== id)
  }
  async function reconnect() {
    try {
      client.connection = await window.anda.connect()
      if (client.authorized) await client.refresh()
    } catch (error) {
      client.fail(error)
    }
  }
  const runtime: RuntimeActions = {
    reconnect,
    restart: async () => {
      try {
        client.connection = await window.anda.control('restart')
        if (client.authorized) await client.refresh()
      } catch (error) {
        client.fail(error)
      }
    },
    stop: () => (confirmStopOpen = true),
    checkUpdate,
    continueUpdate,
    showLogs: () => void window.anda.showLogs(),
    copyToken: async () => {
      try {
        await window.anda.copyExtensionToken()
      } catch (error) {
        client.fail(error)
      }
    }
  }
  async function stopDaemon() {
    confirmStopOpen = false
    try {
      client.connection = await window.anda.control('stop')
    } catch (error) {
      client.fail(error)
    }
  }
  async function reloadModels() {
    if (reloadingModels) return
    reloadingModels = true
    try {
      // The spin stays long enough to register, even when the reload is instant.
      await Promise.all([client.refreshModelState(true), delay(800)])
    } catch (error) {
      client.fail(error)
    } finally {
      reloadingModels = false
    }
  }
  function chooseModel(value: string) {
    if (value === RELOAD_MODELS) void reloadModels()
    else if (value === MANAGE_USAGE) void openChatGptUrl(usageUrl)
    else void client.setActiveModel(value).catch((error) => client.fail(error))
  }
  function startRename(chat: ChatEntry, where: 'sidebar' | 'header') {
    renameWhere = collapsed ? 'header' : where
    renameTitle = chat.title
    renameSource = chat.source
  }
  async function commitRename() {
    const source = renameSource
    if (!source) return
    renameSource = ''
    try {
      await client.updateChat(source, { title: renameTitle.trim() || 'Untitled' })
    } catch (error) {
      client.fail(error)
    }
  }
  async function titleAction(action: TitleAction) {
    const entry = activeEntry
    if (!entry) return
    if (action === 'rename') startRename(entry, 'header')
    else if (action === 'pin') void client.updateChat(entry.source, { pinned: !entry.pinned })
    else if (action === 'archive')
      void client.updateChat(entry.source, { archived: !entry.archived })
    else if (action === 'reveal' && client.workspace)
      void window.anda
        .workspaceFile({ action: 'reveal', workspace: client.workspace })
        .catch((error) => client.fail(error))
    else if (action === 'markdown') {
      try {
        await navigator.clipboard.writeText(
          conversationMarkdown(
            entry.title,
            transcriptGroups.flatMap((group) => group.messages),
            { user: t('you'), assistant: 'Anda' }
          )
        )
        client.systemMessage = { kind: 'info', text: t('copiedMarkdown') }
      } catch (error) {
        client.fail(error)
      }
    }
  }
  function chatState(chat: ChatEntry): ChatState {
    const chatChannel = client.channels.get(chat.source)
    const status = chatChannel?.status || ''
    // The same questions the dock offers; the newest group can be an empty placeholder.
    if (
      chatChannel?.messageGroups.some((group) =>
        group.messages.some((message) => message.actions?.some(actionPending))
      )
    )
      return 'approval'
    if (['working', 'submitted', 'sending'].includes(status)) return 'running'
    if (isUnread(chat)) return status === 'failed' ? 'failed' : 'unread'
    return 'idle'
  }
  function jumpTo(id: string) {
    following = false
    document
      .querySelector(`[data-message-id="${CSS.escape(id)}"]`)
      ?.scrollIntoView({ block: 'start', behavior: 'smooth' })
  }
  function nearBottom(element: HTMLElement) {
    return element.scrollHeight - element.scrollTop - element.clientHeight < 100
  }
  let promptFrame = 0
  function updateActivePrompt() {
    cancelAnimationFrame(promptFrame)
    promptFrame = requestAnimationFrame(() => {
      if (!scrollArea || prompts.length < 3) return
      const top = scrollArea.getBoundingClientRect().top + 90
      let current = prompts[0]!.id
      for (const prompt of prompts) {
        const element = scrollArea.querySelector(`[data-message-id="${CSS.escape(prompt.id)}"]`)
        if (!element) continue
        if (element.getBoundingClientRect().top > top) break
        current = prompt.id
      }
      activePrompt = current
    })
  }
  function openFile(target: FileTarget) {
    rightOpen = true
    rightTab = 'resources'
    fileRequest = { id: ++requestId, ...target }
  }
  function openEditedFile(path: string) {
    if (!branchInfo.repository) return openFile({ path })
    rightOpen = true
    rightTab = 'changes'
    changeFocus = { id: ++requestId, path }
  }
  /** Workspace paths in answers open a preview; web links keep their default. */
  function transcriptClick(event: MouseEvent) {
    const target = event.target as HTMLElement
    if (!target.closest('.md-content')) return
    const link = target.closest<HTMLAnchorElement>('a[href]')
    if (link) {
      let href = link.getAttribute('href') || ''
      try {
        href = decodeURI(href)
      } catch {
        return
      }
      const file = fileTarget(href)
      if (file) {
        event.preventDefault()
        openFile(file)
      }
      return
    }
    const code = target.closest('code')
    if (code && !code.closest('pre') && client.workspace) {
      const file = fileTarget(code.textContent || '')
      if (file) openFile(file)
    }
  }
  /** Inline code that names a file looks like a link once pointed at. */
  function transcriptPointer(event: PointerEvent) {
    const code = (event.target as HTMLElement).closest('code')
    if (!code || code.dataset.fileChecked || code.closest('pre')) return
    code.dataset.fileChecked = '1'
    if (client.workspace && code.closest('.md-content') && fileTarget(code.textContent || ''))
      code.classList.add('file-link')
  }
  function clampSidebar(width: number) {
    return Math.round(Math.min(SIDEBAR_MAX, Math.max(SIDEBAR_MIN, width)))
  }
  function clampPanel(width: number) {
    const sidebarSpace = collapsed ? 0 : (sidebar?.getBoundingClientRect().width ?? 0)
    const max = Math.max(PANEL_MIN, window.innerWidth - sidebarSpace - MAIN_MIN)
    return Math.round(Math.min(max, Math.max(PANEL_MIN, width)))
  }
  /** Pixels an edge's element grows by when the pointer moves `dx` to the right. */
  function growth(dx: number, trailing: boolean) {
    const rtl = document.documentElement.dir === 'rtl'
    return trailing !== rtl ? dx : -dx
  }
  function dragEdge(
    event: PointerEvent,
    element: HTMLElement | null,
    trailing: boolean,
    clamp: (width: number) => number,
    set: (width: number | null) => void,
    save: (width: number) => void
  ) {
    if (event.button !== 0 || !element) return
    const handle = event.currentTarget as HTMLElement
    const startX = event.clientX
    const startWidth = element.getBoundingClientRect().width
    let width: number | null = null
    handle.setPointerCapture(event.pointerId)
    const move = (e: PointerEvent) => {
      width = clamp(startWidth + growth(e.clientX - startX, trailing))
      set(width)
    }
    const end = () => {
      handle.removeEventListener('pointermove', move)
      handle.removeEventListener('pointerup', end)
      handle.removeEventListener('pointercancel', end)
      if (width !== null) save(width)
      set(null)
    }
    handle.addEventListener('pointermove', move)
    handle.addEventListener('pointerup', end)
    handle.addEventListener('pointercancel', end)
  }
  function startSidebarResize(event: PointerEvent) {
    dragEdge(
      event,
      sidebar,
      true,
      clampSidebar,
      (width) => (draggedWidth = width),
      (width) => void preference({ sidebarWidth: width })
    )
  }
  function startPanelResize(event: PointerEvent) {
    dragEdge(
      event,
      panelElement,
      false,
      clampPanel,
      (width) => (draggedPanelWidth = width),
      (width) => void preference({ panelWidth: width })
    )
  }
  function resizeByKey(
    event: KeyboardEvent,
    element: HTMLElement | null,
    trailing: boolean,
    clamp: (width: number) => number,
    save: (width: number) => void
  ) {
    const step = event.key === 'ArrowRight' ? 16 : event.key === 'ArrowLeft' ? -16 : 0
    if (!step || !element) return
    event.preventDefault()
    save(clamp(element.getBoundingClientRect().width + growth(step, trailing)))
  }
  async function preference(patch: Parameters<DesktopClient['savePreferences']>[0]) {
    try {
      await client.savePreferences(patch)
    } catch (error) {
      client.fail(error)
    }
  }
</script>

<svelte:window onkeydown={keydown} />
<svelte:document onvisibilitychange={() => (pageVisible = !document.hidden)} />
<div
  class:sidebar-collapsed={collapsed}
  class:sidebar-resizing={draggedWidth !== null || draggedPanelWidth !== null}
  class:with-panel={rightOpen && client.view === 'chat'}
  class:workbench-open={rightOpen && rightTab !== 'resources' && client.view === 'chat'}
  class:panel-sized={panelWidth > 0}
  class:panel-maximized={panelMaximized && rightOpen && client.view === 'chat'}
  class="desktop-shell"
  style:--sidebar-width={sidebarWidth === defaultPreferences.sidebarWidth
    ? undefined
    : `${sidebarWidth}px`}
  style:--panel-width={panelWidth > 0 ? `${panelWidth}px` : undefined}
>
  <aside class="sidebar" bind:this={sidebar}>
    <Sidebar
      {client}
      {t}
      pinned={pinnedChats}
      sections={chatSections}
      {chatState}
      bind:filter={sidebarFilter}
      bind:showArchived
      renameSource={renameWhere === 'sidebar' ? renameSource : ''}
      bind:renameTitle
      canBack={visitIndex > 0}
      canForward={visitIndex < visits.length - 1}
      onBack={() => travel(-1)}
      onForward={() => travel(1)}
      onCollapse={() => (collapsed = true)}
      onOpenSearch={openSearch}
      onStartRename={(chat) => startRename(chat, 'sidebar')}
      onCommitRename={() => void commitRename()}
      onCancelRename={() => (renameSource = '')}
      {runtime}
    />
    <!-- A focusable separator is ARIA's window-splitter widget (drag, or arrow
         keys); Svelte's a11y rules only know the static kind. -->
    <!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions -->
    <div
      class="sidebar-resizer"
      role="separator"
      aria-orientation="vertical"
      aria-label={t('resizeSidebar')}
      aria-valuemin={SIDEBAR_MIN}
      aria-valuemax={SIDEBAR_MAX}
      aria-valuenow={sidebarWidth}
      tabindex="0"
      use:tip={t('resizeSidebar')}
      onpointerdown={startSidebarResize}
      onkeydown={(event) =>
        resizeByKey(event, sidebar, true, clampSidebar, (width) =>
          preference({ sidebarWidth: width })
        )}
      ondblclick={() => void preference({ sidebarWidth: defaultPreferences.sidebarWidth })}
    ></div>
  </aside>
  <section
    class="main-column"
    class:empty-chat={client.view === 'chat' && isEmpty}
    class:with-index={prompts.length >= 3}
  >
    <header class="workspace-header">
      <div class="header-left">
        {#if collapsed}<span class="header-history no-drag"
            ><button
              class="icon-button"
              aria-label={t('toggleSidebar')}
              use:tip={{ text: t('toggleSidebar'), shortcut: keys('toggle-sidebar') }}
              onclick={() => (collapsed = false)}><PanelLeft size={17} /></button
            ><button
              class="icon-button"
              aria-label={t('back')}
              disabled={visitIndex <= 0}
              use:tip={{ text: t('back'), shortcut: keys('back') }}
              onclick={() => travel(-1)}><ArrowLeft size={16} /></button
            ><button
              class="icon-button"
              aria-label={t('forward')}
              disabled={visitIndex >= visits.length - 1}
              use:tip={{ text: t('forward'), shortcut: keys('forward') }}
              onclick={() => travel(1)}><ArrowRight size={16} /></button
            ></span
          >{/if}
        {#if client.view !== 'chat'}<span class="header-title">{t(client.view as Label)}</span>
        {:else if titleItems.length}
          {#if renameSource && renameWhere === 'header'}<input
              class="header-rename no-drag"
              aria-label={t('rename')}
              bind:value={renameTitle}
              onkeydown={(event) => {
                if (event.key === 'Enter' && !event.isComposing) void commitRename()
                if (event.key === 'Escape') {
                  event.stopPropagation()
                  renameSource = ''
                }
              }}
              onblur={() => void commitRename()}
              {@attach (node) => {
                queueMicrotask(() => {
                  node.focus()
                  node.select()
                })
              }}
            />{/if}
          <!-- Stays mounted while renaming: Rename is picked from it as it closes. -->
          <DropdownMenu
            class="header-title-menu no-drag {renameSource && renameWhere === 'header'
              ? 'hidden'
              : ''}"
            items={titleItems}
            onSelect={(action) => void titleAction(action)}
            ariaLabel={t('chatActions')}
            title=""
          >
            {#snippet trigger()}<span class="header-title">{client.title(client.activeSource)}</span
              ><ChevronDown size={14} />{/snippet}
          </DropdownMenu>
        {:else}<span class="header-title">{client.title(client.activeSource)}</span>{/if}
        {#if client.view === 'chat' && client.workspace}<span
            class="header-chip"
            title={client.workspace}
            ><Folder size={13} /><span>{folderName(client.workspace)}</span
            >{#if branchInfo.branch}<GitBranch size={12} /><span>{branchInfo.branch}</span
              >{/if}</span
          >{/if}
      </div>
      {#if client.view === 'chat'}<div class="header-actions no-drag">
          {#each panelTabs as item (item.id)}<button
              class:pressed={rightOpen && rightTab === item.id}
              class="icon-button"
              aria-label={t(item.label)}
              aria-pressed={rightOpen && rightTab === item.id}
              use:tip={{ text: t(item.label), shortcut: keys(item.action) }}
              onclick={() => togglePanel(item.id)}><item.icon size={17} /></button
            >{/each}
        </div>{/if}
    </header>
    {#if !client.ready}<div class="startup">
        <img class="anda-logo" src={pandaLogo} alt="Anda" />
        <p>{t('loading')}</p>
      </div>
    {:else if client.view === 'chat'}
      {#if client.readOnly}<div class="status-banner">
          <span>{t('readOnly')}</span><button onclick={() => client.newChat()}
            >{t('newChat')}</button
          >
        </div>{/if}
      {#if client.connection.needsSetup}<div class="status-banner">
          <span>{t(client.needsModelSetup ? 'setupMissingModel' : 'setupRepair')}</span>
          <button
            onclick={() =>
              client.needsModelSetup ? (client.modelSetupOpen = true) : openAgentConfig()}
          >
            {t(client.needsModelSetup ? 'connectModel' : 'setupAdvanced')}
          </button>
        </div>
      {:else if !client.authorized}<div class="status-banner">
          <span>{client.connection.error || t('disconnected')}</span><button
            onclick={() => void reconnect()}>{t('reconnect')}</button
          >
        </div>{/if}
      {#if client.systemMessage}<div
          class:error={client.systemMessage.kind === 'error'}
          class="status-banner"
        >
          <span>{client.systemMessage.text}</span><button
            class="icon-button"
            aria-label={t('close')}
            onclick={() => (client.systemMessage = null)}><X size={14} /></button
          >
        </div>{/if}
      {#each currentPending as pending (pending.id)}<div class="uncertain-banner">
          <p>{t('unconfirmed')}</p>
          <blockquote>{pending.prompt}</blockquote>
          <button onclick={() => void acknowledge(pending.id)}>{t('reviewed')}</button>
        </div>{/each}
      {#if prompts.length >= 3}<TurnIndex
          {prompts}
          active={activePrompt}
          {t}
          onJump={jumpTo}
        />{/if}
      <div
        class="conversation-scroll"
        bind:this={scrollArea}
        onscroll={(event) => {
          following = nearBottom(event.currentTarget)
          updateActivePrompt()
        }}
      >
        {#if isEmpty}<div class="welcome">
            <img class="anda-logo" src={pandaLogo} alt="Anda" />
            <h1>{t('welcome')}</h1>
            <p>{t('intro')}</p>
          </div>
        {:else}<!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
          <div class="transcript" onclick={transcriptClick} onpointerover={transcriptPointer}>
            {#if channel?.hasPreviousConversations}<button
                class="history-button"
                onclick={() => void channel?.loadPreviousConversations()}>{t('history')}</button
              >{/if}{#each transcriptGroups as group (group._id)}{#each group.messages as message (message.id)}<div
                  data-message-id={message.id}
                  class="transcript-item"
                >
                  <ChatMessageItem
                    {message}
                    quickPromptActive={client.quickPrompts.has(message.text)}
                    onToggleQuickPrompt={(text) => client.quickPrompts.toggle(text)}
                    compactActions
                    groupTools
                  />
                </div>
                {#if turnFiles.get(message.id)}<EditedFiles
                    files={turnFiles.get(message.id)!}
                    workspace={client.workspace}
                    {t}
                    onOpen={openEditedFile}
                  />{/if}{/each}{/each}{#each transcriptSideMessages as message (message.id)}<div
                class="transcript-item"
              >
                <ChatMessageItem {message} compactActions groupTools />
              </div>{/each}{#if working}<div class="working-indicator" role="status">
                <span class="working-dot"></span><span>{t('working')}</span>{#if elapsed}<span
                    class="working-elapsed">{elapsed}</span
                  >{/if}{#if currentStep}<code title={currentStep}>{currentStep}</code>{/if}
              </div>{/if}
          </div>{/if}
      </div>
      {#if !following && messages.length}<button
          class="jump-bottom"
          onclick={() => {
            following = true
            scrollArea?.scrollTo({ top: scrollArea.scrollHeight, behavior: 'smooth' })
          }}><ArrowDown size={14} />{t('newContent')}</button
        >{/if}
      <footer class="composer-footer">
        <div class="composer-container">
          <ActionDock
            bind:this={dock}
            pending={pendingActions}
            onReply={currentPending.length ? undefined : (text) => send({ text, attachments: [] })}
            shortcuts={{ approve: keys('approve'), deny: keys('deny') }}
          />
          {#key client.activeSource}<ChatComposer
              disabled={(!client.authorized && !client.needsModelSetup) ||
                client.readOnly ||
                currentPending.length > 0}
              connectAction={client.needsModelSetup
                ? { label: t('connectModel'), run: () => (client.modelSetupOpen = true) }
                : undefined}
              placeholder={t('prompt')}
              sending={submitting}
              {working}
              stoppable={(working || client.voice.speaking) && !client.readOnly}
              onSend={send}
              onStop={() => client.stopActiveTask()}
              voiceEnabled={pageVisible && client.authorized}
              voiceAvailable={client.authorized &&
                client.voice.capabilities.transcription.length > 0}
              voiceCapabilities={client.voice.capabilities}
              onVoiceSend={(recording) => client.sendVoiceTurn(recording)}
              approvalMode={client.preferences.approvalMode}
              onApprovalModeChange={(mode) => preference({ approvalMode: mode })}
              submitKeyMode={client.preferences.submitKeyMode}
              onLoadSkills={client.authorized ? () => client.skills.listPrompts() : undefined}
              quickPrompts={client.quickPrompts.items}
              onRemoveQuickPrompt={(prompt) => client.quickPrompts.remove(prompt.text)}
              onClearQuickPrompts={() => client.quickPrompts.clear()}
              onUseQuickPrompt={(prompt) => client.quickPrompts.use(prompt.text)}
              incomingDraft={client.incomingDraft}
              initialDraft={client.getDraft(client.activeSource)}
              onDraftChange={(draft) => client.saveDraft(client.activeSource, draft)}
            >
              {#snippet actions()}
                {#if contextUsage && contextUsage.tokens > 0}
                  {@const percent = contextPercent(contextUsage.tokens, contextUsage.window)}
                  {@const used = formatTokens(contextUsage.tokens)}
                  {@const capacity = formatTokens(contextUsage.window)}
                  {@const lines = [
                    percent === undefined
                      ? t('contextUsageNoWindow', { used })
                      : t('contextUsage', { used, window: capacity, percent: String(percent) }),
                    ...(usage
                      ? [
                          t('conversationUsage', {
                            input: formatTokens(usage.input_tokens),
                            cached: formatTokens(usage.cached_tokens),
                            hitRate: String(
                              cacheHitPercent(usage.input_tokens, usage.cached_tokens)
                            ),
                            output: formatTokens(usage.output_tokens),
                            requests: String(usage.requests)
                          })
                        ]
                      : [])
                  ]}
                  <span
                    class="usage-meter"
                    class:usage-meter-high={percent !== undefined && percent >= 80}
                    role="img"
                    aria-label={lines.join('. ')}
                    use:tip={lines.join('\n')}
                  >
                    {#if percent === undefined}
                      <Gauge size={14} />{used}
                    {:else}
                      <svg class="context-ring" viewBox="0 0 16 16" aria-hidden="true">
                        <circle cx="8" cy="8" r="6" pathLength="100" />
                        <circle
                          class="context-ring-fill"
                          cx="8"
                          cy="8"
                          r="6"
                          pathLength="100"
                          stroke-dasharray="{percent} 100"
                          transform="rotate(-90 8 8)"
                        />
                      </svg>{used} / {capacity}
                    {/if}
                  </span>
                {/if}
                {#if modelItems.length}
                  <DropdownMenu
                    class="composer-model"
                    items={modelItems}
                    value={client.modelState.activeModel || ''}
                    onSelect={chooseModel}
                    ariaLabel="Model"
                    title={t('modelScope')}
                    align="end"
                  >
                    {#snippet trigger()}
                      {#if reloadingModels}<LoaderCircle size={13} class="animate-spin" />{/if}
                      <span class="truncate"
                        >{client.modelState.activeModel
                          ? modelLabel(client.modelState.activeModel)
                          : t('chooseModel')}</span
                      >
                      <ChevronDown size={12} />
                    {/snippet}
                  </DropdownMenu>
                {/if}
              {/snippet}
            </ChatComposer>{/key}
          <div class="composer-context">
            <DropdownMenu
              items={folderItems}
              value={client.workspace || ''}
              onSelect={chooseFolder}
              heading={client.isNewChat ? t('workspaceFor') : t('newChatIn')}
              ariaLabel={t('workspaceFor')}
              title={client.workspace || t('noFolder')}
              align="start"
            >
              {#snippet trigger()}<Folder size={13} /><span class="truncate"
                  >{client.workspace ? folderName(client.workspace) : t('noFolder')}</span
                ><ChevronDown size={12} />{/snippet}
            </DropdownMenu>
          </div>
        </div>
        {#if isEmpty}
          <div class="suggestions">
            <button onclick={addProject}><Folder size={16} />{t('suggestion1')}</button><button
              onclick={() =>
                (client.incomingDraft = {
                  id: crypto.randomUUID(),
                  text: t('organizePrompt'),
                  createdAt: Date.now()
                })}><Sparkles size={16} />{t('suggestion2')}</button
            ><button onclick={() => (client.view = 'memory')}
              ><BrainCircuit size={16} />{t('suggestion3')}</button
            >
          </div>
          {#if recentFolders.length}<div class="recent-projects">
              <span>{t('recentProjects')}</span>
              {#each recentFolders.slice(0, 4) as path (path)}<button
                  class:active={client.workspace === path}
                  title={path}
                  onclick={() => client.setNewChatWorkspace(path)}
                  ><Folder size={13} />{folderName(path)}</button
                >{/each}
            </div>{/if}
        {/if}
      </footer>
    {:else if client.view === 'memory'}<div class="management-page"><MemoryWorkspace /></div>
    {:else if client.view === 'skills'}<div class="management-page"><SkillsWorkspace /></div>
    {:else if client.view === 'bookmarks'}<div class="management-page"><BookmarksWorkspace /></div>
    {:else if client.view === 'automations'}<div class="management-page">
        <Automations {client} />
      </div>
    {:else if client.view === 'settings'}<SettingsPage
        {client}
        {t}
        bind:category={settingsCategory}
        {runtime}
      />{/if}
  </section>
  {#if rightOpen && client.view === 'chat'}<aside class="resource-panel" bind:this={panelElement}>
      <!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions -->
      <div
        class="panel-resizer"
        role="separator"
        aria-orientation="vertical"
        aria-label={t('resizePanel')}
        aria-valuemin={PANEL_MIN}
        aria-valuenow={panelElement?.getBoundingClientRect().width}
        tabindex="0"
        use:tip={t('resizePanel')}
        onpointerdown={startPanelResize}
        onkeydown={(event) =>
          resizeByKey(event, panelElement, false, clampPanel, (width) =>
            preference({ panelWidth: width })
          )}
        ondblclick={() => void preference({ panelWidth: 0 })}
      ></div>
      <WorkbenchPanel
        bind:this={panel}
        {client}
        {t}
        tab={rightTab}
        {dark}
        branch={branchInfo.branch}
        {resources}
        {sessionFiles}
        {fileRequest}
        {changeFocus}
        bind:maximized={panelMaximized}
        onClose={() => {
          rightOpen = false
          panelMaximized = false
        }}
      />
    </aside>{/if}
</div>
{#if searchOpen}<div
    class="modal-backdrop"
    role="presentation"
    onclick={(event) => {
      if (event.target === event.currentTarget) searchOpen = false
    }}
  >
    <div
      use:focusDialog={() => (searchOpen = false)}
      class="search-dialog"
      role="dialog"
      aria-modal="true"
      aria-label={t('search')}
      tabindex="-1"
    >
      <div class="search-input">
        <Search size={19} /><input
          placeholder={t('searchHint')}
          bind:value={query}
          onkeydown={(event) => {
            if (event.key === 'Enter' && !event.isComposing) void searchHistory()
          }}
        /><button class="icon-button" aria-label={t('close')} onclick={() => (searchOpen = false)}
          ><X size={17} /></button
        >
      </div>
      <div class="search-results">
        {#each searchChats as chat (chat.source)}<button
            onclick={() => {
              void client.switchChannel(chat.source)
              searchOpen = false
            }}><Circle size={12} /><span>{chat.title}</span><ArrowUpRight size={14} /></button
          >{/each}{#each searchResults as result (result._id)}<button
            onclick={() => void openHistory(result)}
            ><Search size={13} /><span>{result.label || `Conversation ${result._id}`}</span
            ><ArrowUpRight size={14} /></button
          >{/each}{#if !searchChats.length && !searchResults.length}<p>
            {searchBusy ? '…' : t('emptySearch')}
          </p>{/if}
      </div>
    </div>
  </div>{/if}
{#if confirmStopOpen}
  <Modal
    bind:open={confirmStopOpen}
    alert
    title={t('confirmStop')}
    contentClass="min-h-0 sm:max-w-md"
  >
    <p class="muted">{t('confirmStopDetail')}</p>
    {#snippet footer()}
      <button class="dialog-button" onclick={() => (confirmStopOpen = false)}>{t('cancel')}</button
      ><button class="danger" onclick={() => void stopDaemon()}>{t('stopDaemon')}</button>
    {/snippet}
  </Modal>
{/if}
{#if client.updateDialogOpen}
  <UpdateDialog
    status={client.updateStatus}
    language={client.preferences.language}
    onclose={() => (client.updateDialogOpen = false)}
    oncheck={() => void checkUpdate()}
  />
{/if}
{#if client.modelSetupOpen && !client.updateDialogOpen}
  <ModelSetup {client} onadvanced={openAgentConfig} />
{/if}
