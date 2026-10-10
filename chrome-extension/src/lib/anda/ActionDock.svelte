<script lang="ts" module>
  import {
    actionApproveLabel,
    actionChoiceInputKey,
    actionDefaultChoiceId,
    actionDenyLabel,
    actionDetailIsBlock,
    actionDetailLabel,
    actionDetailText,
    actionKindLabel,
    actionMessage,
    actionTitle,
    choiceHasInput,
    choiceInputPlaceholder,
    choiceInputRequired,
    countdownLabel,
    isApprovalAction,
    isPaymentApproval,
    isShellApproval
  } from '$lib/anda/chat/action-view'
  import type { ChatAction, ChatActionChoice } from '$lib/anda/client/types'

  /** One answer row: a click, its number key, or Enter on it answers at once. */
  type DockRow =
    | { kind: 'approve' | 'deny'; id: string; label: string }
    | { kind: 'choice'; id: string; label: string; choice: ChatActionChoice }
    | { kind: 'reply'; id: string; label: string; placeholder: string }

  /** Rows that take typed text: the free-text reply and choices with an input. */
  function rowHasInput(row: DockRow): boolean {
    return row.kind === 'reply' || (row.kind === 'choice' && choiceHasInput(row.choice))
  }

  function rowPlaceholder(row: DockRow): string {
    if (row.kind === 'reply') return row.placeholder
    return row.kind === 'choice' ? choiceInputPlaceholder(row.choice) : ''
  }

  function isTextField(
    target: EventTarget | null
  ): target is HTMLInputElement | HTMLTextAreaElement {
    return target instanceof HTMLInputElement || target instanceof HTMLTextAreaElement
  }
</script>

