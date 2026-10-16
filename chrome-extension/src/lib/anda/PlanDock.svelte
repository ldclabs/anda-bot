<script lang="ts">
  /**
   * The steps of the request the agent is working on, docked above the
   * composer so the user can follow the work without reading every message.
   * The agent keeps the list; the daemon drops it when a new request starts.
   */
  import type { PlanStep } from '$lib/anda/client/types'
  import { getMessage } from '$lib/i18n'
  import {
    ChevronDown,
    ChevronUp,
    CircleCheck,
    CircleDashed,
    CircleDot,
    ListChecks,
    LoaderCircle
  } from '@lucide/svelte'

  let {
    steps,
    working = false
  }: {
    steps: PlanStep[]
    /** The turn is running: the step in progress spins. */
    working?: boolean
  } = $props()

  let collapsed = $state(false)
  let list = $state<HTMLOListElement | null>(null)

  const done = $derived(steps.filter((step) => step.status === 'completed').length)
  const currentIndex = $derived(steps.findIndex((step) => step.status === 'in_progress'))
  // Collapsed, the header names the step in progress, else the next or last one.
  const headline = $derived(
    steps[currentIndex]?.content ||
      steps.find((step) => step.status === 'pending')?.content ||
      steps.at(-1)?.content ||
      ''
  )
  const progress = $derived(getMessage('planDockProgress', [String(done), String(steps.length)]))

  // A long plan scrolls; keep the step in progress in view as it advances.
  $effect(() => {
    if (collapsed || currentIndex < 0) return
    list?.children[currentIndex]?.scrollIntoView({ block: 'nearest' })
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
    <header class="plan-dock-header">
      <span class="plan-dock-icon"><ListChecks class="size-4" /></span>
      <h2 class="plan-dock-title" title={collapsed ? headline : undefined}>
        {collapsed ? headline : getMessage('planDockLabel')}
      </h2>
      <span class="plan-dock-count" title={progress}>
        <span aria-hidden="true">{done}/{steps.length}</span>
        <span class="sr-only">{progress}</span>
      </span>
      <button
        type="button"
        class="plan-dock-toggle"
        aria-expanded={!collapsed}
        aria-label={getMessage(collapsed ? 'actionDockExpand' : 'actionDockCollapse')}
        title={getMessage(collapsed ? 'actionDockExpand' : 'actionDockCollapse')}
        onclick={() => (collapsed = !collapsed)}
      >
        {#if collapsed}
          <ChevronUp class="size-4" />
        {:else}
          <ChevronDown class="size-4" />
        {/if}
      </button>
    </header>

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
            <span class="plan-dock-text">{step.content}</span>
          </li>
        {/each}
      </ol>
    {/if}
  </section>
{/if}

<style>
  /* Reads the host's --message-* palette, like the action dock below it. */
  .plan-dock {
    display: grid;
    gap: 0.375rem;
    margin-bottom: 0.5rem;
    border: 1px solid var(--message-border, #e6e6e6);
    border-radius: 1rem;
    background: var(--message-bg, #ffffff);
    padding: 0.5rem 0.625rem 0.625rem;
    color: var(--message-text, #171717);
    font-size: 0.8125rem;
    box-shadow: 0 6px 24px rgba(0, 0, 0, 0.06);
    animation: plan-dock-in 160ms ease-out;
  }

  .plan-dock-collapsed {
    padding-bottom: 0.5rem;
  }

  @keyframes plan-dock-in {
    from {
      opacity: 0;
      transform: translateY(6px);
    }
  }

  .plan-dock-header {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    min-width: 0;
    padding-inline-start: 0.25rem;
  }

  .plan-dock-icon {
    display: grid;
    flex-shrink: 0;
    place-items: center;
    color: var(--message-muted, #737373);
  }

  .plan-dock-title {
    flex: 1;
    min-width: 0;
    margin: 0;
    overflow: hidden;
    font-size: 0.8125rem;
    font-weight: 600;
    line-height: 1.25rem;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .plan-dock-count {
    flex-shrink: 0;
    color: var(--message-muted, #737373);
    font-size: 0.75rem;
    font-variant-numeric: tabular-nums;
  }

  .plan-dock-toggle {
    display: grid;
    flex-shrink: 0;
    place-items: center;
    width: 1.5rem;
    height: 1.5rem;
    border-radius: 0.375rem;
    color: var(--message-muted, #737373);
  }

  .plan-dock-toggle:hover {
    background: var(--message-surface-hover, #eeeeee);
    color: var(--message-text, #171717);
  }

  .plan-dock-steps {
    display: grid;
    gap: 0.125rem;
    max-height: min(28vh, 14rem);
    margin: 0;
    overflow-y: auto;
    overscroll-behavior: contain;
    padding: 0 0.25rem;
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
    .plan-dock {
      animation: none;
    }

    :global(.plan-dock-spinner) {
      animation: none;
    }
  }
</style>
