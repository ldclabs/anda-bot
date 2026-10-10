<script lang="ts" module>
  // Survives re-keyed remounts while a turn streams in.
  const expandedRows = new Set<string>()
</script>

<script lang="ts">
  import { prefersReducedMotion } from '$lib/anda/chat/entrance'
  import type { ToolCallStatus } from '$lib/anda/chat/tool-view'
  import { getMessage } from '$lib/i18n'
  import { Check, ChevronRight, CircleDashed, CircleX } from '@lucide/svelte'
  import type { Component, Snippet } from 'svelte'
  import { untrack } from 'svelte'
  import { slide } from 'svelte/transition'

  let {
    rowKey,
    icon: Icon,
    name,
    summary = '',
    mono = false,
    status,
    live = false,
    animate = false,
    enterDelay = 0,
    children
  }: {
    rowKey: string
    icon: Component<{ class?: string }>
    name: string
    summary?: string
    mono?: boolean
    status?: ToolCallStatus
    /** In progress right now: the text shimmers and a running call spins. */
    live?: boolean
    /** Slides in when mounted (read once). */
    animate?: boolean
    enterDelay?: number
    children: Snippet
  } = $props()

  let expanded = $state(untrack(() => expandedRows.has(rowKey)))
  const entering = untrack(() => animate)
  const reveal = { duration: prefersReducedMotion() ? 0 : 200 }

  // A call that finishes while watched shows a check for a moment.
  let finished = $state(false)
  let lastStatus = untrack(() => status)
  $effect(() => {
    const current = status
    const previous = lastStatus
    lastStatus = current
    if (previous !== 'running' || current !== 'ok') return
    finished = true
    const timer = window.setTimeout(() => (finished = false), 1600)
    return () => {
      clearTimeout(timer)
      finished = false
    }
  })

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
<div
  class="min-w-0 text-xs leading-5"
  class:chat-detail-enter={entering}
  style:--row-delay={entering ? `${enterDelay}ms` : undefined}
>
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
    <span class="chat-detail-row-name shrink-0 font-medium" class:anda-shimmer={live}>{name}</span>
    {#if summary}
      <span class="min-w-0 truncate {mono ? 'font-mono text-[11px]' : ''}" class:anda-shimmer={live}
        >{summary}</span
      >
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
        <CircleDashed class="size-3.5 {live ? 'chat-detail-row-spin' : ''}" />
      </span>
    {:else if finished}
      <span class="chat-detail-row-done shrink-0" aria-hidden="true">
        <Check class="size-3.5" />
      </span>
    {/if}
  </button>

  {#if expanded}
    <div
      class="chat-detail-row-body mt-0.5 mb-1.5 ml-3 min-w-0 border-l pl-3"
      transition:slide={reveal}
    >
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

  .chat-detail-row {
    transition:
      background-color 120ms ease-out,
      color 120ms ease-out;
  }

  /* Tailwind's rotate-90 sets `rotate`, which springs open. */
  .chat-detail-row :global(.chat-detail-row-chevron) {
    transition: rotate 260ms var(--anda-spring, ease-out);
  }

  .chat-detail-row :global(.chat-detail-row-spin) {
    animation: chat-detail-spin 1.6s linear infinite;
  }

  .chat-detail-row-done {
    color: #059669;
    animation: chat-detail-done 1600ms var(--anda-ease-out, ease-out) both;
  }

  .chat-detail-enter {
    animation: chat-detail-in 420ms var(--anda-ease-out, ease-out) var(--row-delay, 0ms) both;
  }

  @keyframes chat-detail-in {
    from {
      opacity: 0;
      transform: translateX(-8px);
      filter: blur(2px);
    }
  }

  @keyframes chat-detail-spin {
    to {
      transform: rotate(360deg);
    }
  }

  @keyframes chat-detail-done {
    0% {
      opacity: 0;
      transform: scale(0.3) rotate(-30deg);
    }
    18% {
      opacity: 1;
      transform: scale(1.25) rotate(0deg);
    }
    30%,
    72% {
      opacity: 1;
      transform: scale(1);
    }
    100% {
      opacity: 0;
      transform: scale(0.85);
    }
  }

  :global(.dark) .chat-detail-row-done {
    color: #34d399;
  }

  @media (prefers-reduced-motion: reduce) {
    .chat-detail-enter,
    .chat-detail-row :global(.chat-detail-row-spin) {
      animation: none;
    }
  }

  .chat-detail-row-body {
    border-color: var(--message-border, #e6e6e6);
  }

  :global(.dark) .chat-detail-row-error .chat-detail-row-name,
  :global(.dark) .chat-detail-row-failed {
    color: #fca5a5;
  }
</style>