<script lang="ts">
  /**
   * The chat's open question, docked above the composer so it stays in reach
   * while the transcript scrolls; the transcript keeps the record. A free-text
   * row replies in chat: for a choice that is the "other" answer (the daemon
   * releases the card as answered in chat), and for an approval it denies
   * first, then says what to do instead.
   */
  import { useAndaClient } from '$lib/anda/client/context'
  import { getMessage } from '$lib/i18n'
  import {
    ArrowUp,
    ChevronDown,
    ChevronUp,
    CreditCard,
    ListChecks,
    LoaderCircle,
    ShieldCheck,
    Terminal
  } from '@lucide/svelte'
  import { tick } from 'svelte'

  let {
    pending,
    onReply,
    shortcuts
  }: {
    /** Pending actions in transcript order; the dock shows the first. */
    pending: ChatAction[]
    /** Sends a chat message. Without it there is no free-text row. */
    onReply?: (text: string) => Promise<void> | void
    /** Labels of the host's own approve and deny shortcuts, shown as a hint. */
    shortcuts?: { approve: string; deny: string }
  } = $props()

  const andaClient = useAndaClient()
  let root = $state<HTMLElement | null>(null)
  // Per-question state is keyed by the action id, so the next question starts fresh.
  let collapsedId = $state('')
  let focused = $state({ actionId: '', index: -1 })
  let failure = $state({ actionId: '', text: '' })
  let respondingRow = $state('')
  let drafts = $state<Record<string, string>>({})
  let now = $state(Date.now())
  let shownActionId = ''
  let shownAt = 0

  const action = $derived(pending[0])
  const approval = $derived(Boolean(action && isApprovalAction(action)))
  const title = $derived(action ? actionTitle(action) || actionKindLabel(action) : '')
  const message = $derived(action ? actionMessage(action) || '' : '')
  const defaultChoiceId = $derived(action ? actionDefaultChoiceId(action) : '')
  const rows = $derived.by<DockRow[]>(() => {
    if (!action) return []
    const replyRow = (label: string, placeholder: string): DockRow[] =>
      onReply ? [{ kind: 'reply', id: '$reply', label, placeholder }] : []
    if (approval) {
      return [
        { kind: 'approve', id: '$approve', label: actionApproveLabel(action) },
        { kind: 'deny', id: '$deny', label: actionDenyLabel(action) },
        ...replyRow(getMessage('actionDenyWithReply'), getMessage('actionDenyWithReplyPlaceholder'))
      ]
    }
    const choices = action.choices || []
    return [
      ...choices.map((choice): DockRow => ({
        kind: 'choice',
        id: choice.id,
        label: choice.label,
        choice
      })),
      // A choice that takes typed input already is the "other" answer.
      ...(choices.some(choiceHasInput)
        ? []
        : replyRow(getMessage('actionChoiceOther'), getMessage('actionChoiceOtherPlaceholder')))
    ]
  })
  const collapsed = $derived(Boolean(action) && collapsedId === action?.id)
  // The highlighted row follows focus and otherwise rests on the recommended one.
  const active = $derived(
    focused.actionId === action?.id && focused.index >= 0
      ? focused.index
      : Math.max(
          0,
          rows.findIndex((row) => row.id === defaultChoiceId)
        )
  )
  const error = $derived(failure.actionId === action?.id ? failure.text : '')
  const disabled = $derived(Boolean(respondingRow) || Boolean(andaClient.readOnly))
  const deadline = $derived(defaultChoiceId ? action?.expiresAt : undefined)
  const countdown = $derived(countdownLabel(deadline, now))
  const hint = $derived.by(() => {
    if (countdown) {
      const label = action?.choices?.find((choice) => choice.id === defaultChoiceId)?.label || ''
      return getMessage('actionChoiceAutoSelect', [label, countdown])
    }
    return approval && shortcuts
      ? getMessage('actionDockShortcuts', [shortcuts.approve, shortcuts.deny])
      : ''
  })

  // The auto-select countdown ticks only while a default is waiting.
  $effect(() => {
    if (!deadline) return
    now = Date.now()
    const timer = window.setInterval(() => (now = Date.now()), 1000)
    return () => window.clearInterval(timer)
  })

  // A new choice takes keyboard focus when nothing else holds it, so its
  // number keys work at once. Approvals never do: a stray Enter must not run a command.
  $effect(() => {
    const id = action?.id || ''
    if (id === shownActionId) return
    shownActionId = id
    shownAt = performance.now()
    if (!id || approval) return
    void tick().then(() => {
      const current = document.activeElement
      if (!current || current === document.body) rowTarget(active)?.focus({ preventScroll: true })
    })
  })

  function rowTarget(index: number): HTMLElement | null {
    return root?.querySelector<HTMLElement>(`[data-row="${index}"]`) ?? null
  }

  function draftKey(row: DockRow): string {
    return action ? actionChoiceInputKey(action, row.id) : ''
  }

  function rowText(row: DockRow): string {
    return rowHasInput(row) ? (drafts[draftKey(row)] || '').trim() : ''
  }

  function rowReady(row: DockRow): boolean {
    if (row.kind === 'reply') return Boolean(rowText(row))
    if (row.kind === 'choice' && choiceInputRequired(row.choice)) return Boolean(rowText(row))
    return true
  }

  /**
   * A row with an input takes focus, so its text can be typed; any other row
   * answers. The next question lands under the pointer, so a click or key
   * right after it appears is the tail of the last answer, not a new one.
   */
  function activate(index: number) {
    const row = rows[index]
    if (!row || disabled || performance.now() - shownAt < 300) return
    if (rowHasInput(row)) rowTarget(index)?.focus()
    else void answer(row)
  }

  async function answer(row: DockRow) {
    const current = action
    if (!current || disabled || !rowReady(row)) return
    const text = rowText(row)
    respondingRow = row.id
    failure = { actionId: '', text: '' }
    try {
      if (row.kind === 'approve' || row.kind === 'deny') {
        await andaClient.respondAction({ actionId: current.id, approve: row.kind === 'approve' })
      } else if (row.kind === 'choice') {
        await andaClient.respondAction({
          actionId: current.id,
          choiceId: row.id,
          choiceText: text || undefined
        })
      } else {
        // Denied first, so the agent reads why it stopped before the new instruction.
        if (isApprovalAction(current)) {
          await andaClient.respondAction({ actionId: current.id, approve: false })
        }
        await onReply?.(text)
      }
      delete drafts[actionChoiceInputKey(current, row.id)]
    } catch (err) {
      failure = {
        actionId: current.id,
        text: err instanceof Error ? err.message : String(err || getMessage('actionFailed'))
      }
    } finally {
      respondingRow = ''
    }
  }

  /** Answers the docked approval; the host's shortcuts call this. */
  export function respond(approve: boolean): Promise<void> {
    const row = approval ? rows.find((r) => r.kind === (approve ? 'approve' : 'deny')) : undefined
    return row ? answer(row) : Promise.resolve()
  }

  function keydown(event: KeyboardEvent) {
    if (collapsed || event.isComposing || event.metaKey || event.ctrlKey || event.altKey) return
    const target = event.target
    const index = Number((target as HTMLElement | null)?.dataset?.row ?? -1)
    if (isTextField(target)) {
      if (event.key === 'Enter' && !event.shiftKey) {
        event.preventDefault()
        if (rows[index]) void answer(rows[index])
      } else if (
        target instanceof HTMLInputElement &&
        (event.key === 'ArrowDown' || event.key === 'ArrowUp')
      ) {
        event.preventDefault()
        move(index, event.key === 'ArrowDown' ? 1 : -1)
      }
      return
    }
    if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
      event.preventDefault()
      move(index >= 0 ? index : active, event.key === 'ArrowDown' ? 1 : -1)
    } else if (/^[1-9]$/.test(event.key) && Number(event.key) <= rows.length) {
      event.preventDefault()
      activate(Number(event.key) - 1)
    }
  }

  function move(from: number, step: number) {
    rowTarget((from + step + rows.length) % rows.length)?.focus()
  }

  function rowFocused(index: number) {
    if (action) focused = { actionId: action.id, index }
  }

  function toggleCollapsed() {
    if (action) collapsedId = collapsed ? '' : action.id
  }
