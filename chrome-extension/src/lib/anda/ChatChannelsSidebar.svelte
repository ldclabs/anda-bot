<script lang="ts">
  import * as AlertDialog from '$lib/components/ui/alert-dialog/index.js'
  import { Badge } from '$lib/components/ui/badge/index.js'
  import { Button } from '$lib/components/ui/button/index.js'
  import { Item, ItemContent, ItemMedia, ItemTitle } from '$lib/components/ui/item/index.js'
  import { ChevronDown, CircleAlert, History, LoaderCircle, Radio, Trash2 } from '@lucide/svelte'
  import type { Channel } from './client/channel.svelte'

  type Props = {
    channels: Channel[]
    activeSource: string | null
    sending?: boolean
    onSelect?: (source: string) => void | Promise<void>
    onDelete?: (source: string) => void | Promise<void>
  }

  let { channels = [], activeSource = null, sending = false, onSelect, onDelete }: Props = $props()
  let viewportWidth = $state(0)
  let collapsedOverride = $state<boolean | null>(null)
  let deleteDialogOpen = $state(false)
  let pendingDeleteSource = $state<string | null>(null)

  const autoCollapsed = $derived(viewportWidth > 0 && viewportWidth < 760)
  const collapsed = $derived(collapsedOverride ?? autoCollapsed)
  const pendingDeleteTitle = $derived(
    pendingDeleteSource
      ? channelTitle(pendingDeleteSource)
      : chrome.i18n.getMessage('deleteChannel')
  )
  const pendingDeleteDescription = $derived(
    chrome.i18n.getMessage('deleteChannelConfirm', pendingDeleteTitle)
  )

  function toggleCollapsed() {
    collapsedOverride = !collapsed
  }

  async function selectChannel(source: string) {
    if (source === activeSource) {
      return
    }
    await onSelect?.(source)
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
      return scope === 'incognito' ? 'Incognito' : 'Chrome'
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
    return 'bg-stone-300'
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
    return value.split('/').filter(Boolean).pop() || ''
  }
</script>

<svelte:window bind:innerWidth={viewportWidth} />

<aside
  class={`h-full shrink-0 overflow-hidden border-r border-emerald-900/10 bg-emerald-50/70 backdrop-blur transition-[width] duration-200 ${
    collapsed ? 'w-12' : 'w-64'
  }`}
  aria-label={chrome.i18n.getMessage('channelsLabel')}
