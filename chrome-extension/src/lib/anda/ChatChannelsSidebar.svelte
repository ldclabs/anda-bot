<script lang="ts">
  import { getMessage } from '$lib/i18n'
  import {
    badgeClass,
    buttonClass,
    inputClass,
    itemClass,
    itemContentClass,
    itemMediaClass,
    itemTitleClass
  } from '$lib/anda/ui'
  import {
    PanelLeftClose,
    CircleAlert,
    FolderOpen,
    History,
    LoaderCircle,
    Radio,
    Search,
    Trash2,
    X
  } from '@lucide/svelte'
  import { statusLabel } from './chat/status'
  import type { Channel } from './client/channel.svelte'
  import Modal from './Modal.svelte'

  type Props = {
    channels: Channel[]
    activeSource: string | null
    sending?: boolean
    onSelect?: (source: string) => void | Promise<void>
    onOpenFolder?: () => void | Promise<void>
    onDelete?: (source: string) => void | Promise<void>
  }

  let {
    channels = [],
    activeSource = null,
    sending = false,
    onSelect,
    onOpenFolder,
    onDelete
  }: Props = $props()
  let viewportWidth = $state(0)
  let folded = $state(false)
  let toggleButton: HTMLButtonElement
  let flyoutOpen = $state(false)
  let deleteDialogOpen = $state(false)
  let pendingDeleteSource = $state<string | null>(null)
  let searchQuery = $state('')
  let sidebarElement = $state<HTMLElement | null>(null)

  const autoCollapsed = $derived(viewportWidth > 0 && viewportWidth < 760)
  const collapsed = $derived(autoCollapsed || folded)
  const expanded = $derived(!collapsed || flyoutOpen)
  const toggleLabel = $derived(
    getMessage((autoCollapsed ? flyoutOpen : !collapsed) ? 'collapseChannels' : 'expandChannels')
  )
  const searchTerm = $derived(searchQuery.trim().toLowerCase())
  const filteredChannels = $derived.by(() => {
    if (!searchTerm) {
      return channels
    }
    return channels.filter((channel) => channelSearchText(channel).includes(searchTerm))
  })
  const channelCountLabel = $derived(
    searchTerm ? `${filteredChannels.length}/${channels.length}` : String(channels.length)
  )
  const pendingDeleteTitle = $derived(
    pendingDeleteSource ? channelTitle(pendingDeleteSource) : getMessage('deleteChannel')
  )
  const pendingDeleteDescription = $derived(getMessage('deleteChannelConfirm', pendingDeleteTitle))

  function toggleCollapsed() {
    if (autoCollapsed) flyoutOpen = !flyoutOpen
    else {
      folded = !folded
      flyoutOpen = false
    }
  }

  function openFlyout() {
    if (collapsed && !autoCollapsed) {
      flyoutOpen = true
    }
  }

  function closeFlyout() {
    flyoutOpen = false
  }

  function closeFlyoutAfterPointerLeave() {
    if (autoCollapsed || focusedInsideSidebar()) {
      return
    }
    closeFlyout()
  }

  function handleSidebarFocusOut(event: FocusEvent) {
    const current = event.currentTarget
    const next = event.relatedTarget
    if (current instanceof HTMLElement && next instanceof Node && current.contains(next)) {
      return
    }
    closeFlyout()
  }

  async function selectChannel(source: string) {
    if (source !== activeSource) {
      await onSelect?.(source)
    }
    if (collapsed && flyoutOpen) {
      toggleButton.focus()
      closeFlyout()
    }
  }

  async function openFolder() {
    await onOpenFolder?.()
  }

  function requestDeleteChannel(source: string) {
    pendingDeleteSource = source
    deleteDialogOpen = true
  }

  async function confirmDeleteChannel() {
    const source = pendingDeleteSource
    pendingDeleteSource = null
    deleteDialogOpen = false
    if (!source) {
      return
    }
    await onDelete?.(source)
  }

  function clearSearch() {
    searchQuery = ''
  }

  function handleSearchKeydown(event: KeyboardEvent) {
    if (event.key !== 'Escape') {
      return
    }
    event.preventDefault()
    event.stopPropagation()
    if (searchQuery) {
      clearSearch()
      return
    }
    if (collapsed) {
      toggleButton.focus()
      closeFlyout()
    }
  }

  function focusedInsideSidebar(): boolean {
    if (typeof document === 'undefined') {
      return false
    }
    return Boolean(
      sidebarElement &&
      document.activeElement instanceof Node &&
      sidebarElement.contains(document.activeElement)
    )
  }

  function channelTitle(source: string): string {
    if (source.startsWith('browser:')) {
      const [, scope] = source.split(':')
      return titleCase(scope || 'browser')
    }

    if (source.startsWith('cli:')) {
      return `CLI ${lastPathPart(source.slice(4)) || source.slice(4)}`.trim()
    }
    const [kind] = source.split(':')
    return titleCase(kind || source)
  }

  function channelSubtitle(source: string): string {
    if (source.startsWith('browser:')) {
      return source.split(':').slice(1).join(':')
    }
    if (source.startsWith('cli:')) {
      return source.slice(4)
    }
    return source
  }

  function channelMeta(channel: Channel): string {
    const parts = []
    if (channel.conversationId) {
      parts.push(`#${channel.conversationId}`)
    }
    if (channel.messageCount > 0) {
      parts.push(String(channel.messageCount))
    }
    return parts.join(' / ')
  }

  function channelSearchText(channel: Channel): string {
    return [
      channel.source,
      channelTitle(channel.source),
      channelSubtitle(channel.source),
      channelStatus(channel),
      statusLabel(channelStatus(channel)),
      channel.conversationId ? `#${channel.conversationId}` : '',
      channel.conversationId || '',
      channel.messageCount > 0 ? String(channel.messageCount) : ''
    ]
      .join(' ')
      .toLowerCase()
  }

  /** The status code; `statusLabel` turns it into words. */
  function channelStatus(channel: Channel): string {
    if (channel.sending) {
      return 'sending'
    }
    return channel.status
  }

  function statusDotClass(channel: Channel): string {
    const status = channelStatus(channel)
    if (status === 'failed' || status.includes('failed')) {
      return 'bg-amber-500 shadow-[0_0_0_3px_rgba(245,158,11,0.14)]'
    }
    if (['sending', 'submitted', 'working', 'syncing'].includes(status)) {
      return 'animate-pulse bg-emerald-600 shadow-[0_0_0_3px_rgba(5,150,105,0.14)]'
    }
    return 'bg-stone-300 dark:bg-stone-600'
  }

  function statusIcon(channel: Channel): 'loader' | 'warning' | 'radio' {
    const status = channelStatus(channel)
    if (['sending', 'submitted', 'working', 'syncing'].includes(status)) {
      return 'loader'
    }
    if (status === 'failed' || status.includes('failed')) {
      return 'warning'
    }
    return 'radio'
  }

  function titleCase(value: string): string {
    if (!value) {
      return ''
    }
    return value.charAt(0).toUpperCase() + value.slice(1)
  }

  function lastPathPart(value: string): string {
    return value.split(/[\\/]/).filter(Boolean).pop() || ''
  }
