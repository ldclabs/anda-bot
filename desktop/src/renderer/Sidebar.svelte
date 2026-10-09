<script lang="ts" module>
  export interface RuntimeActions {
    reconnect(): Promise<void>
    restart(): Promise<void>
    stop(): void
    checkUpdate(): Promise<void>
    /** Downloads the offered release, or restarts into a downloaded one. */
    continueUpdate(): Promise<void>
    showLogs(): void
    copyToken(): Promise<void>
  }
</script>

<script lang="ts">
  /**
   * The chat sidebar: history buttons beside the window controls, a title
   * filter, navigation, pinned and recent chats (optionally grouped), and the
   * runtime footer, topped by the offered update's download or restart. Each
   * chat row leads with its state: working, waiting for approval, failed or
   * unread.
   */
  import DropdownMenu from '$lib/anda/DropdownMenu.svelte'
  import {
    AlertCircle,
    ArrowLeft,
    ArrowRight,
    BookOpen,
    Bookmark,
    BrainCircuit,
    Clock3,
    Download,
    ListFilter,
    LoaderCircle,
    MoreHorizontal,
    PanelLeftClose,
    Pin,
    RotateCw,
    Search,
    Settings,
    ShieldAlert,
    SquarePen
  } from '@lucide/svelte'
  import type { DesktopClient } from './client.svelte'
  import type { ChatEntry, SidebarGroup } from '../shared/contract'
  import { shortcutLabel } from '../shared/shortcuts'
  import { folderName, type ChatSection, type ChatState } from './chat-list'
  import type { Label } from './labels'
  import { tip } from './tooltip'

  let {
    client,
    t,
    pinned,
    sections,
    chatState,
    filter = $bindable(''),
    showArchived = $bindable(false),
    renameSource,
    renameTitle = $bindable(''),
    canBack,
    canForward,
    onBack,
    onForward,
    onCollapse,
    onOpenSearch,
    onStartRename,
    onCommitRename,
    onCancelRename,
    runtime
  }: {
    client: DesktopClient
    t: (key: Label) => string
    pinned: ChatEntry[]
    sections: ChatSection[]
    chatState: (chat: ChatEntry) => ChatState
    filter?: string
    showArchived?: boolean
    renameSource: string
    renameTitle?: string
    canBack: boolean
    canForward: boolean
    onBack: () => void
    onForward: () => void
    onCollapse: () => void
    onOpenSearch: (query: string) => void
    onStartRename: (chat: ChatEntry) => void
    onCommitRename: () => void
    onCancelRename: () => void
    runtime: RuntimeActions
  } = $props()

  const keys = (action: Parameters<typeof shortcutLabel>[0]) =>
    shortcutLabel(action, client.platform)
  const navigation = [
    { id: 'memory', text: 'memory' as Label, icon: BrainCircuit },
    { id: 'skills', text: 'skills' as Label, icon: BookOpen },
    { id: 'automations', text: 'automations' as Label, icon: Clock3 },
    { id: 'bookmarks', text: 'bookmarks' as Label, icon: Bookmark }
  ]
  const stateLabels: Record<ChatState, Label | null> = {
    approval: 'stateApproval',
    running: 'stateRunning',
    failed: 'stateFailed',
    unread: 'stateUnread',
    idle: null
  }
  const dateLabels: Record<string, Label> = {
    today: 'today',
    yesterday: 'yesterday',
    week: 'previousWeek',
    month: 'previousMonth',
    older: 'older'
  }
  type FilterAction = SidebarGroup | 'archived'
  const filterItems = $derived<
    { value: FilterAction; label: string; checked?: boolean; separator?: boolean }[]
  >([
    { value: 'none', label: t('groupNone'), checked: client.preferences.sidebarGroup === 'none' },
    { value: 'date', label: t('groupDate'), checked: client.preferences.sidebarGroup === 'date' },
    {
      value: 'project',
      label: t('groupProject'),
      checked: client.preferences.sidebarGroup === 'project'
    },
    { value: 'archived', label: t('showArchived'), checked: showArchived, separator: true }
  ])
  function filterAction(action: FilterAction) {
    if (action === 'archived') showArchived = !showArchived
    else void client.savePreferences({ sidebarGroup: action }).catch((error) => client.fail(error))
  }
  type RowAction = 'rename' | 'pin' | 'archive'
  function rowMenu(chat: ChatEntry) {
    return [
      { value: 'rename' as const, label: t('rename') },
      { value: 'pin' as const, label: chat.pinned ? t('unpin') : t('pin') },
      { value: 'archive' as const, label: chat.archived ? t('restore') : t('archive') }
    ]
  }
  function rowAction(chat: ChatEntry, action: RowAction) {
    if (action === 'rename') onStartRename(chat)
    else if (action === 'pin') void client.updateChat(chat.source, { pinned: !chat.pinned })
    else void client.updateChat(chat.source, { archived: !chat.archived })
  }
  function sectionTitle(section: ChatSection): string | null {
    if (section.bucket) return t(dateLabels[section.bucket]!)
    if (section.workspace !== null)
      return section.workspace ? folderName(section.workspace) : t('noProject')
    return null
  }
  type RuntimeAction = 'reconnect' | 'restart' | 'stop' | 'update' | 'logs' | 'token'
  const runtimeItems = $derived<
    { value: RuntimeAction; label: string; separator?: boolean; tone?: 'danger' }[]
  >([
    ...(client.authorized
      ? [{ value: 'restart' as const, label: t('restartDaemon') }]
      : [{ value: 'reconnect' as const, label: t('reconnect') }]),
    { value: 'update', label: t('update') },
    { value: 'logs', label: t('logs'), separator: true },
    { value: 'token', label: t('extensionToken') },
    ...(client.authorized
      ? [
          {
            value: 'stop' as const,
            label: t('stopDaemon'),
            separator: true,
            tone: 'danger' as const
          }
        ]
      : [])
  ])
  function runtimeAction(action: RuntimeAction) {
    if (action === 'reconnect') void runtime.reconnect()
    else if (action === 'restart') void runtime.restart()
    else if (action === 'stop') runtime.stop()
    else if (action === 'update') void runtime.checkUpdate()
    else if (action === 'logs') runtime.showLogs()
    else void runtime.copyToken()
  }
  function focusRename(node: HTMLInputElement) {
    queueMicrotask(() => {
      node.focus()
      node.select()
    })
  }
