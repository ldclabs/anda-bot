<script lang="ts">
  import { getMessage } from '$lib/i18n'
  import {
    alertDialogContentClass,
    alertDialogDescriptionClass,
    alertDialogOverlayClass,
    badgeClass,
    buttonClass,
    itemClass,
    itemContentClass,
    itemMediaClass,
    itemTitleClass
  } from '$lib/anda/ui'
  import {
    ChevronDown,
    CircleAlert,
    FolderOpen,
    History,
    LoaderCircle,
    Radio,
    Trash2
  } from '@lucide/svelte'
  import { AlertDialog } from 'bits-ui'
  import { cubicOut } from 'svelte/easing'
  import { fly } from 'svelte/transition'
  import type { Channel } from './client/channel.svelte'

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
  let collapsedOverride = $state<boolean | null>(null)
  let flyoutOpen = $state(false)
  let deleteDialogOpen = $state(false)
  let pendingDeleteSource = $state<string | null>(null)

  const autoCollapsed = $derived(viewportWidth > 0 && viewportWidth < 760)
  const collapsed = $derived(collapsedOverride ?? autoCollapsed)
  const pendingDeleteTitle = $derived(
    pendingDeleteSource
      ? channelTitle(pendingDeleteSource)
      : getMessage('deleteChannel')
  )
  const pendingDeleteDescription = $derived(
    getMessage('deleteChannelConfirm', pendingDeleteTitle)
  )

  function toggleCollapsed() {
    collapsedOverride = !collapsed
    flyoutOpen = false
  }

  function openFlyout() {
    if (collapsed) {
      flyoutOpen = true
    }
  }

  function closeFlyout() {
    flyoutOpen = false
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
    if (collapsed) {
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

  function statusLabel(channel: Channel): string {
    if (channel.sending) {
      return 'sending'
    }
    return channel.status
  }

  function statusDotClass(channel: Channel): string {
    const status = statusLabel(channel)
    if (status === 'failed' || status.includes('failed')) {
      return 'bg-amber-500 shadow-[0_0_0_3px_rgba(245,158,11,0.14)]'
    }
    if (['sending', 'submitted', 'working', 'syncing'].includes(status)) {
      return 'animate-pulse bg-emerald-600 shadow-[0_0_0_3px_rgba(5,150,105,0.14)]'
    }
    return 'bg-stone-300 dark:bg-stone-600'
  }

  function statusIcon(channel: Channel): 'loader' | 'warning' | 'radio' {
    const status = statusLabel(channel)
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

<svelte:window bind:innerWidth={viewportWidth} />

{#snippet channelsHeader(expanded: boolean, floating: boolean)}
  <div
    class={`flex h-12 shrink-0 items-center gap-2 border-b border-sidebar-border px-1.5 ${
      expanded ? '' : 'justify-center'
    } ${floating ? 'rounded-tr-md bg-sidebar/95' : ''}`}
  >
    <button
      type="button"
      class={buttonClass(
        'ghost',
        'icon-sm',
        'grid place-items-center bg-sidebar-accent text-sidebar-accent-foreground hover:bg-muted'
      )}
      aria-label={getMessage(collapsed ? 'expandChannels' : 'collapseChannels')}
      aria-expanded={!collapsed || flyoutOpen}
      title={getMessage(collapsed ? 'expandChannels' : 'collapseChannels')}
      onclick={toggleCollapsed}
    >
      <History class="size-4" />
    </button>

    {#if expanded}
      <div class="min-w-0 flex-1">
        <div class="truncate text-xs font-bold text-sidebar-foreground">
          {getMessage('channelsLabel')}
          <span class={badgeClass('outline')}>
            {channels.length}
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
      <button
        type="button"
        class={buttonClass(
          'ghost',
          'icon-sm',
          'grid place-items-center bg-sidebar-accent text-sidebar-accent-foreground hover:bg-muted'
        )}
        aria-label={getMessage(collapsed ? 'expandChannels' : 'collapseChannels')}
        aria-expanded={!collapsed || flyoutOpen}
        title={getMessage(collapsed ? 'expandChannels' : 'collapseChannels')}
        onclick={toggleCollapsed}
      >
        <ChevronDown class="size-4 shrink-0 rotate-90 text-muted-foreground" />
      </button>
    {/if}
  </div>
{/snippet}

{#snippet channelsList(expanded: boolean)}
  <div
    class={`flex min-h-0 flex-1 flex-col gap-1 overflow-y-auto p-1.5 ${
      expanded ? 'scrollbar-slim' : 'scrollbar-none'
    }`}
  >
    {#each channels as channel (channel.source)}
      {@const active = channel.source === activeSource}
      {@const icon = statusIcon(channel)}
      <div
        data-slot="item"
        data-variant={active ? 'outline' : 'default'}
        data-size="xs"
        class={itemClass(
          active ? 'outline' : 'default',
          'xs',
          `group relative flex-nowrap p-0 text-left ${
            active
              ? 'border-sidebar-border bg-background text-foreground shadow-sm'
              : 'text-muted-foreground hover:border-sidebar-border hover:bg-sidebar-accent hover:text-sidebar-accent-foreground'
          } ${expanded ? '' : 'mx-auto size-9 shrink-0 justify-center'}`
        )}
      >
        <button
          type="button"
          class={`flex items-center text-left ${
            expanded ? 'min-w-0 flex-1 gap-2 px-2 py-2' : 'size-full justify-center'
          }`}
          aria-current={active ? 'page' : undefined}
          aria-label={`${channelTitle(channel.source)} ${statusLabel(channel)}`}
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
                <span class="shrink-0">{statusLabel(channel)}</span>
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
              'pointer-events-none absolute bottom-1 right-1 z-10 text-muted-foreground opacity-0 shadow-sm group-hover:pointer-events-auto group-hover:opacity-100 hover:bg-amber-50 hover:text-amber-700 focus-visible:pointer-events-auto focus-visible:opacity-100 dark:hover:bg-amber-950/40 dark:hover:text-amber-300'
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
  </div>
{/snippet}

<aside
  class={`group/sidebar relative z-20 h-full shrink-0 border-r border-sidebar-border bg-sidebar text-sidebar-foreground backdrop-blur transition-[width] duration-200 ${
    collapsed ? 'w-12 overflow-visible' : 'w-64 overflow-hidden'
  }`}
  aria-label={getMessage('channelsLabel')}
  onpointerenter={openFlyout}
  onpointerleave={closeFlyout}
  onfocusin={openFlyout}
  onfocusout={handleSidebarFocusOut}
>
  <div class="flex h-full min-h-0 flex-col">
    {@render channelsHeader(!collapsed, false)}
    {@render channelsList(!collapsed)}
  </div>

  {#if collapsed && flyoutOpen}
    <div
      class="absolute inset-y-0 left-0 z-50 w-64"
      aria-label={getMessage('channelsLabel')}
    >
      <div
        class="flex h-full min-h-0 flex-col overflow-hidden rounded-r-md border-r border-sidebar-border bg-sidebar text-sidebar-foreground shadow-[0_18px_40px_rgba(15,23,42,0.18)] ring-1 ring-sidebar-border/70 backdrop-blur-xl will-change-transform dark:shadow-[0_18px_40px_rgba(0,0,0,0.35)]"
        transition:fly={{ x: -18, duration: 160, easing: cubicOut, opacity: 0.92 }}
      >
        {@render channelsHeader(true, true)}
        {@render channelsList(true)}
      </div>
    </div>
  {/if}
</aside>

<AlertDialog.Root bind:open={deleteDialogOpen}>
  <AlertDialog.Portal>
    <AlertDialog.Overlay class={alertDialogOverlayClass()} />
    <AlertDialog.Content class={alertDialogContentClass()}>
      <div
        class="grid grid-rows-[auto_1fr] place-items-center gap-1.5 text-center has-data-[slot=alert-dialog-media]:grid-rows-[auto_auto_1fr] has-data-[slot=alert-dialog-media]:gap-x-6"
      >
        <AlertDialog.Title class="text-lg font-medium">
          {getMessage('deleteChannel')}
        </AlertDialog.Title>
        <AlertDialog.Description class={alertDialogDescriptionClass()}>
          {pendingDeleteDescription}
        </AlertDialog.Description>
      </div>
      <div class="cn-alert-dialog-footer grid grid-cols-2 gap-2">
        <AlertDialog.Cancel class={buttonClass('outline')}>
          {getMessage('cancel')}
        </AlertDialog.Cancel>
        <AlertDialog.Action class={buttonClass('destructive')} onclick={confirmDeleteChannel}>
          {getMessage('deleteChannel')}
        </AlertDialog.Action>
      </div>
    </AlertDialog.Content>
  </AlertDialog.Portal>
</AlertDialog.Root>
