<script lang="ts">
  /**
   * The steps of the request the agent is working on. It sits in the
   * transcript under the latest message, above the work in progress, so the
   * user can follow the work without reading every message. The agent keeps
   * the list; the daemon drops it when a new request starts.
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
    working = false
  }: {
    steps: PlanStep[]
    /** The turn is running: the step in progress spins. */
    working?: boolean
  } = $props()

  const done = $derived(steps.filter((step) => step.status === 'completed').length)
  const complete = $derived(steps.length > 0 && done === steps.length)
  // A finished plan folds to one line. A click overrides this until the plan
  // finishes or reopens; polls with the same completion state leave it alone.
  let collapsed = $derived(complete)
  // A folded active plan still names the current or next step.
  const headline = $derived(
    steps.find((step) => step.status === 'in_progress')?.content ||
      steps.find((step) => step.status === 'pending')?.content ||
      ''
  )
  const progress = $derived(getMessage('planProgress', [String(done), String(steps.length)]))

  function statusLabel(step: PlanStep): string {
    if (step.status === 'completed') return getMessage('planStepCompleted')
    if (step.status === 'in_progress') return getMessage('planStepInProgress')
    return getMessage('planStepPending')
  }
</script>

{#if steps.length}
  <section
    class="chat-plan"
    class:chat-plan-collapsed={collapsed}
    aria-label={getMessage('planLabel')}
  >
    <h2 class="chat-plan-heading">
      <button
        type="button"
        class="chat-plan-header"
        aria-expanded={!collapsed}
        onclick={() => (collapsed = !collapsed)}
      >
        {#if complete}<CircleCheck class="chat-plan-complete size-4 shrink-0" />
        {:else}<ListChecks class="size-4 shrink-0" />{/if}
        <span class="chat-plan-title">
          {getMessage(complete ? 'planCompleted' : 'planLabel')}
        </span>
        {#if collapsed && !complete && headline}
          <span dir="auto" class="chat-plan-headline" title={headline}>{headline}</span>
        {/if}
        <span class="chat-plan-count" title={progress}>
          <span aria-hidden="true">{done}/{steps.length}</span><span class="sr-only">
            {progress}
          </span>
        </span>
        <ChevronRight
          aria-hidden="true"
          class="chat-plan-chevron size-3.5 shrink-0 {collapsed ? '' : 'open'}"
        />
      </button>
    </h2>

    {#if !collapsed}
      <ol class="chat-plan-steps">
        {#each steps as step, index (index)}
          <li
            class="chat-plan-step"
            data-status={step.status}
            aria-current={step.status === 'in_progress' ? 'step' : undefined}
          >
            <span class="chat-plan-mark" title={statusLabel(step)}>
              {#if step.status === 'completed'}
                <CircleCheck class="size-4" />
              {:else if step.status === 'in_progress' && working}
                <LoaderCircle class="chat-plan-spinner size-4 animate-spin" />
              {:else if step.status === 'in_progress'}
                <CircleDot class="size-4" />
              {:else}
                <CircleDashed class="size-4" />
              {/if}
              <span class="sr-only">{statusLabel(step)}</span>
            </span>
            <span dir="auto" class="chat-plan-text">{step.content}</span>
          </li>
        {/each}
      </ol>
    {/if}
  </section>
{/if}

<style>
  /* Reads the host's --message-* palette, like the rows around it. */
  .chat-plan {
    min-width: 0;
    border: 1px solid var(--message-border, #e6e6e6);
    border-radius: 0.75rem;
    background: var(--message-bg, #fff);
    color: var(--message-text, #171717);
    font-size: 0.8125rem;
    box-shadow: 0 1px 2px rgb(0 0 0 / 0.04);
  }

  /* Folded, the plan reads like the activity rows beside it. */
  .chat-plan-collapsed {
    border-color: transparent;
    background: transparent;
    box-shadow: none;
  }

  .chat-plan-heading {
    margin: 0;
  }

  .chat-plan-header {
    display: flex;
    width: 100%;
    min-height: 2.25rem;
    align-items: center;
    gap: 0.5rem;
    padding: 0.375rem 0.75rem;
    border-radius: inherit;
    color: var(--message-muted, #737373);
    text-align: start;
  }
  .chat-plan-collapsed .chat-plan-header {
    min-height: 1.75rem;
    padding: 0.125rem 0.375rem;
  }
  .chat-plan-header:hover {
    background: var(--message-surface-hover, #eee);
    border-radius: 0.625rem;
  }
  .chat-plan-header:focus-visible {
    outline: 2px solid var(--chat-accent, #059669);
    outline-offset: -2px;
    border-radius: 0.625rem;
  }
  .chat-plan-title {
    flex-shrink: 0;
    color: var(--message-text, #171717);
    font-size: 0.8125rem;
    font-weight: 600;
  }
  .chat-plan-headline {
    min-width: 0;
    overflow: hidden;
    font-size: 0.75rem;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .chat-plan-count {
    margin-inline-start: auto;
    font-size: 0.75rem;
    font-variant-numeric: tabular-nums;
  }
  .chat-plan-header:dir(rtl) :global(.chat-plan-chevron) {
    transform: rotate(180deg);
  }
  .chat-plan-header :global(.chat-plan-chevron.open),
  .chat-plan-header:dir(rtl) :global(.chat-plan-chevron.open) {
    transform: rotate(90deg);
  }
  .chat-plan-header :global(.chat-plan-complete) {
    color: var(--chat-accent, #059669);
  }

  /* A plan holds at most 12 short steps, so the list never scrolls on its own. */
  .chat-plan-steps {
    display: grid;
    gap: 0.25rem;
    margin: 0;
    padding: 0.125rem 0.75rem 0.625rem;
    list-style: none;
  }

  .chat-plan-step {
    display: flex;
    align-items: flex-start;
    gap: 0.5rem;
    min-width: 0;
    line-height: 1.375rem;
  }

  .chat-plan-mark {
    display: grid;
    flex-shrink: 0;
    place-items: center;
    height: 1.375rem;
    color: var(--message-muted, #737373);
  }

  .chat-plan-text {
    min-width: 0;
    overflow-wrap: anywhere;
  }

  .chat-plan-step[data-status='completed'] .chat-plan-text,
  .chat-plan-step[data-status='pending'] .chat-plan-text {
    color: var(--message-muted, #737373);
  }

  .chat-plan-step[data-status='completed'] .chat-plan-mark {
    color: var(--chat-accent, #10b981);
  }

  .chat-plan-step[data-status='in_progress'] .chat-plan-text {
    font-weight: 600;
  }

  .chat-plan-step[data-status='in_progress'] .chat-plan-mark {
    color: var(--message-text, #171717);
  }

  @media (prefers-reduced-motion: reduce) {
    :global(.chat-plan-spinner) {
      animation: none;
    }
  }
</style>
