<script lang="ts">
  import { ChevronRight } from '@lucide/svelte'
  import { fly } from 'svelte/transition'
  import { getMessage } from '$lib/i18n'
  import AndaMark from './AndaMark.svelte'
  import ChatActivityDetails from './ChatActivityDetails.svelte'
  import { expandedRows } from './ChatDetailRow.svelte'
  import { timelineRevealEnd } from './ChatTimeline.svelte'
  import { activityCounts, activityLabel } from './chat/activity-summary'
  import { prefersReducedMotion } from './chat/entrance'
  import type { ActivityRow } from './chat/message-display'

  // The transcript's last line while a turn runs, below the plan: Anda reads
  // along, the label shimmers, and each new step slides in over the one before.
  // The line opens into the work in progress, which settles into the transcript
  // above the plan once a reply follows it.
  let {
    label,
    elapsed = '',
    step = '',
    activity
  }: {
    label: string
    elapsed?: string
    step?: string
    /** The running turn's latest run of work, if any. */
    activity?: ActivityRow
  } = $props()

  const stepIn = { y: 8, duration: prefersReducedMotion() ? 0 : 280 }
  const detailsId = $props.id()
  const revealEnd = timelineRevealEnd()
  // The settled row reads the same choice, so an open run stays open.
  let expanded = $derived(activity ? expandedRows.has(activity.id) : false)
  const summary = $derived(
    activity ? activityLabel(activityCounts(activity.details)) || getMessage('chatActivity') : ''
  )

  function toggle() {
    if (!activity) return
    expanded = !expanded
    if (expanded) expandedRows.add(activity.id)
    else expandedRows.delete(activity.id)
    // The line ends the transcript: show what opened, then keep up with new calls.
    if (expanded) revealEnd?.()
  }
</script>

<!-- Carries the live activity's id, so a link into the running work lands here. -->
<div class="chat-working" id={activity?.id}>
  <div class="chat-working-line">
    <span class="chat-working-mark" aria-hidden="true">
      <AndaMark working class="chat-working-panda" />
    </span>
    <span class="chat-working-status" role="status">
      <span class="chat-working-label anda-shimmer">{label}</span>
      {#if elapsed}
        <span aria-hidden="true" class="chat-working-elapsed">{elapsed}</span>
      {/if}
      {#if step}
        <span class="chat-working-step">
          {#key step}
            <code title={step} in:fly={stepIn}>{step}</code>
          {/key}
        </span>
      {/if}
    </span>
    {#if activity}
      <button
        type="button"
        class="chat-working-toggle"
        aria-expanded={expanded}
        aria-controls={expanded ? detailsId : undefined}
        title={summary}
        onclick={toggle}
      >
        <span class="sr-only">{summary}</span>
        <ChevronRight
          aria-hidden="true"
          class="chat-working-chevron size-3.5 {expanded ? 'open' : ''}"
        />
      </button>
    {/if}
  </div>
  {#if activity && expanded}
    <div id={detailsId} class="chat-working-details">
      <ChatActivityDetails details={activity.details} />
    </div>
  {/if}
</div>

<style>
  .chat-working {
    min-width: 0;
    padding: 0.75rem 0;
    color: var(--message-muted, #737373);
    font-size: 0.75rem;
    line-height: 1rem;
    animation: chat-working-in 420ms var(--anda-ease-out, ease-out) both;
  }

  .chat-working-line {
    position: relative;
    isolation: isolate;
    display: flex;
    min-width: 0;
    align-items: center;
    gap: 0.5rem;
  }

  .chat-working-status {
    display: flex;
    min-width: 0;
    align-items: center;
    gap: 0.5rem;
  }

  .chat-working-mark {
    position: relative;
    display: grid;
    flex-shrink: 0;
    place-items: center;
    width: 1.5rem;
    height: 1.25rem;
  }

  /* A soft aura that breathes behind the mark. */
  .chat-working-mark::before {
    position: absolute;
    inset: -0.375rem -0.25rem;
    border-radius: 999px;
    background: radial-gradient(
      closest-side,
      color-mix(in srgb, var(--chat-accent, #10b981) 32%, transparent),
      transparent
    );
    content: '';
    animation: chat-working-aura 1.9s ease-in-out infinite;
  }

  .chat-working-mark :global(.chat-working-panda) {
    position: relative;
    width: 1.375rem;
    height: auto;
  }

  .chat-working-label {
    flex-shrink: 0;
    font-weight: 500;
  }

  .chat-working-elapsed {
    flex-shrink: 0;
    font-variant-numeric: tabular-nums;
  }

  .chat-working-step {
    display: grid;
    min-width: 0;
    overflow: hidden;
  }

  .chat-working-step code {
    grid-area: 1 / 1;
    min-width: 0;
    overflow: hidden;
    font-size: 0.6875rem;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .chat-working-toggle {
    display: grid;
    flex-shrink: 0;
    place-items: center;
    width: 1.25rem;
    height: 1.25rem;
    color: inherit;
  }

  /* The whole line opens the work in progress, not just the chevron: the
     button's overlay takes the clicks, the line's backdrop shows the hover. */
  .chat-working-toggle::after,
  .chat-working-line::before {
    position: absolute;
    inset: -0.25rem -0.375rem;
    border-radius: 0.5rem;
    content: '';
  }
  .chat-working-line::before {
    z-index: -1;
  }
  .chat-working-line:has(.chat-working-toggle:hover)::before {
    background: var(--message-surface-hover, #eee);
  }
  .chat-working-toggle:focus-visible {
    outline: none;
  }
  .chat-working-toggle:focus-visible::after {
    outline: 2px solid var(--chat-accent, #059669);
  }

  .chat-working-toggle :global(.chat-working-chevron) {
    transition: transform 160ms var(--anda-ease-out, ease-out);
  }
  .chat-working-toggle:dir(rtl) :global(.chat-working-chevron) {
    transform: rotate(180deg);
  }
  .chat-working-toggle :global(.chat-working-chevron.open),
  .chat-working-toggle:dir(rtl) :global(.chat-working-chevron.open) {
    transform: rotate(90deg);
  }

  /* Indented under the mark, like an opened detail row. */
  .chat-working-details {
    margin-block-start: 0.5rem;
    margin-inline-start: 0.75rem;
    border-inline-start: 1px solid var(--message-border, #e6e6e6);
    padding-inline-start: 1.1875rem;
  }

  @keyframes chat-working-in {
    from {
      opacity: 0;
      transform: translateY(6px);
    }
  }

  @keyframes chat-working-aura {
    0%,
    100% {
      opacity: 0.45;
      transform: scale(0.82);
    }
    50% {
      opacity: 1;
      transform: scale(1.12);
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .chat-working,
    .chat-working-mark::before {
      animation: none;
    }
    .chat-working-toggle :global(.chat-working-chevron) {
      transition: none;
    }
  }
</style>
