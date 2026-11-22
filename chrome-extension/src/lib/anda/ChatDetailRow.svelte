<script lang="ts" module>
  // Preserve the reader's choice as polls replace records and chats are reopened,
  // and as the working line's activity settles into the transcript.
  export const expandedRows = new Set<string>()
</script>

<script lang="ts">
  import { getMessage } from '$lib/i18n'
  import type { ToolCallStatus } from './chat/tool-view'
  import { ChevronRight, CircleDashed, CircleX } from '@lucide/svelte'
  import { untrack, type Component, type Snippet } from 'svelte'

  let {
    rowKey,
    icon: Icon,
    name,
    summary = '',
    issue = '',
    mono = false,
    inline = false,
    status,
    children
  }: {
    rowKey: string
    icon: Component<{ class?: string }>
    name: string
    summary?: string
    issue?: string
    mono?: boolean
    inline?: boolean
    status?: ToolCallStatus
    children: Snippet
  } = $props()

  // A single detail is shown directly by its parent. Keep it open if the group grows.
  let expanded = $state(untrack(() => inline || expandedRows.has(rowKey)))

  function toggle() {
    expanded = !expanded
    if (expanded) expandedRows.add(rowKey)
    else expandedRows.delete(rowKey)
  }
</script>

<div class="min-w-0 text-xs leading-5">
  {#if inline}
    {#if mono}<span dir="auto" class="font-mono font-medium">{name}</span>{/if}
  {:else}
    <button
      type="button"
      class="chat-detail-row flex min-h-7 max-w-full min-w-0 items-center gap-1.5 rounded-md px-1.5 py-0.5 text-start"
      aria-expanded={expanded}
      title={summary || undefined}
      onclick={toggle}
    >
      <ChevronRight
        aria-hidden="true"
        class="detail-chevron size-3 shrink-0 {expanded ? 'open' : ''}"
      />
      <Icon class="size-3.5 shrink-0" />
      <span class="detail-label min-w-0 truncate font-medium">{name}</span>
      {#if summary}
        <span class="min-w-0 truncate {mono ? 'font-mono text-[11px]' : ''}">{summary}</span>
      {/if}
      {#if issue || status === 'error'}
        <span class="detail-issue inline-flex shrink-0 items-center gap-1">
          <CircleX class="size-3.5" aria-hidden="true" />
          <span class:sr-only={!issue}>{issue || getMessage('toolFailed')}</span>
        </span>
      {:else if status === 'running'}
        <CircleDashed
          class="size-3.5 shrink-0"
          role="img"
          aria-label={getMessage('toolNoResult')}
        />
      {/if}
    </button>
  {/if}
  {#if inline || expanded}
    <div class="min-w-0" class:detail-body={!inline}>{@render children()}</div>
  {/if}
</div>

<style>
  .chat-detail-row {
    color: var(--message-muted, #737373);
  }
  .detail-label {
    color: color-mix(in srgb, var(--message-text, #171717) 80%, var(--message-bg, white));
  }
  .chat-detail-row:hover {
    background: var(--message-surface-hover, #eee);
  }
  .chat-detail-row:focus-visible {
    outline: 2px solid var(--chat-accent, #059669);
    outline-offset: 2px;
  }
  .chat-detail-row:dir(rtl) :global(.detail-chevron) {
    transform: rotate(180deg);
  }
  .chat-detail-row :global(.detail-chevron.open),
  .chat-detail-row:dir(rtl) :global(.detail-chevron.open) {
    transform: rotate(90deg);
  }
  .detail-body {
    margin-block: 0.25rem;
    margin-inline-start: 0.75rem;
    border-inline-start: 1px solid var(--message-border, #e6e6e6);
    padding-inline-start: 0.75rem;
  }
  .detail-issue {
    color: #b45309;
  }
  :global(.dark) .detail-issue {
    color: #fcd19a;
  }
</style>
