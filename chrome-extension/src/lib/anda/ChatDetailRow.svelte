<script lang="ts" module>
  // Survives re-keyed remounts while a turn streams in.
  const expandedRows = new Set<string>()
</script>

<script lang="ts">
  import type { ToolCallStatus } from '$lib/anda/chat/tool-view'
  import { getMessage } from '$lib/i18n'
  import { ChevronRight, CircleDashed, CircleX } from '@lucide/svelte'
  import type { Component, Snippet } from 'svelte'
  import { untrack } from 'svelte'

  let {
    rowKey,
    icon: Icon,
    name,
    summary = '',
    mono = false,
    status,
    children
  }: {
    rowKey: string
    icon: Component<{ class?: string }>
    name: string
    summary?: string
    mono?: boolean
    status?: ToolCallStatus
    children: Snippet
  } = $props()

  let expanded = $state(untrack(() => expandedRows.has(rowKey)))

  function toggle() {
    expanded = !expanded
    if (expanded) {
      expandedRows.add(rowKey)
    } else {
      expandedRows.delete(rowKey)
    }
  }
</script>

<!-- Sized on the wrapper: the global `button { font: inherit }` outranks utilities. -->
<div class="min-w-0 text-xs leading-5">
  <button
    type="button"
    class="chat-detail-row flex w-fit max-w-full min-w-0 items-center gap-1.5 rounded-md px-1.5 py-0.5 text-left"
    class:chat-detail-row-error={status === 'error'}
    aria-expanded={expanded}
    title={summary ? `${name} · ${summary}` : name}
    onclick={toggle}
  >
    <ChevronRight
      class="chat-detail-row-chevron size-3 shrink-0 transition-transform {expanded
        ? 'rotate-90'
        : ''}"
    />
    <Icon class="size-3.5 shrink-0" />
    <span class="chat-detail-row-name shrink-0 font-medium">{name}</span>
    {#if summary}
      <span class="min-w-0 truncate {mono ? 'font-mono text-[11px]' : ''}">{summary}</span>
    {/if}
    {#if status === 'error'}
      <span
        class="chat-detail-row-failed shrink-0"
        role="img"
        aria-label={getMessage('toolFailed')}
        title={getMessage('toolFailed')}
      >
        <CircleX class="size-3.5" />
      </span>
    {:else if status === 'running'}
      <span
        class="chat-detail-row-pending shrink-0"
        role="img"
        aria-label={getMessage('toolNoResult')}
        title={getMessage('toolNoResult')}
      >
        <CircleDashed class="size-3.5" />
      </span>
    {/if}
  </button>

  {#if expanded}
    <div class="chat-detail-row-body mt-0.5 mb-1.5 ml-3 min-w-0 border-l pl-3">
      {@render children()}
    </div>
  {/if}
</div>

<style>
  .chat-detail-row {
    color: var(--message-muted, #737373);
  }

  .chat-detail-row:hover {
    background: var(--message-surface-hover, #eeeeee);
    color: var(--message-text, #171717);
  }

  .chat-detail-row-name {
    color: color-mix(in srgb, var(--message-text, #171717) 78%, transparent);
  }

  .chat-detail-row-error .chat-detail-row-name,
  .chat-detail-row-failed {
    color: #b91c1c;
  }

  .chat-detail-row-pending {
    opacity: 0.7;
  }

  .chat-detail-row-body {
    border-color: var(--message-border, #e6e6e6);
  }

  :global(.dark) .chat-detail-row-error .chat-detail-row-name,
  :global(.dark) .chat-detail-row-failed {
    color: #fca5a5;
  }
</style>