</script>

{#snippet row(chat: ChatEntry)}
  {@const rowState = chatState(chat)}
  {@const stateLabel = stateLabels[rowState]}
  <div
    class:active={client.view === 'chat' && client.activeSource === chat.source}
    class:unread={rowState === 'unread' || rowState === 'failed'}
    class="chat-row"
  >
    {#if renameSource === chat.source}
      <input
        class="chat-rename"
        aria-label={t('rename')}
        bind:value={renameTitle}
        use:focusRename
        onkeydown={(event) => {
          if (event.key === 'Enter' && !event.isComposing) onCommitRename()
          if (event.key === 'Escape') {
            event.stopPropagation()
            onCancelRename()
          }
        }}
        onblur={onCommitRename}
      />
    {:else}
      <button class="chat-title" onclick={() => void client.switchChannel(chat.source)}>
        <span class="chat-state" data-state={rowState}>
          {#if rowState === 'running'}<LoaderCircle
              size={13}
            />{:else if rowState === 'approval'}<ShieldAlert
              size={13}
            />{:else if rowState === 'failed'}<AlertCircle
              size={13}
            />{:else if rowState === 'unread'}<span class="unread-dot"
            ></span>{:else if chat.pinned}<Pin size={12} />{/if}
          {#if stateLabel}<span class="sr-only">{t(stateLabel)}</span>{/if}
        </span>
        <span class="chat-name">{chat.title}</span>
      </button>
    {/if}
    <!-- Stays mounted while renaming: Rename is picked from this menu as it closes. -->
    <DropdownMenu
      class="chat-more icon-button"
      items={rowMenu(chat)}
      onSelect={(action) => rowAction(chat, action)}
      ariaLabel={t('details')}
      title=""
      align="end"
    >
      {#snippet trigger()}<MoreHorizontal size={16} />{/snippet}
    </DropdownMenu>
  </div>
{/snippet}

<div class="sidebar-titlebar">
  <button
    class="icon-button"
    aria-label={t('toggleSidebar')}
    use:tip={{ text: t('toggleSidebar'), shortcut: keys('toggle-sidebar') }}
    onclick={onCollapse}><PanelLeftClose size={17} /></button
  ><button
    class="icon-button"
    aria-label={t('back')}
    disabled={!canBack}
    use:tip={{ text: t('back'), shortcut: keys('back') }}
    onclick={onBack}><ArrowLeft size={16} /></button
  ><button
    class="icon-button"
    aria-label={t('forward')}
    disabled={!canForward}
    use:tip={{ text: t('forward'), shortcut: keys('forward') }}
    onclick={onForward}><ArrowRight size={16} /></button
  >
</div>
<div class="sidebar-search">
  <Search size={14} />
  <input
    type="search"
    placeholder={t('search')}
    aria-label={t('search')}
    bind:value={filter}
    onkeydown={(event) => {
      if (event.key === 'Enter' && !event.isComposing && filter.trim()) onOpenSearch(filter)
      if (event.key === 'Escape') filter = ''
    }}
  />
  <kbd>{keys('search')}</kbd>
</div>
<div class="sidebar-primary">
  <button class="nav-row" onclick={() => client.newChat()}
    ><SquarePen size={16} /><span>{t('newChat')}</span><kbd>{keys('new-chat')}</kbd></button
  >
</div>
<nav class="sidebar-navigation">
  {#each navigation as item (item.id)}<button
      class:active={client.view === item.id}
      class="nav-row"
      onclick={() => (client.view = item.id)}
      ><item.icon size={16} /><span>{t(item.text)}</span></button
    >{/each}
</nav>
<div class="sidebar-scroll">
  {#if pinned.length}
    <section aria-label={t('pinned')}>
      <div class="section-label"><span>{t('pinned')}</span></div>
      {#each pinned as chat (chat.source)}{@render row(chat)}{/each}
    </section>
  {/if}
  <section aria-label={showArchived ? t('archived') : t('recent')}>
    <div class="section-label">
      <span>{filter.trim() ? t('search') : showArchived ? t('archived') : t('recent')}</span>
      <DropdownMenu
        class="icon-button section-filter"
        items={filterItems}
        onSelect={filterAction}
        ariaLabel={t('filterChats')}
        title=""
        align="end"
      >
        {#snippet trigger()}<ListFilter size={14} />{/snippet}
      </DropdownMenu>
    </div>
    {#each sections as section (section.key)}
      {@const title = sectionTitle(section)}
      {#if title}<h3 class="section-group" title={section.workspace || undefined}>{title}</h3>{/if}
      {#each section.chats as chat (chat.source)}{@render row(chat)}{/each}
    {/each}
    {#if !sections.some((section) => section.chats.length) && !pinned.length}<p
        class="sidebar-empty"
      >
        {filter.trim() ? t('emptySearch') : t('noChats')}
      </p>{/if}
  </section>
</div>
<div class="sidebar-bottom">
  {#if client.updateOffer}
    {@const offer = client.updateOffer}
    {@const progress = client.updateStatus?.phase === 'running' ? client.updateStatus.message : ''}
    <button
      class="update-row"
      class:busy={progress}
      title={progress || undefined}
      onclick={() => void runtime.continueUpdate()}
    >
      {#if progress}<LoaderCircle size={15} />{:else if offer.ready}<RotateCw
          size={15}
        />{:else}<Download size={15} />{/if}
      <span class="runtime-text">
        <span>{progress || t(offer.ready ? 'restartToUpdate' : 'downloadUpdate')}</span>
        <small>Anda {offer.version.replace(/^v/, '')}</small>
      </span>
    </button>
  {/if}
  <DropdownMenu
    class="runtime-row"
    items={runtimeItems}
    onSelect={runtimeAction}
    ariaLabel={t('runtime')}
    title={client.connection.error || client.connection.home}
    align="start"
  >
    {#snippet trigger()}
      <span class:online={client.authorized} class="connection-dot"></span>
      <span class="runtime-text">
        <span>Anda{client.connection.version ? ` ${client.connection.version}` : ''}</span>
        <small>{client.authorized ? t('connected') : t('disconnected')}</small>
      </span>
    {/snippet}
  </DropdownMenu>
  <button
    class:pressed={client.view === 'settings'}
    class="icon-button"
    aria-label={t('settings')}
    use:tip={{ text: t('settings'), shortcut: keys('settings') }}
    onclick={() => (client.view = 'settings')}><Settings size={17} /></button
  >
</div>
