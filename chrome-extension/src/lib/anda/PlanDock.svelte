<script lang="ts">
  /**
   * The steps of the request the agent is working on, docked above the
   * composer so the user can follow the work without reading every message.
   * The agent keeps the list; the daemon drops it when a new request starts.
   */
  import type { PlanStep } from '$lib/anda/client/types'
  import { getMessage } from '$lib/i18n'
  import {
    ChevronRight,
    CircleCheck,
    CircleDashed,
    CircleDot,
    ListChecks,
    LoaderCircle
  } from '@lucide/svelte'

  let {
    steps,
    working = false,
    awaitingResponse = false
  }: {
    steps: PlanStep[]
    /** The turn is running: the step in progress spins. */
    working?: boolean
    awaitingResponse?: boolean
  } = $props()

  let list = $state<HTMLOListElement | null>(null)

  const done = $derived(steps.filter((step) => step.status === 'completed').length)
  const complete = $derived(steps.length > 0 && done === steps.length)
  // A click overrides the default for this phase. Completing the plan folds it;
  // unrelated polls with the same completion state leave the choice alone.
  let collapsed = $derived(complete || awaitingResponse)
  const currentIndex = $derived(steps.findIndex((step) => step.status === 'in_progress'))
  // A folded active plan still names the current or next step.
  const headline = $derived(
    steps[currentIndex]?.content || steps.find((step) => step.status === 'pending')?.content || ''
  )
  const progress = $derived(getMessage('planDockProgress', [String(done), String(steps.length)]))

  // A long plan scrolls; keep the step in progress in view as it advances.
  $effect(() => {
    if (collapsed || currentIndex < 0) return
    const step = list?.children[currentIndex]
    if (!list || !step) return
    const bounds = list.getBoundingClientRect()
    const item = step.getBoundingClientRect()
    if (item.top < bounds.top) list.scrollTop += item.top - bounds.top
    else if (item.bottom > bounds.bottom) list.scrollTop += item.bottom - bounds.bottom
  })

  function statusLabel(step: PlanStep): string {
    if (step.status === 'completed') return getMessage('planStepCompleted')
    if (step.status === 'in_progress') return getMessage('planStepInProgress')
    return getMessage('planStepPending')
  }
</script>

{#if steps.length}
  <section
    class="plan-dock"
    class:plan-dock-collapsed={collapsed}
    aria-label={getMessage('planDockLabel')}
  >
    <h2 class="plan-dock-heading">
      <button
        type="button"
        class="plan-dock-header"
        aria-expanded={!collapsed}
        onclick={() => (collapsed = !collapsed)}
      >
        {#if complete}<CircleCheck class="plan-dock-complete size-4 shrink-0" />
        {:else}<ListChecks class="size-4 shrink-0" />{/if}
        <span class="plan-dock-title">
          {getMessage(complete ? 'planCompleted' : 'planDockLabel')}
        </span>
        {#if collapsed && !complete && headline}
          <span dir="auto" class="plan-dock-headline" title={headline}>{headline}</span>
        {/if}
        <span class="plan-dock-count" title={progress}>
          <span aria-hidden="true">{done}/{steps.length}</span><span class="sr-only">
            {progress}
          </span>
        </span>
        <ChevronRight
          aria-hidden="true"
          class="plan-chevron size-3.5 shrink-0 {collapsed ? '' : 'open'}"
        />
      </button>
    </h2>

    {#if !collapsed}
      <ol bind:this={list} class="plan-dock-steps scrollbar-slim">
        {#each steps as step, index (index)}
          <li
            class="plan-dock-step"
            data-status={step.status}
            aria-current={step.status === 'in_progress' ? 'step' : undefined}
          >
            <span class="plan-dock-mark" title={statusLabel(step)}>
              {#if step.status === 'completed'}
                <CircleCheck class="size-3.5" />
              {:else if step.status === 'in_progress' && working}
                <LoaderCircle class="plan-dock-spinner size-3.5 animate-spin" />
              {:else if step.status === 'in_progress'}
                <CircleDot class="size-3.5" />
              {:else}
                <CircleDashed class="size-3.5" />
              {/if}
              <span class="sr-only">{statusLabel(step)}</span>
            </span>
            <span dir="auto" class="plan-dock-text">{step.content}</span>
          </li>
        {/each}
      </ol>
    {/if}
  </section>
{/if}

<style>
  /* Reads the host's --message-* palette, like the action dock below it. */
  .plan-dock {
    min-width: 0;
    margin-bottom: 0.375rem;
    border: 1px solid var(--message-border, #e6e6e6);
    border-radius: 0.75rem;
    background: var(--message-bg, #fff);
    color: var(--message-text, #171717);
    font-size: 0.8125rem;
  }

  .plan-dock-collapsed {
    border-color: transparent;
    background: transparent;
  }

  .plan-dock-heading {
    margin: 0;
  }

  .plan-dock-header {
    display: flex;
    width: 100%;
    min-height: 2rem;
    align-items: center;
    gap: 0.5rem;
    padding: 0.25rem 0.625rem;
    border-radius: inherit;
    color: var(--message-muted, #737373);
    text-align: start;
  }
  .plan-dock-header:hover {
    background: var(--message-surface-hover, #eee);
    border-radius: 0.625rem;
  }
  .plan-dock-header:focus-visible {
    outline: 2px solid var(--chat-accent, #059669);
    outline-offset: -2px;
    border-radius: 0.625rem;
  }
  .plan-dock-title {
    flex-shrink: 0;
    color: var(--message-text, #171717);
    font-size: 0.8125rem;
    font-weight: 600;
  }
  .plan-dock-headline {
    min-width: 0;
    overflow: hidden;
    font-size: 0.75rem;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .plan-dock-count {
    margin-inline-start: auto;
    font-size: 0.75rem;
    font-variant-numeric: tabular-nums;
  }
  .plan-dock-header:dir(rtl) :global(.plan-chevron) {
    transform: rotate(180deg);
  }
  .plan-dock-header :global(.plan-chevron.open),
  .plan-dock-header:dir(rtl) :global(.plan-chevron.open) {
    transform: rotate(90deg);
  }
  .plan-dock-header :global(.plan-dock-complete) {
    color: var(--chat-accent, #059669);
  }

  .plan-dock-steps {
    display: grid;
    gap: 0.125rem;
    max-height: min(28vh, 14rem);
    margin: 0;
    overflow-y: auto;
    overscroll-behavior: contain;
    padding: 0 0.625rem 0.5rem;
    list-style: none;
  }

  .plan-dock-step {
    display: flex;
    align-items: flex-start;
    gap: 0.5rem;
    min-width: 0;
    padding: 0.125rem 0;
    line-height: 1.25rem;
  }

  .plan-dock-mark {
    display: grid;
    flex-shrink: 0;
    place-items: center;
    height: 1.25rem;
    color: var(--message-muted, #737373);
  }

  .plan-dock-text {
    min-width: 0;
    overflow-wrap: anywhere;
  }

  .plan-dock-step[data-status='completed'] .plan-dock-text {
    color: var(--message-muted, #737373);
  }

  .plan-dock-step[data-status='completed'] .plan-dock-mark {
    color: var(--chat-accent, #10b981);
  }

  .plan-dock-step[data-status='in_progress'] .plan-dock-text {
    font-weight: 600;
  }

  .plan-dock-step[data-status='in_progress'] .plan-dock-mark {
    color: var(--message-text, #171717);
  }

  @media (prefers-reduced-motion: reduce) {
    :global(.plan-dock-spinner) {
      animation: none;
    }
  }
</style>