</script>

<svelte:window
  bind:innerWidth={viewportWidth}
  onpointerdown={(event) => {
    if (flyoutOpen && event.target instanceof Node && !sidebarElement?.contains(event.target))
      closeFlyout()
  }}
  onkeydown={(event) => {
    if (event.key === 'Escape' && flyoutOpen && !deleteDialogOpen && focusedInsideSidebar()) {
      event.preventDefault()
      toggleButton.focus()
      closeFlyout()
    }
  }}
/>

{#snippet channelsHeader()}
  <div
    class={`flex h-12 shrink-0 items-center gap-2 border-b border-sidebar-border px-1.5 ${
      expanded ? '' : 'justify-center'
    }`}
  >
    <button
      type="button"
      class={buttonClass(
        'ghost',
        'icon-sm',
        'grid place-items-center bg-sidebar-accent text-sidebar-accent-foreground hover:bg-muted'
      )}
      bind:this={toggleButton}
      aria-label={toggleLabel}
      aria-expanded={expanded}
      title={toggleLabel}
      onclick={toggleCollapsed}
    >
      {#if expanded}<PanelLeftClose class="size-4 rtl:rotate-180" />{:else}<History
          class="size-4"
        />{/if}
    </button>

    {#if expanded}
      <div class="min-w-0 flex-1">
        <div class="truncate text-xs font-bold text-sidebar-foreground">
          {getMessage('channelsLabel')}
          <span class={badgeClass('outline')}>
            {channelCountLabel}
          </span>
        </div>
      </div>
      <button
        type="button"
        class={buttonClass(
          'ghost',
          'icon-sm',
          'grid place-items-center bg-sidebar-accent text-sidebar-accent-foreground hover:bg-muted'
        )}
        aria-label={getMessage('openFolder')}
        title={getMessage('openFolder')}
        disabled={sending}
        onclick={openFolder}
      >
        <FolderOpen class="size-4" />
      </button>
    {/if}
  </div>
{/snippet}

{#snippet channelsSearch()}
  <div class="shrink-0 border-b border-sidebar-border px-1.5 py-1.5">
    <label class="relative block min-w-0">
      <Search
        class="pointer-events-none absolute start-2.5 top-1/2 size-3.5 -translate-y-1/2 text-muted-foreground"
      />
      <input
        type="text"
        class={inputClass('h-8 bg-sidebar-accent/35 ps-8 pe-8 text-xs shadow-none')}
        bind:value={searchQuery}
        placeholder={getMessage('searchChannelsPlaceholder')}
        aria-label={getMessage('searchChannels')}
        onkeydown={handleSearchKeydown}
      />
      {#if searchQuery}
        <button
          type="button"
          class={buttonClass(
            'ghost',
            'icon-xs',
            'absolute end-1.5 top-1/2 -translate-y-1/2 text-muted-foreground hover:text-foreground'
          )}
          aria-label={getMessage('clearSearch')}
          title={getMessage('clearSearch')}
          onclick={clearSearch}
        >
          <X class="size-3" />
        </button>
      {/if}
    </label>
  </div>
{/snippet}

{#snippet channelsList(expanded: boolean)}
  <div
    class={`flex min-h-0 flex-1 flex-col gap-1 overflow-y-auto p-1.5 ${
      expanded ? 'scrollbar-slim' : 'scrollbar-none'
    }`}
  >
    {#if filteredChannels.length === 0}
      <div
        class="grid min-h-24 flex-1 place-items-center px-3 text-center text-xs text-muted-foreground"
      >
        <div class="grid max-w-40 gap-2">
          <Search class="mx-auto size-5 opacity-70" />
          <div class="font-medium">{getMessage('noChannelsFound')}</div>
          <button type="button" class={buttonClass('ghost', 'xs', 'mx-auto')} onclick={clearSearch}>
            {getMessage('clearSearch')}
          </button>
        </div>
      </div>
    {:else}
      {#each filteredChannels as channel (channel.source)}
        {@const active = channel.source === activeSource}
        {@const icon = statusIcon(channel)}
        <div
          data-slot="item"
          data-variant={active ? 'outline' : 'default'}
          data-size="xs"
          class={itemClass(
            active ? 'outline' : 'default',
            'xs',
            `group relative flex-nowrap p-0 text-start ${
              active
                ? 'border-sidebar-border bg-background text-foreground shadow-sm'
                : 'text-muted-foreground hover:border-sidebar-border hover:bg-sidebar-accent hover:text-sidebar-accent-foreground'
            } ${expanded ? '' : 'mx-auto size-9 shrink-0 justify-center'}`
          )}
        >
          <button
            type="button"
            class={`flex items-center text-start ${
              expanded ? 'min-w-0 flex-1 gap-2 px-2 py-2' : 'size-full justify-center'
            }`}
            aria-current={active ? 'page' : undefined}
            aria-label={`${channelTitle(channel.source)} ${statusLabel(channelStatus(channel))}`}
            title={`${channelTitle(channel.source)}\n${channel.source}`}
            onclick={() => selectChannel(channel.source)}
          >
            <span
              data-slot="item-media"
              data-variant="icon"
              class={itemMediaClass(
                'icon',
                `relative grid size-6 place-items-center rounded-md border ${
                  active
                    ? 'border-sidebar-border bg-sidebar-accent text-emerald-700 dark:text-emerald-300'
                    : 'border-sidebar-border bg-background/75 text-muted-foreground'
                }`
              )}
            >
              {#if icon === 'loader'}
                <LoaderCircle class="size-3.5 animate-spin" />
              {:else if icon === 'warning'}
                <CircleAlert class="size-3.5 text-amber-700" />
              {:else}
                <Radio class="size-3.5" />
              {/if}
              <span
                class={`absolute -right-0.5 -bottom-0.5 size-2 rounded-full ${statusDotClass(channel)}`}
              ></span>
            </span>

            {#if expanded}
              <div data-slot="item-content" class={itemContentClass('min-w-0 gap-0')}>
                <div class="flex min-w-0 items-center gap-2">
                  <div class="flex min-w-0 flex-1 items-center gap-2">
                    <div
                      data-slot="item-title"
                      class={itemTitleClass('min-w-0 flex-1 text-xs font-bold')}
                    >
                      <span class="truncate">{channelTitle(channel.source)}</span>
                    </div>
                    {#if active && sending}
                      <LoaderCircle class="size-3 shrink-0 animate-spin text-emerald-700" />
                    {/if}
                  </div>
                  {#if channelMeta(channel)}
                    <span class={badgeClass('secondary', 'h-4 rounded-full px-1.5 text-[10px]')}>
                      {channelMeta(channel)}
                    </span>
                  {/if}
                </div>
                <div
                  class="mt-0.5 flex min-w-0 items-center gap-1.5 text-[10px] text-muted-foreground"
                >
                  <span class="shrink-0">{statusLabel(channelStatus(channel))}</span>
                  <span class="min-w-0 truncate text-muted-foreground opacity-70"
                    >{channelSubtitle(channel.source)}</span
                  >
                </div>
              </div>
            {/if}
          </button>

          {#if expanded}
            <button
              type="button"
              class={buttonClass(
                'outline',
                'icon-xs',
                'pointer-events-none absolute end-1 bottom-1 z-10 text-muted-foreground opacity-0 shadow-sm group-hover:pointer-events-auto group-hover:opacity-100 hover:bg-amber-50 hover:text-amber-700 focus-visible:pointer-events-auto focus-visible:opacity-100 dark:hover:bg-amber-950/40 dark:hover:text-amber-300'
              )}
              aria-label={getMessage('deleteChannel')}
              title={getMessage('deleteChannel')}
              disabled={sending || channel.sending}
              onclick={() => requestDeleteChannel(channel.source)}
            >
              <Trash2 class="size-3.5" />
            </button>
          {/if}
        </div>
      {/each}
    {/if}
  </div>
{/snippet}

<aside
  bind:this={sidebarElement}
  class={`group/sidebar relative z-20 h-full shrink-0 border-e border-sidebar-border bg-sidebar text-sidebar-foreground backdrop-blur transition-[width] duration-200 ${
    collapsed ? 'w-12 overflow-visible' : 'w-64 overflow-hidden'
  }`}
  aria-label={getMessage('channelsLabel')}
  onpointerenter={openFlyout}
  onpointerleave={closeFlyoutAfterPointerLeave}
  onfocusin={openFlyout}
  onfocusout={handleSidebarFocusOut}
>
  <div
    class={`flex h-full min-h-0 flex-col ${collapsed && flyoutOpen ? 'absolute inset-y-0 start-0 z-50 w-64 max-w-[calc(100vw-1rem)] overflow-hidden rounded-e-md border-e border-sidebar-border bg-sidebar shadow-xl' : ''}`}
  >
    {@render channelsHeader()}
    {#if expanded}{@render channelsSearch()}{/if}
    {@render channelsList(expanded)}
  </div>
</aside>

{#snippet deleteActions()}
  <button class={buttonClass('outline', 'sm')} onclick={() => (deleteDialogOpen = false)}>
    {getMessage('cancel')}
  </button>
  <button class={buttonClass('destructive', 'sm')} onclick={confirmDeleteChannel}>
    {getMessage('deleteChannel')}
  </button>
{/snippet}

<Modal
  alert
  bind:open={deleteDialogOpen}
  title={getMessage('deleteChannel')}
  contentClass="min-h-0 sm:max-w-sm"
  footer={deleteActions}
>
  <p class="text-sm leading-relaxed text-muted-foreground">{pendingDeleteDescription}</p>
</Modal>