</script>

{#if action}
  <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
  <section
    bind:this={root}
    class="action-dock"
    class:action-dock-collapsed={collapsed}
    aria-label={getMessage('actionWaiting')}
    onkeydown={keydown}
  >
    <header class="action-dock-header">
      <span class="action-dock-icon" class:action-dock-icon-risk={approval}>
        {#if isShellApproval(action)}
          <Terminal class="size-4" />
        {:else if isPaymentApproval(action)}
          <CreditCard class="size-4" />
        {:else if approval}
          <ShieldCheck class="size-4" />
        {:else}
          <ListChecks class="size-4" />
        {/if}
      </span>
      <h2 class="action-dock-title" {title}>{title}</h2>
      {#if pending.length > 1}
        <span class="action-dock-more">
          {getMessage('actionDockMore', String(pending.length - 1))}
        </span>
      {/if}
      <button
        type="button"
        class="action-dock-toggle"
        aria-expanded={!collapsed}
        aria-label={getMessage(collapsed ? 'actionDockExpand' : 'actionDockCollapse')}
        title={getMessage(collapsed ? 'actionDockExpand' : 'actionDockCollapse')}
        onclick={toggleCollapsed}
      >
        {#if collapsed}
          <ChevronUp class="size-4" />
        {:else}
          <ChevronDown class="size-4" />
        {/if}
      </button>
    </header>

    {#if !collapsed}
      {#if message || action.summary || action.details?.length || action.command}
        <div class="action-dock-body scrollbar-slim">
          {#if message}
            <p class="action-dock-message">{message}</p>
          {/if}
          {#if action.summary}
            <p class="action-dock-message">{action.summary}</p>
          {/if}
          {#if action.details?.length}
            {#each action.details as detail, detailIndex (`${detail.label}-${detailIndex}`)}
              <div class="action-dock-detail">
                <div class="action-dock-detail-label">{actionDetailLabel(detail)}</div>
                {#if actionDetailIsBlock(detail)}
                  <pre class="action-dock-code"><code>{actionDetailText(detail)}</code></pre>
                {:else}
                  <div class="action-dock-detail-text">{actionDetailText(detail)}</div>
                {/if}
              </div>
            {/each}
          {:else if action.command}
            <pre class="action-dock-code"><code>{action.command}</code></pre>
            {#if action.workspace}
              <div class="action-dock-meta" title={action.workspace}>
                {action.workspace}{#if action.background}
                  · {getMessage('actionBackground')}{/if}
              </div>
            {/if}
          {/if}
        </div>
      {/if}

      <div class="action-dock-options" role="group" aria-label={title}>
        {#each rows as row, index (row.id)}
          {@const key = String(index + 1)}
          {#if rowHasInput(row)}
            {@const multiline = row.kind === 'choice' && row.choice.input?.multiline}
            {@const placeholder = rowPlaceholder(row)}
            <label
              class="action-dock-option action-dock-option-input"
              class:action-dock-option-active={index === active}
            >
              <span class="action-dock-option-head">
                <span class="action-dock-option-text">
                  <span class="action-dock-option-label">{row.label}</span>
                  {#if row.kind === 'choice' && row.choice.description}
                    <span class="action-dock-option-description">{row.choice.description}</span>
                  {/if}
                </span>
                <kbd class="action-dock-key" aria-hidden="true">{key}</kbd>
              </span>
              <span class="action-dock-input-row">
                {#if multiline}
                  <textarea
                    class="action-dock-input"
                    data-row={index}
                    rows="2"
                    aria-label={row.label}
                    {placeholder}
                    {disabled}
                    bind:value={drafts[draftKey(row)]}
                    onfocus={() => rowFocused(index)}></textarea>
                {:else}
                  <input
                    class="action-dock-input"
                    data-row={index}
                    type="text"
                    aria-label={row.label}
                    {placeholder}
                    {disabled}
                    bind:value={drafts[draftKey(row)]}
                    onfocus={() => rowFocused(index)}
                  />
                {/if}
                <button
                  type="button"
                  class="action-dock-send"
                  aria-label={getMessage('actionChoiceSubmit')}
                  title={getMessage('actionChoiceSubmit')}
                  disabled={disabled || !rowReady(row)}
                  onclick={() => void answer(row)}
                >
                  {#if respondingRow === row.id}
                    <LoaderCircle class="size-3.5 animate-spin" />
                  {:else}
                    <ArrowUp class="size-3.5" />
                  {/if}
                </button>
              </span>
            </label>
          {:else}
            <button
              type="button"
              class="action-dock-option"
              class:action-dock-option-active={index === active}
              data-row={index}
              {disabled}
              onfocus={() => rowFocused(index)}
              onclick={() => activate(index)}
            >
              <span class="action-dock-option-text">
                <span class="action-dock-option-label"
                  >{row.label}{#if row.id === defaultChoiceId}<span class="action-dock-badge"
                      >{getMessage('actionChoiceRecommended')}</span
                    >{/if}</span
                >
                {#if row.kind === 'choice' && row.choice.description}
                  <span class="action-dock-option-description">{row.choice.description}</span>
                {/if}
              </span>
              {#if respondingRow === row.id}
                <LoaderCircle class="action-dock-spinner size-3.5 animate-spin" />
              {/if}
              <kbd class="action-dock-key" aria-hidden="true">{key}</kbd>
            </button>
          {/if}
        {/each}
      </div>

      {#if error}
        <p class="action-dock-error" role="alert">{error}</p>
      {/if}
      {#if hint}
        <p class="action-dock-hint">{hint}</p>
      {/if}
    {/if}
  </section>
{/if}

<style>
  /* Reads the host's --message-* palette, like the composer card it sits on. */
  .action-dock {
    display: grid;
    gap: 0.5rem;
    margin-bottom: 0.5rem;
    border: 1px solid var(--message-border, #e6e6e6);
    border-radius: 1rem;
    background: var(--message-bg, #ffffff);
    padding: 0.625rem 0.625rem 0.75rem;
    color: var(--message-text, #171717);
    font-size: 0.8125rem;
    box-shadow: 0 6px 24px rgba(0, 0, 0, 0.06);
    animation: action-dock-in 160ms ease-out;
  }

  .action-dock-collapsed {
    padding-bottom: 0.625rem;
  }

  @keyframes action-dock-in {
    from {
      opacity: 0;
      transform: translateY(6px);
    }
  }

  .action-dock-header {
    display: flex;
    align-items: flex-start;
    gap: 0.5rem;
    min-width: 0;
    padding-left: 0.25rem;
  }

  .action-dock-icon {
    display: grid;
    flex-shrink: 0;
    place-items: center;
    height: 1.25rem;
    color: var(--message-muted, #737373);
  }

  .action-dock-icon-risk {
    color: #b45309;
  }

  .action-dock-title {
    flex: 1;
    min-width: 0;
    margin: 0;
    font-size: 0.875rem;
    font-weight: 600;
    line-height: 1.25rem;
    overflow-wrap: anywhere;
  }

  .action-dock-collapsed .action-dock-title {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .action-dock-more {
    flex-shrink: 0;
    line-height: 1.25rem;
    color: var(--message-muted, #737373);
    font-size: 0.75rem;
    white-space: nowrap;
  }

  .action-dock-toggle {
    display: grid;
    flex-shrink: 0;
    place-items: center;
    width: 1.5rem;
    height: 1.5rem;
    margin: -0.125rem 0;
    border-radius: 0.375rem;
    color: var(--message-muted, #737373);
  }

  .action-dock-toggle:hover {
    background: var(--message-surface-hover, #eeeeee);
    color: var(--message-text, #171717);
  }

  .action-dock-body {
    display: grid;
    gap: 0.5rem;
    max-height: min(32vh, 16rem);
    overflow-y: auto;
    overscroll-behavior: contain;
    padding: 0 0.25rem;
  }

  .action-dock-message {
    margin: 0;
    color: color-mix(in srgb, var(--message-text, #171717) 82%, transparent);
    line-height: 1.5;
    overflow-wrap: anywhere;
    white-space: pre-wrap;
  }

  .action-dock-detail {
    display: grid;
    gap: 0.25rem;
    min-width: 0;
  }

  .action-dock-detail-label {
    color: var(--message-muted, #737373);
    font-size: 0.6875rem;
    font-weight: 600;
  }

  .action-dock-detail-text,
  .action-dock-meta {
    overflow-wrap: anywhere;
  }

  .action-dock-meta {
    color: var(--message-muted, #737373);
    font-size: 0.75rem;
  }

  .action-dock-code {
    margin: 0;
    max-height: 9rem;
    overflow: auto;
    border-radius: 0.5rem;
    background: var(--message-surface, #f7f7f7);
    padding: 0.5rem 0.625rem;
    font-size: 0.75rem;
    line-height: 1.5;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }

  .action-dock-options {
    display: grid;
    gap: 0.375rem;
  }

  .action-dock-option {
    display: flex;
    align-items: flex-start;
    gap: 0.75rem;
    width: 100%;
    min-width: 0;
    border: 1px solid transparent;
    border-radius: 0.625rem;
    background: var(--message-surface, #f7f7f7);
    padding: 0.5rem 0.625rem 0.5rem 0.75rem;
    color: inherit;
    text-align: left;
    transition:
      background-color 120ms ease-out,
      border-color 120ms ease-out;
  }

  .action-dock-option:not(.action-dock-option-active):hover:not(:disabled) {
    background: var(--message-surface-hover, #eeeeee);
  }

  .action-dock-option.action-dock-option-active {
    border-color: var(--message-border, #e6e6e6);
    background: var(--message-bg, #ffffff);
    box-shadow: 0 1px 3px rgba(0, 0, 0, 0.05);
  }

  .action-dock-option:focus-visible {
    outline: 2px solid color-mix(in srgb, var(--message-text, #171717) 28%, transparent);
    outline-offset: 1px;
  }

  .action-dock-option:disabled {
    opacity: 0.6;
  }

  .action-dock-option-input {
    flex-direction: column;
    align-items: stretch;
    gap: 0.375rem;
    cursor: text;
  }

  .action-dock-option-head {
    display: flex;
    align-items: flex-start;
    gap: 0.75rem;
  }

  .action-dock-option-text {
    display: grid;
    flex: 1;
    gap: 0.125rem;
    min-width: 0;
  }

  .action-dock-option-label {
    font-weight: 500;
    line-height: 1.25rem;
    overflow-wrap: anywhere;
  }

  .action-dock-option-description {
    color: var(--message-muted, #737373);
    font-size: 0.75rem;
    line-height: 1.45;
    overflow-wrap: anywhere;
  }

  .action-dock-badge {
    margin-left: 0.375rem;
    color: var(--message-muted, #737373);
    font-size: 0.6875rem;
    font-weight: 400;
  }

  .action-dock-key {
    display: grid;
    flex-shrink: 0;
    place-items: center;
    min-width: 1.25rem;
    height: 1.25rem;
    border: 1px solid var(--message-border, #e6e6e6);
    border-radius: 0.3125rem;
    background: var(--message-bg, #ffffff);
    padding: 0 0.25rem;
    color: var(--message-muted, #737373);
    font-family: inherit;
    font-size: 0.6875rem;
  }

  :global(.action-dock-spinner) {
    flex-shrink: 0;
    margin-top: 0.1875rem;
    color: var(--message-muted, #737373);
  }

  .action-dock-input-row {
    display: flex;
    align-items: flex-end;
    gap: 0.375rem;
  }

  .action-dock-input {
    flex: 1;
    min-width: 0;
    border: 1px solid var(--message-border, #e6e6e6);
    border-radius: 0.5rem;
    background: var(--message-bg, #ffffff);
    padding: 0.375rem 0.5rem;
    color: inherit;
    font: inherit;
    line-height: 1.4;
    outline: none;
    resize: vertical;
  }

  .action-dock-input::placeholder {
    color: var(--message-muted-soft, #a0a0a0);
  }

  .action-dock-input:focus {
    border-color: color-mix(
      in srgb,
      var(--message-text, #171717) 35%,
      var(--message-border, #e6e6e6)
    );
  }

  .action-dock-send {
    display: grid;
    flex-shrink: 0;
    place-items: center;
    width: 1.875rem;
    height: 1.875rem;
    border-radius: 999px;
    background: var(--message-text, #171717);
    color: var(--message-bg, #ffffff);
  }

  .action-dock-send:disabled {
    opacity: 0.3;
  }

  .action-dock-error,
  .action-dock-hint {
    margin: 0;
    padding: 0 0.25rem;
    font-size: 0.75rem;
    line-height: 1.4;
  }

  .action-dock-error {
    color: #b91c1c;
  }

  .action-dock-hint {
    color: var(--message-muted, #737373);
  }

  :global(.dark) .action-dock {
    box-shadow: 0 6px 24px rgba(0, 0, 0, 0.3);
  }

  :global(.dark) .action-dock-icon-risk {
    color: #fbbf24;
  }

  :global(.dark) .action-dock-error {
    color: #fca5a5;
  }

  @media (prefers-reduced-motion: reduce) {
    .action-dock {
      animation: none;
    }

    .action-dock-option {
      transition: none;
    }
  }
</style>
