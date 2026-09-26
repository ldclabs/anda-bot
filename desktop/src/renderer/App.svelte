<script lang="ts">
  import { focusDialog } from './dialog'
  import { onDestroy, tick, untrack } from 'svelte'
  import { provideAndaClient } from '$lib/anda/client/context'
  import { applyAppearanceTheme } from '$lib/anda/theme'
  import pandaLogo from '../../../anda_bot/assets/logo.png'
  import ChatComposer from '$lib/anda/ChatComposer.svelte'
  import ChatMessageItem from '$lib/anda/ChatMessageItem.svelte'
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
    Plus,
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
  import type { ChatAttachment, Conversation, RpcOutput, Resource } from '$lib/anda/client/types'
  import { label, type Label } from './labels'
  import Automations from './Automations.svelte'
  import TerminalPanel from './TerminalPanel.svelte'
  import GitPanel from './GitPanel.svelte'
  import BrowserPanel from './BrowserPanel.svelte'
  import AudioPanel from './AudioPanel.svelte'
  import { wb } from './workbench-labels'
  let { client }: { client: DesktopClient } = $props()
  provideAndaClient(untrack(() => client))
  const t = (key: Label) => label(client.preferences.language, key)
  let pageVisible = $state(!document.hidden)
  let collapsed = $state(false)
  let rightOpen = $state(false)
  let rightTab = $state('resources')
  let searchOpen = $state(false)
  let query = $state('')
  let searchBusy = $state(false)
  let searchResults = $state<Conversation[]>([])
  let showArchived = $state(false)
  let menuSource = $state('')
  let renameSource = $state('')
  let renameTitle = $state('')
  let settingsTab = $state('general')
  let memoryMode = $state<'standard' | 'no_store' | 'off'>('standard')
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
      memoryMode = 'standard'
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
      menuSource = ''
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
      await client.sendPrompt(
        payload.text,
        payload.attachments,
        !channel?.conversationId && memoryMode !== 'standard' ? memoryMode : undefined
      )
    } catch (error) {
      client.fail(error)
      throw error
    }
    memoryMode = 'standard'
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
      const bytes = Uint8Array.from(atob(resource.blob), (c) => c.charCodeAt(0))
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
  async function rename() {
    await client.updateChat(renameSource, { title: renameTitle.trim() || 'Untitled' })
    renameSource = ''
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
  class:with-panel={rightOpen && client.view === 'chat'}
  class:workbench-open={rightOpen && rightTab !== 'resources' && client.view === 'chat'}
  class="desktop-shell"
>
  <aside class="sidebar">
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
        <span>{t('projects')}</span><button
          class="icon-button"
          title={t('addProject')}
          onclick={addProject}><Plus size={14} /></button
        >
      </div>
      {#each client.preferences.projects as project}<button
          class="nav-row project-row"
          title={project.path}
          onclick={() => client.newChat(project.path)}
          ><Folder size={15} /><span>{project.name}</span><Plus size={13} /></button
        >{/each}
      {#if !client.preferences.projects.length}<button class="add-project" onclick={addProject}
          ><Plus size={13} />{t('addProject')}</button
        >{/if}
      <div class="section-label chat-section">
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
          <button
            class="chat-more icon-button"
            title={t('details')}
            onclick={() => (menuSource = menuSource === chat.source ? '' : chat.source)}
            ><MoreHorizontal size={16} /></button
          >
          {#if menuSource === chat.source}<div class="context-menu">
              <button
                onclick={() => {
                  renameSource = chat.source
                  renameTitle = chat.title
                  menuSource = ''
                }}>{t('rename')}</button
              ><button
                onclick={() => {
                  void client.updateChat(chat.source, { pinned: !chat.pinned })
                  menuSource = ''
                }}>{chat.pinned ? t('unpin') : t('pin')}</button
              ><button
                onclick={() => {
                  void client.updateChat(chat.source, { archived: !chat.archived })
                  menuSource = ''
                }}>{chat.archived ? t('restore') : t('archive')}</button
              >
            </div>{/if}
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
          <span
            >{client.preferences.language === 'zh_CN'
              ? '通道聊天仅供查看，避免改变原有发送者和回复路由。'
              : 'Channel history is read-only to preserve its original sender and reply route.'}</span
          ><button onclick={() => client.newChat()}>{t('newChat')}</button>
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
                    text: client.preferences.language.startsWith('zh')
                      ? '帮我整理一下这些想法：'
                      : 'Help me organize these thoughts: ',
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
                <span class="working-dot"></span>{client.preferences.language.startsWith('zh')
                  ? 'Anda 正在处理…'
                  : 'Anda is working…'}
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
          <div class="composer-context">
            <button onclick={addProject}
              ><Folder size={13} />{client.workspace?.split(/[\\/]/).at(-1) ||
                t('noFolder')}<ChevronDown size={12} /></button
            >{#if !channel?.conversationId}<select
                aria-label={t('memoryMode')}
                bind:value={memoryMode}
                ><option value="standard">{t('standard')}</option><option value="no_store"
                  >{t('noStore')}</option
                ><option value="off">{t('off')}</option></select
              >{/if}
          </div>
          {#key client.activeSource}<ChatComposer
              disabled={!client.authorized || client.readOnly || currentPending.length > 0}
              placeholder={t('prompt')}
              sending={submitting}
              {working}
              stoppable={(working || client.voice.speaking) && !client.readOnly}
              onSend={send}
              onStop={() => client.stopActiveTask()}
              voiceEnabled={memoryMode === 'standard' && pageVisible}
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
            />{/key}
          <div class="composer-meta">
            <select
              aria-label="Model"
              title={t('modelScope')}
              value={client.modelState.activeModel || ''}
              onchange={(event) =>
                void client
                  .setActiveModel(event.currentTarget.value)
                  .catch((error) => client.fail(error))}
              >{#each client.modelState.modelNames as name}<option value={name}>{name}</option
                >{/each}</select
            ><span>{t('local')} · Anda</span>
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
          >{wb(client.preferences.language, 'audio')}</button
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
            <span>{t('theme')}</span><select
              value={client.preferences.theme}
              onchange={(event) =>
                preference({ theme: event.currentTarget.value as 'system' | 'light' | 'dark' })}
              ><option value="system">{t('system')}</option><option value="light"
                >{t('light')}</option
              ><option value="dark">{t('dark')}</option></select
            >
          </div>
          <div class="setting-row">
            <span>{t('language')}</span><select
              value={client.preferences.language}
              onchange={async (event) => {
                await preference({ language: event.currentTarget.value })
                location.reload()
              }}
              ><option value="en">English</option><option value="zh_CN">简体中文</option><option
                value="fr">Français</option
              ><option value="es">Español</option><option value="ru">Русский</option><option
                value="ar">العربية</option
              ></select
            >
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
          <p class="runtime-path">{client.connection.binary || t('disconnected')}</p>
          <div class="settings-buttons">
            <button
              onclick={async () => {
                client.connection = await window.anda.chooseBinary()
                await client.refresh()
              }}>{t('chooseBinary')}</button
            ><button onclick={() => void window.anda.showLogs()}>{t('logs')}</button>
            <button
              onclick={async () => {
                try {
                  client.connection = await window.anda.control('restart')
                  if (client.authorized) await client.refresh()
                } catch (error) {
                  client.fail(error)
                }
              }}>{client.preferences.language === 'zh_CN' ? '重启服务' : 'Restart daemon'}</button
            >
            <button
              onclick={async () => {
                try {
                  client.connection = await window.anda.control('stop')
                } catch (error) {
                  client.fail(error)
                }
              }}>{client.preferences.language === 'zh_CN' ? '停止服务' : 'Stop daemon'}</button
            ><button
              onclick={async () => {
                client.systemMessage = { kind: 'info', text: await window.anda.checkUpdate() }
                client.view = 'chat'
              }}>{t('update')}</button
            >
          </div>
        </div>{/if}
    {/if}
  </section>
  {#if rightOpen && client.view === 'chat'}<aside class="resource-panel">
      <header>
        <span
          >{rightTab === 'resources'
            ? t('resources')
            : wb(client.preferences.language, rightTab as 'changes' | 'terminal' | 'browser')}</span
        ><button class="icon-button" aria-label={t('close')} onclick={() => (rightOpen = false)}
          ><X size={16} /></button
        >
      </header>
      <nav class="workbench-tabs" aria-label="Workbench">
        {#each ['resources', 'changes', 'terminal', 'browser'] as tab}<button
            class:active={rightTab === tab}
            onclick={() => (rightTab = tab)}
            >{tab === 'resources'
              ? t('resources')
              : wb(client.preferences.language, tab as 'changes' | 'terminal' | 'browser')}</button
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
            {wb(client.preferences.language, 'chooseProject')}
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
