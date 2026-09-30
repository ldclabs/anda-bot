<script lang="ts">
  import { focusDialog } from './dialog'
  import { onDestroy, tick, untrack } from 'svelte'
  import { provideAndaClient } from '$lib/anda/client/context'
  import { applyAppearanceTheme } from '$lib/anda/theme'
  import { base64ToBytes } from '$lib/utils/base64'
  import pandaLogo from '../../../anda_bot/assets/logo.png'
  import ChatComposer from '$lib/anda/ChatComposer.svelte'
  import ChatMessageItem from '$lib/anda/ChatMessageItem.svelte'
  import DropdownMenu from '$lib/anda/DropdownMenu.svelte'
  import { buttonClass } from '$lib/anda/ui'
  import { delay } from '$lib/utils/async'
  import MemoryWorkspace from '$lib/anda/memory/MemoryWorkspace.svelte'
  import SkillsWorkspace from '$lib/anda/dashboard/SkillsWorkspace.svelte'
  import BookmarksWorkspace from '$lib/anda/dashboard/BookmarksWorkspace.svelte'
  import ConfigApp from '$extension/ConfigApp.svelte'
  import {
    BrainCircuit,
    BookOpen,
    Bookmark,
    Clock3,
    Settings,
    SquarePen,
    Search,
    PanelLeftClose,
    PanelLeft,
    PanelRight,
    Folder,
    ChevronDown,
    ArrowDown,
    X,
    MoreHorizontal,
    Pin,
    Archive,
    RefreshCw,
    Circle,
    ArrowUpRight,
    Paperclip,
    Sparkles
  } from '@lucide/svelte'
  import type { DesktopClient } from './client.svelte'
  import { defaultPreferences, type ChatEntry } from '../shared/contract'
  import type { ChatAttachment, Conversation, RpcOutput, Resource } from '$lib/anda/client/types'
  import { label, type Label } from './labels'
  import Automations from './Automations.svelte'
  import TerminalPanel from './TerminalPanel.svelte'
  import GitPanel from './GitPanel.svelte'
  import BrowserPanel from './BrowserPanel.svelte'
  import AudioPanel from './AudioPanel.svelte'
  import LocaleSwitcher from './LocaleSwitcher.svelte'
  import UpdateDialog from './UpdateDialog.svelte'
  let { client }: { client: DesktopClient } = $props()
  provideAndaClient(untrack(() => client))
  const t = (key: Label) => label(client.preferences.language, key)
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
  let pageVisible = $state(!document.hidden)
  let collapsed = $state(false)
  let rightOpen = $state(false)
  let rightTab = $state('resources')
  let searchOpen = $state(false)
  let query = $state('')
  let searchBusy = $state(false)
  let searchResults = $state<Conversation[]>([])
  let showArchived = $state(false)
  let renameSource = $state('')
  let renameTitle = $state('')
  let settingsTab = $state('general')
  let reloadingModels = $state(false)
  const themeItems = $derived<{ value: 'system' | 'light' | 'dark'; label: string }[]>([
    { value: 'system', label: t('system') },
    { value: 'light', label: t('light') },
    { value: 'dark', label: t('dark') }
  ])
  // The sidebar's width is dragged from its edge and saved on release; until
  // someone resizes it the narrow-window defaults in style.css apply.
  const SIDEBAR_MIN = 200
  const SIDEBAR_MAX = 400
  let sidebar = $state<HTMLElement | null>(null)
  let draggedWidth = $state<number | null>(null)
  const sidebarWidth = $derived(draggedWidth ?? client.preferences.sidebarWidth)
  let scrollArea = $state<HTMLElement | null>(null)
  let following = $state(true)
  let scrollPositions = new Map<string, number>()
  let renderedSource = ''
  let selectedResource = $state<Resource | null>(null)
  let previewText = $state('')
  let previewUrl = $state('')
  let previewError = $state('')
  let resourceGeneration = 0
  const channel = $derived(client.activeChannel)
  const groups = $derived(channel?.messageGroups || [])
  const messages = $derived([
    ...groups.flatMap((group) => group.messages),
    ...(channel?.sideMessages || [])
  ])
  const resources = $derived(messages.flatMap((message) => message.attachments || []))
  const submitting = $derived(client.sending || Boolean(channel?.sending))
  const working = $derived(
    ['working', 'submitted', 'sending'].includes(channel?.status || '') || client.sending
  )
  const activeEntry = $derived(
    client.preferences.chats.find((c) => c.source === client.activeSource)
  )
  const chats = $derived(
    [...client.preferences.chats]
      .filter(
        (c) =>
          Boolean(c.archived) === showArchived ||
          (!showArchived &&
            ['working', 'submitted'].includes(client.channels.get(c.source)?.status || ''))
      )
      .sort((a, b) => Number(b.pinned) - Number(a.pinned) || b.updatedAt - a.updatedAt)
  )
  const searchChats = $derived(
    client.preferences.chats.filter((c) => c.title.toLowerCase().includes(query.toLowerCase()))
  )
  const currentPending = $derived(
    client.pending.filter((p) => p.source === client.activeSource && p.state === 'unknown')
  )
  const navigation = [
    { id: 'memory', text: 'memory' as Label, icon: BrainCircuit },
    { id: 'skills', text: 'skills' as Label, icon: BookOpen },
    { id: 'automations', text: 'automations' as Label, icon: Clock3 },
    { id: 'bookmarks', text: 'bookmarks' as Label, icon: Bookmark }
  ]
  $effect(() => applyAppearanceTheme(client.preferences.theme))
  $effect(() => {
    const source = client.activeSource
    if (source !== renderedSource) {
      if (renderedSource && scrollArea) scrollPositions.set(renderedSource, scrollArea.scrollTop)
      renderedSource = source
      resourceGeneration++
      selectedResource = null
      previewText = ''
      previewError = ''
      if (previewUrl) URL.revokeObjectURL(previewUrl)
      previewUrl = ''
      following = !scrollPositions.has(source)
      void tick().then(() => {
        if (scrollArea)
          scrollArea.scrollTop = scrollPositions.get(source) ?? scrollArea.scrollHeight
      })
    }
  })
  $effect(() => {
    const _length = messages.length
    const _text = messages.at(-1)?.text
    if (following)
      void tick().then(() => {
        if (scrollArea) scrollArea.scrollTop = scrollArea.scrollHeight
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
  onDestroy(() => {
    client.dispose()
    if (previewUrl) URL.revokeObjectURL(previewUrl)
  })

  function keydown(event: KeyboardEvent) {
    if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === 'k') {
      event.preventDefault()
      searchOpen = !searchOpen
    }
    if (event.key === 'Escape') {
      searchOpen = false
      renameSource = ''
    }
  }
  async function addProject() {
    try {
      const path = await window.anda.chooseWorkspace()
      if (!path) return
      if (!client.preferences.projects.some((p) => p.path === path))
        await client.savePreferences({
          projects: [
            ...client.preferences.projects,
            {
              id: crypto.randomUUID(),
              path,
              name: path.split(/[\\/]/).filter(Boolean).at(-1) || path
            }
          ]
        })
      client.newChat(path)
    } catch (error) {
      client.fail(error)
    }
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
  async function showResource(attachment: ChatAttachment) {
    const generation = ++resourceGeneration
    rightOpen = true
    rightTab = 'resources'
    previewText = ''
    previewError = ''
    if (previewUrl) URL.revokeObjectURL(previewUrl)
    previewUrl = ''
    selectedResource = attachment.resource
    try {
      const resource = await client.loadResource(attachment.resource)
      if (generation !== resourceGeneration) return
      if (resource) selectedResource = resource
      if (!resource?.blob) {
        previewError = 'This resource has no downloadable content.'
        return
      }
      const bytes = base64ToBytes(resource.blob)
      const mime = resource.mime_type || 'application/octet-stream'
      if (mime.startsWith('image/') || mime === 'application/pdf' || mime.startsWith('audio/'))
        previewUrl = URL.createObjectURL(new Blob([bytes], { type: mime }))
      else previewText = new TextDecoder().decode(bytes.slice(0, 512_000))
    } catch (error) {
      if (generation === resourceGeneration) previewError = String(error)
    }
  }
  async function acknowledge(id: string) {
    await window.anda.acknowledgeSubmission(id)
    client.pending = client.pending.filter((p) => p.id !== id)
  }
  async function reconnect() {
    client.connection = await window.anda.connect()
    if (client.authorized) await client.refresh()
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
  async function rename() {
    await client.updateChat(renameSource, { title: renameTitle.trim() || 'Untitled' })
    renameSource = ''
  }
  function chatMenu(chat: ChatEntry) {
    return [
      { value: 'rename' as const, label: t('rename') },
      { value: 'pin' as const, label: chat.pinned ? t('unpin') : t('pin') },
      { value: 'archive' as const, label: chat.archived ? t('restore') : t('archive') }
    ]
  }
  function chatAction(chat: ChatEntry, action: 'rename' | 'pin' | 'archive') {
    if (action === 'rename') {
      renameSource = chat.source
      renameTitle = chat.title
    } else if (action === 'pin') void client.updateChat(chat.source, { pinned: !chat.pinned })
    else void client.updateChat(chat.source, { archived: !chat.archived })
  }
  function clampSidebar(width: number) {
    return Math.round(Math.min(SIDEBAR_MAX, Math.max(SIDEBAR_MIN, width)))
  }
  /** Pixels the sidebar grows by when the pointer moves `dx` to the right. */
  function sidebarGrowth(dx: number) {
    return document.documentElement.dir === 'rtl' ? -dx : dx
  }
  function startSidebarResize(event: PointerEvent) {
    if (event.button !== 0 || !sidebar) return
    const handle = event.currentTarget as HTMLElement
    const startX = event.clientX
    const startWidth = sidebar.getBoundingClientRect().width
    handle.setPointerCapture(event.pointerId)
    const move = (e: PointerEvent) => {
      draggedWidth = clampSidebar(startWidth + sidebarGrowth(e.clientX - startX))
    }
    const end = () => {
      handle.removeEventListener('pointermove', move)
      handle.removeEventListener('pointerup', end)
      handle.removeEventListener('pointercancel', end)
      if (draggedWidth !== null) void preference({ sidebarWidth: draggedWidth })
      draggedWidth = null
    }
    handle.addEventListener('pointermove', move)
    handle.addEventListener('pointerup', end)
    handle.addEventListener('pointercancel', end)
  }
  function resizeSidebarByKey(event: KeyboardEvent) {
    const step = event.key === 'ArrowRight' ? 16 : event.key === 'ArrowLeft' ? -16 : 0
    if (!step || !sidebar) return
    event.preventDefault()
    const width = sidebar.getBoundingClientRect().width
    void preference({ sidebarWidth: clampSidebar(width + sidebarGrowth(step)) })
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
  class:sidebar-resizing={draggedWidth !== null}
  class:with-panel={rightOpen && client.view === 'chat'}
  class:workbench-open={rightOpen && rightTab !== 'resources' && client.view === 'chat'}
  class="desktop-shell"
  style:--sidebar-width={sidebarWidth === defaultPreferences.sidebarWidth
    ? undefined
    : `${sidebarWidth}px`}
>
  <aside class="sidebar" bind:this={sidebar}>
    <div class="sidebar-titlebar">
      <span class="wordmark">anda<span class="wordmark-dot">●</span></span><button
        class="icon-button"
        title={t('close')}
        onclick={() => (collapsed = true)}><PanelLeftClose size={17} /></button
      >
    </div>
    <div class="sidebar-primary">
      <button class="nav-row" onclick={() => client.newChat()}
        ><SquarePen size={17} /><span>{t('newChat')}</span><kbd
          >{client.platform === 'darwin' ? '⌘' : 'Ctrl'} N</kbd
        ></button
      >
      <button
        class="nav-row"
        onclick={() => {
          query = ''
          searchOpen = true
        }}
        ><Search size={17} /><span>{t('search')}</span><kbd
          >{client.platform === 'darwin' ? '⌘' : 'Ctrl'} K</kbd
        ></button
      >
    </div>
    <div class="sidebar-navigation">
      {#each navigation as item}<button
          class:active={client.view === item.id}
          class="nav-row"
          onclick={() => (client.view = item.id)}
          ><item.icon size={17} /><span>{t(item.text)}</span></button
        >{/each}
    </div>
    <div class="sidebar-scroll">
      <div class="section-label">
        <button onclick={() => (showArchived = !showArchived)}
          >{showArchived ? t('archived') : t('recent')}<ChevronDown size={12} /></button
        >
      </div>
      {#each chats as chat (chat.source)}
        <div
          class:active={client.view === 'chat' && client.activeSource === chat.source}
          class="chat-row"
        >
          <button class="chat-title" onclick={() => void client.switchChannel(chat.source)}
            >{#if chat.pinned}<Pin size={12} />{/if}<span>{chat.title}</span
            >{#if ['working', 'submitted'].includes(client.channels.get(chat.source)?.status || '')}<span
                class="working-dot"
              ></span>{/if}</button
          >
          <DropdownMenu
            class="chat-more icon-button"
            items={chatMenu(chat)}
            onSelect={(action) => chatAction(chat, action)}
            ariaLabel={t('details')}
            title={t('details')}
            align="end"
          >
            {#snippet trigger()}<MoreHorizontal size={16} />{/snippet}
          </DropdownMenu>
        </div>
      {/each}
      {#if !chats.length}<p class="sidebar-empty">{t('noChats')}</p>{/if}
    </div>
    <div class="sidebar-bottom">
      <button
        class:active={client.view === 'settings'}
        class="nav-row"
        onclick={() => (client.view = 'settings')}
        ><Settings size={17} /><span>{t('settings')}</span></button
      ><button
        class="connection-row"
        title={client.connection.error || client.connection.home}
        onclick={() => void reconnect()}
        ><span class:online={client.authorized} class="connection-dot"></span><span
          >{client.authorized ? t('connected') : t('disconnected')}</span
        ><RefreshCw size={12} /></button
      >
    </div>
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
      onpointerdown={startSidebarResize}
      onkeydown={resizeSidebarByKey}
      ondblclick={() => void preference({ sidebarWidth: defaultPreferences.sidebarWidth })}
    ></div>
  </aside>
  <section class="main-column">
    <header class="workspace-header">
      <div class="header-left">
        {#if collapsed}<button
            class="icon-button no-drag"
            title={t('projects')}
            onclick={() => (collapsed = false)}><PanelLeft size={18} /></button
          >{/if}<span class="header-title"
          >{client.view === 'chat'
            ? client.title(client.activeSource)
            : t(client.view as Label)}</span
        >{#if client.view === 'chat'}<span class="header-divider">/</span><span
            class="header-context">{client.workspace?.split(/[\\/]/).at(-1) || t('local')}</span
          >{/if}
      </div>
      <div class="header-actions no-drag">
        {#if client.view === 'chat'}<button
            class:pressed={rightOpen}
            class="icon-button"
            title={t('resources')}
            onclick={() => (rightOpen = !rightOpen)}><PanelRight size={18} /></button
          >{/if}
      </div>
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
      {#if !client.authorized}<div class="status-banner">
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
            onclick={() => (client.systemMessage = null)}><X size={14} /></button
          >
        </div>{/if}
      {#each currentPending as pending}<div class="uncertain-banner">
          <p>{t('unconfirmed')}</p>
          <blockquote>{pending.prompt}</blockquote>
          <button onclick={() => void acknowledge(pending.id)}>{t('reviewed')}</button>
        </div>{/each}
      <div
        class="conversation-scroll"
        bind:this={scrollArea}
        onscroll={(event) =>
          (following =
            event.currentTarget.scrollHeight -
              event.currentTarget.scrollTop -
              event.currentTarget.clientHeight <
            100)}
      >
        {#if !messages.length && !channel?.syncing}<div class="welcome">
            <img class="anda-logo" src={pandaLogo} alt="Anda" />
            <h1>{t('welcome')}</h1>
            <p>{t('intro')}</p>
            <div class="suggestions">
              <button onclick={addProject}><Folder size={17} />{t('suggestion1')}</button><button
                onclick={() =>
                  (client.incomingDraft = {
                    id: crypto.randomUUID(),
                    text: t('organizePrompt'),
                    createdAt: Date.now()
                  })}><Sparkles size={17} />{t('suggestion2')}</button
              ><button onclick={() => (client.view = 'memory')}
                ><BrainCircuit size={17} />{t('suggestion3')}</button
              >
            </div>
          </div>
        {:else}<div class="transcript">
            {#if channel?.hasPreviousConversations}<button
                class="history-button"
                onclick={() => void channel?.loadPreviousConversations()}>{t('history')}</button
              >{/if}{#each groups as group (group._id)}{#each group.messages as message (message.id)}<div
                  data-message-id={message.id}
                  class="transcript-item"
                >
                  <ChatMessageItem
                    {message}
                    quickPromptActive={client.quickPrompts.has(message.text)}
                    onToggleQuickPrompt={(text) => client.quickPrompts.toggle(text)}
                  />
                </div>{/each}{/each}{#each channel?.sideMessages || [] as message (message.id)}<ChatMessageItem
                {message}
              />{/each}{#if working}<div class="working-indicator">
                <span class="working-dot"></span>{t('working')}
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
          {#key client.activeSource}<ChatComposer
              disabled={!client.authorized || client.readOnly || currentPending.length > 0}
              placeholder={t('prompt')}
              sending={submitting}
              {working}
              stoppable={(working || client.voice.speaking) && !client.readOnly}
              onSend={send}
              onStop={() => client.stopActiveTask()}
              voiceEnabled={pageVisible}
              voiceAvailable={client.voice.capabilities.transcription.length > 0}
              voiceCapabilities={client.voice.capabilities}
              onVoiceSend={(recording) => client.sendVoiceTurn(recording)}
              approvalMode={client.preferences.approvalMode}
              onApprovalModeChange={(mode) => preference({ approvalMode: mode })}
              submitKeyMode={client.preferences.submitKeyMode}
              onLoadSkills={() => client.skills.listPrompts()}
              quickPrompts={client.quickPrompts.items}
              onRemoveQuickPrompt={(prompt) => client.quickPrompts.remove(prompt.text)}
              onClearQuickPrompts={() => client.quickPrompts.clear()}
              onUseQuickPrompt={(prompt) => client.quickPrompts.use(prompt.text)}
              incomingDraft={client.incomingDraft}
              initialDraft={client.getDraft(client.activeSource)}
              onDraftChange={(draft) => client.saveDraft(client.activeSource, draft)}
            >
              {#snippet actions()}
                {#if client.modelState.modelNames.length}
                  <DropdownMenu
                    class="composer-model"
                    items={client.modelState.modelNames.map((name) => ({
                      value: name,
                      label: name
                    }))}
                    value={client.modelState.activeModel || ''}
                    onSelect={(name) =>
                      void client.setActiveModel(name).catch((error) => client.fail(error))}
                    ariaLabel="Model"
                    title={t('modelScope')}
                    align="end"
                  >
                    {#snippet trigger()}
                      <span class="truncate">{client.modelState.activeModel}</span>
                      <ChevronDown size={12} />
                    {/snippet}
                  </DropdownMenu>
                {/if}
                {#if client.authorized}
                  <button
                    type="button"
                    class={buttonClass('ghost', 'icon-sm', 'composer-icon-button rounded-full')}
                    disabled={reloadingModels}
                    aria-label={t('reloadModels')}
                    title={t('reloadModels')}
                    onclick={reloadModels}
                    ><RefreshCw
                      class={reloadingModels ? 'size-4 animate-spin' : 'size-4'}
                    /></button
                  >
                {/if}
              {/snippet}
            </ChatComposer>{/key}
          <div class="composer-context">
            <button onclick={addProject}
              ><Folder size={13} />{client.workspace?.split(/[\\/]/).at(-1) ||
                t('noFolder')}<ChevronDown size={12} /></button
            >
            <span class="composer-local">{t('local')} · Anda</span>
          </div>
        </div>
      </footer>
    {:else if client.view === 'memory'}<div class="management-page"><MemoryWorkspace /></div>
    {:else if client.view === 'skills'}<div class="management-page"><SkillsWorkspace /></div>
    {:else if client.view === 'bookmarks'}<div class="management-page"><BookmarksWorkspace /></div>
    {:else if client.view === 'automations'}<div class="management-page">
        <Automations {client} />
      </div>
    {:else if client.view === 'settings'}
      <div class="settings-tabs">
        <button class:active={settingsTab === 'general'} onclick={() => (settingsTab = 'general')}
          >{t('general')}</button
        ><button class:active={settingsTab === 'config'} onclick={() => (settingsTab = 'config')}
          >{t('config')}</button
        >
        <button class:active={settingsTab === 'audio'} onclick={() => (settingsTab = 'audio')}
          >{t('audio')}</button
        >
      </div>
      {#if settingsTab === 'audio'}<AudioPanel {client} />{:else if settingsTab === 'config'}<div
          class="management-page"
        >
          <ConfigApp embedded />
        </div>{:else}<div class="settings-page">
          <h1>{t('general')}</h1>
          <p class="muted">Anda Desktop · {client.connection.home}</p>
          <div class="setting-row">
            <span>{t('theme')}</span><DropdownMenu
              items={themeItems}
              value={client.preferences.theme}
              onSelect={(theme) => void preference({ theme })}
              ariaLabel={t('theme')}
              align="end"
            />
          </div>
          <div class="setting-row">
            <span>{t('language')}</span><LocaleSwitcher {client} />
          </div>
          <div class="setting-row">
            <span>{t('notifications')}</span><input
              type="checkbox"
              checked={client.preferences.notifications}
              onchange={(event) => preference({ notifications: event.currentTarget.checked })}
            />
          </div>
          <div class="setting-row">
            <span>{t('login')}</span><input
              type="checkbox"
              checked={client.preferences.launchAtLogin}
              onchange={(event) => preference({ launchAtLogin: event.currentTarget.checked })}
            />
          </div>
          <h2>{t('runtime')}</h2>
          <p class="runtime-path">
            {client.connection.binary || t('disconnected')}{client.connection.version
              ? ` · v${client.connection.version}`
              : ''}
          </p>
          <div class="settings-buttons">
            <button
              onclick={async () => {
                try {
                  client.connection = await window.anda.chooseBinary()
                  if (client.authorized) await client.refresh()
                } catch (error) {
                  client.fail(error)
                }
              }}>{t('chooseBinary')}</button
            ><button onclick={() => void window.anda.showLogs()}>{t('logs')}</button>
            <button
              onclick={async () => {
                try {
                  await window.anda.copyExtensionToken()
                } catch (error) {
                  client.fail(error)
                }
              }}>{t('extensionToken')}</button
            >
            <button
              onclick={async () => {
                try {
                  client.connection = await window.anda.control('restart')
                  if (client.authorized) await client.refresh()
                } catch (error) {
                  client.fail(error)
                }
              }}>{t('restartDaemon')}</button
            >
            <button
              onclick={async () => {
                try {
                  client.connection = await window.anda.control('stop')
                } catch (error) {
                  client.fail(error)
                }
              }}>{t('stopDaemon')}</button
            ><button onclick={() => void checkUpdate()}>{t('update')}</button>
          </div>
        </div>{/if}
    {/if}
  </section>
  {#if rightOpen && client.view === 'chat'}<aside class="resource-panel">
      <header>
        <span>{rightTab === 'resources' ? t('resources') : t(rightTab as Label)}</span><button
          class="icon-button"
          aria-label={t('close')}
          onclick={() => (rightOpen = false)}><X size={16} /></button
        >
      </header>
      <nav class="workbench-tabs" aria-label="Workbench">
        {#each ['resources', 'changes', 'terminal', 'browser'] as tab}<button
            class:active={rightTab === tab}
            onclick={() => (rightTab = tab)}
            >{tab === 'resources' ? t('resources') : t(tab as Label)}</button
          >{/each}
      </nav>
      {#if rightTab === 'browser'}
        {#key client.activeSource}<BrowserPanel
            source={client.activeSource}
            language={client.preferences.language}
          />{/key}
      {:else if rightTab === 'terminal' || rightTab === 'changes'}
        {#if client.workspace}{#key client.workspace}
            {#if rightTab === 'terminal'}<TerminalPanel
                workspace={client.workspace}
                language={client.preferences.language}
              />{:else}<GitPanel {client} workspace={client.workspace} />{/if}
          {/key}{:else}<p class="workbench-empty">
            {t('chooseProject')}
          </p>{/if}
      {:else}
        {#if resources.length}<div class="resource-list">
            {#each resources as resource}<button onclick={() => void showResource(resource)}
                ><Paperclip size={14} /><span>{resource.name}</span></button
              >{/each}
          </div>{:else}<div class="resource-empty">
            <Paperclip size={25} />
            <p>{t('noResources')}</p>
          </div>{/if}{#if selectedResource}<div class="resource-preview">
            <h3>{selectedResource.name}</h3>
            {#if previewError}<p>
                {previewError}
              </p>{:else if previewUrl && selectedResource.mime_type?.startsWith('image/')}<img
                src={previewUrl}
                alt={selectedResource.name}
              />{:else if previewUrl && selectedResource.mime_type === 'application/pdf'}<iframe
                title={selectedResource.name}
                src={previewUrl}
              ></iframe>{:else if previewUrl}<audio controls src={previewUrl}
              ></audio>{:else}<pre>{previewText}</pre>{/if}
          </div>{/if}
      {/if}
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
        /><button class="icon-button" onclick={() => (searchOpen = false)}><X size={17} /></button>
      </div>
      <div class="search-results">
        {#each searchChats as chat}<button
            onclick={() => {
              void client.switchChannel(chat.source)
              searchOpen = false
            }}><Circle size={12} /><span>{chat.title}</span><ArrowUpRight size={14} /></button
          >{/each}{#each searchResults as result}<button onclick={() => void openHistory(result)}
            ><Search size={13} /><span>{result.label || `Conversation ${result._id}`}</span
            ><ArrowUpRight size={14} /></button
          >{/each}{#if !searchChats.length && !searchResults.length}<p>
            {searchBusy ? '…' : t('emptySearch')}
          </p>{/if}
      </div>
    </div>
  </div>{/if}
{#if renameSource}<div class="modal-backdrop">
    <div
      use:focusDialog={() => (renameSource = '')}
      class="rename-dialog"
      role="dialog"
      aria-modal="true"
      aria-label={t('rename')}
      tabindex="-1"
    >
      <h2>{t('rename')}</h2>
      <input
        bind:value={renameTitle}
        onkeydown={(event) => {
          if (event.key === 'Enter') void rename()
        }}
      />
      <div>
        <button onclick={() => (renameSource = '')}>{t('cancel')}</button><button
          class="primary"
          onclick={() => void rename()}>{t('save')}</button
        >
      </div>
    </div>
  </div>{/if}
{#if client.updateDialogOpen}
  <UpdateDialog
    status={client.updateStatus}
    language={client.preferences.language}
    onclose={() => (client.updateDialogOpen = false)}
    oncheck={() => void checkUpdate()}
  />
{/if}