>
  <div class="flex h-full min-h-0 flex-col">
    <div class="flex h-12 shrink-0 items-center gap-2 border-b border-emerald-900/10 px-1.5">
      <Button
        variant="ghost"
        size="icon-sm"
        class="grid place-items-center bg-white/50 text-emerald-900 hover:bg-white/80"
        aria-label={chrome.i18n.getMessage(collapsed ? 'expandChannels' : 'collapseChannels')}
        title={chrome.i18n.getMessage(collapsed ? 'expandChannels' : 'collapseChannels')}
        onclick={toggleCollapsed}
      >
        <History class="size-4" />
      </Button>

      {#if !collapsed}
        <div class="min-w-0 flex-1">
          <div class="truncate text-xs font-bold text-stone-800">
            {chrome.i18n.getMessage('channelsLabel')}
            <Badge variant="outline">
              {channels.length}
            </Badge>
          </div>
        </div>
        <Button
          variant="ghost"
          size="icon-sm"
          class="grid place-items-center bg-white/50 text-emerald-900 hover:bg-white/80"
          aria-label={chrome.i18n.getMessage(collapsed ? 'expandChannels' : 'collapseChannels')}
          title={chrome.i18n.getMessage(collapsed ? 'expandChannels' : 'collapseChannels')}
          onclick={toggleCollapsed}
        >
          <ChevronDown class="size-4 shrink-0 rotate-90 text-stone-400" />
        </Button>
      {/if}
    </div>
    <div class="scrollbar-slim flex min-h-0 flex-1 flex-col gap-1 overflow-y-auto p-1.5">
      {#each channels as channel (channel.source)}
        {@const active = channel.source === activeSource}
        {@const icon = statusIcon(channel)}
        <Item
          size="xs"
          variant={active ? 'outline' : 'default'}
          class={`group relative flex-nowrap p-0 text-left ${
            active
              ? 'border-emerald-900/15 bg-background text-stone-950 shadow-sm'
              : 'text-stone-600 hover:border-emerald-900/10 hover:bg-background/60 hover:text-stone-900'
          } ${collapsed ? 'h-9 justify-center px-0' : ''}`}
        >
          <button
            type="button"
            class={`flex min-w-0 flex-1 items-center gap-2 px-2 py-2 text-left ${
              collapsed ? 'h-9 justify-center px-0' : ''
            }`}
            aria-current={active ? 'page' : undefined}
            aria-label={`${channelTitle(channel.source)} ${statusLabel(channel)}`}
            title={`${channelTitle(channel.source)}\n${channel.source}`}
            onclick={() => selectChannel(channel.source)}
          >
            <ItemMedia
              variant="icon"
              class={`relative grid size-6 place-items-center rounded-md border ${
                active
                  ? 'border-emerald-900/10 bg-emerald-50 text-emerald-800'
                  : 'border-stone-200 bg-white/75 text-stone-500'
              }`}
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
            </ItemMedia>

            {#if !collapsed}
              <ItemContent class="min-w-0 gap-0">
                <div class="flex min-w-0 items-center gap-2">
                  <div class="flex min-w-0 flex-1 items-center gap-2">
                    <ItemTitle class="min-w-0 flex-1 text-xs font-bold">
                      <span class="truncate">{channelTitle(channel.source)}</span>
                    </ItemTitle>
                    {#if active && sending}
                      <LoaderCircle class="size-3 shrink-0 animate-spin text-emerald-700" />
                    {/if}
                  </div>
                  {#if channelMeta(channel)}
                    <Badge variant="secondary" class="h-4 rounded-full px-1.5 text-[10px]">
                      {channelMeta(channel)}
                    </Badge>
                  {/if}
                </div>
                <div class="mt-0.5 flex min-w-0 items-center gap-1.5 text-[10px] text-stone-500">
                  <span class="shrink-0">{statusLabel(channel)}</span>
                  <span class="min-w-0 truncate text-stone-400"
                    >{channelSubtitle(channel.source)}</span
                  >
                </div>
              </ItemContent>
            {/if}
          </button>

          {#if !collapsed}
            <Button
              variant="outline"
              size="icon-xs"
              class="pointer-events-none absolute bottom-1 right-1 z-10 text-stone-400 opacity-0 shadow-sm group-hover:pointer-events-auto group-hover:opacity-100 hover:bg-amber-50 hover:text-amber-700 focus-visible:pointer-events-auto focus-visible:opacity-100"
              aria-label={chrome.i18n.getMessage('deleteChannel')}
              title={chrome.i18n.getMessage('deleteChannel')}
              disabled={sending || channel.sending}
              onclick={() => requestDeleteChannel(channel.source)}
            >
              <Trash2 class="size-3.5" />
            </Button>
          {/if}
        </Item>
      {/each}
    </div>
  </div>
</aside>

<AlertDialog.Root bind:open={deleteDialogOpen}>
  <AlertDialog.Content size="sm">
    <AlertDialog.Header>
      <AlertDialog.Title>{chrome.i18n.getMessage('deleteChannel')}</AlertDialog.Title>
      <AlertDialog.Description>{pendingDeleteDescription}</AlertDialog.Description>
    </AlertDialog.Header>
    <AlertDialog.Footer>
      <AlertDialog.Cancel>{chrome.i18n.getMessage('cancel')}</AlertDialog.Cancel>
      <AlertDialog.Action variant="destructive" onclick={confirmDeleteChannel}>
        {chrome.i18n.getMessage('deleteChannel')}
      </AlertDialog.Action>
    </AlertDialog.Footer>
  </AlertDialog.Content>
</AlertDialog.Root>
