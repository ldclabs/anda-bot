<script lang="ts">
  /**
   * A server's events (MCP Events) and the automations that run on them. The
   * event types are read from the server when the tab opens; the automations
   * are refreshed while it shows, since their state changes in the
   * background. Event descriptions come from the server and are untrusted.
   */
  import { useAndaClient } from '$lib/anda/client/context'
  import type {
    McpEventDefinition,
    McpEventsView,
    McpServerView,
    McpTrigger,
    McpTriggerDetail,
    McpTriggerState
  } from '$lib/anda/client/types'
  import Modal from '$lib/anda/Modal.svelte'
  import { badgeClass, buttonClass } from '$lib/anda/ui'
  import { getMessage } from '$lib/i18n'
  import { errorToMessage } from '$lib/service-worker/settings'
  import { cn } from '$lib/utils'
  import {
    AlertTriangle,
    ChevronDown,
    ChevronRight,
    LoaderCircle,
    Pause,
    Play,
    Plus,
    RefreshCw,
    Trash2,
    Zap
  } from '@lucide/svelte'
  import { onMount, untrack } from 'svelte'

  import McpTriggerDialog from './McpTriggerDialog.svelte'

  let { server }: { server: McpServerView } = $props()

  const mcp = useAndaClient().mcp
  const POLL_MS = 5000

  let view = $state<McpEventsView | null>(null)
  let triggers = $state<McpTrigger[]>([])
  let loading = $state(false)
  let busy = $state('')
  let error = $state('')
  let notice = $state('')
  let creating = $state<McpEventDefinition | null>(null)
  let createOpen = $state(false)
  let deleting = $state<McpTrigger | null>(null)
  let deleteOpen = $state(false)
  let expanded = $state<Record<number, McpTriggerDetail | 'loading'>>({})
  let loadedFor = ''

  const stateLabels: Record<McpTriggerState, string> = {
    starting: getMessage('mcpTriggerStateStarting'),
    active: getMessage('mcpTriggerStateActive'),
    retrying: getMessage('mcpTriggerStateRetrying'),
    paused: getMessage('mcpTriggerStatePaused'),
    waiting: getMessage('mcpTriggerStateWaiting'),
    needs_auth: getMessage('mcpTriggerStateNeedsAuth'),
    needs_ingress: getMessage('mcpTriggerStateNeedsIngress'),
    ended: getMessage('mcpTriggerStateEnded')
  }

  // Another server: read its events again.
  $effect(() => {
    const id = server.id
    if (id !== loadedFor) {
      loadedFor = id
      untrack(() => void load())
    }
  })

  onMount(() => {
    const changed = () => void refreshTriggers()
    mcp.addEventListener('mcp-changed', changed)
    const timer = window.setInterval(() => {
      if (document.visibilityState === 'visible' && !busy) void refreshTriggers()
    }, POLL_MS)
    return () => {
      mcp.removeEventListener('mcp-changed', changed)
      window.clearInterval(timer)
    }
  })

  async function load() {
    const id = server.id
    loading = true
    error = ''
    notice = ''
    view = null
    expanded = {}
    try {
      const next = await mcp.events(id)
      if (id !== server.id) return
      view = next
      triggers = next.triggers
    } catch (err) {
      if (id === server.id) error = errorToMessage(err)
    } finally {
      if (id === server.id) loading = false
    }
  }

  async function refreshTriggers() {
    const id = server.id
    const next = await mcp.triggers(id).catch(() => null)
    if (next && id === server.id) triggers = next
  }

  async function run(action: string, work: () => Promise<string>) {
    if (busy) return
    busy = action
    error = ''
    notice = ''
    try {
      notice = await work()
      await refreshTriggers()
    } catch (err) {
      error = errorToMessage(err)
    } finally {
      busy = ''
    }
  }

  function openCreate(event: McpEventDefinition) {
    creating = event
    createOpen = true
  }

  function created(trigger: McpTriggerDetail) {
    notice = getMessage('mcpTriggerCreated', trigger.name)
    void refreshTriggers()
  }

  function setEnabled(trigger: McpTrigger, enabled: boolean) {
    void run(`enable-${trigger.id}`, async () => {
      await mcp.applyTrigger({ op: 'set_enabled', id: trigger.id, enabled })
      return getMessage(enabled ? 'mcpTriggerResumed' : 'mcpTriggerPausedNotice', trigger.name)
    })
  }

  function confirmDelete(trigger: McpTrigger) {
    deleting = trigger
    deleteOpen = true
  }

  function remove() {
    const trigger = deleting
    deleteOpen = false
    if (!trigger) return
    void run(`delete-${trigger.id}`, async () => {
      await mcp.applyTrigger({ op: 'delete', id: trigger.id })
      return getMessage('mcpTriggerDeleted', trigger.name)
    })
  }

  async function toggle(trigger: McpTrigger) {
    if (expanded[trigger.id]) {
      const { [trigger.id]: _, ...rest } = expanded
      expanded = rest
      return
    }
    expanded = { ...expanded, [trigger.id]: 'loading' }
    try {
      const detail = await mcp.trigger(trigger.id)
      expanded = { ...expanded, [trigger.id]: detail }
    } catch (err) {
      const { [trigger.id]: _, ...rest } = expanded
      expanded = rest
      error = errorToMessage(err)
    }
  }

  function stateTone(state: McpTriggerState): string {
    switch (state) {
      case 'active':
        return 'border-emerald-500/50 text-emerald-700 dark:text-emerald-300'
      case 'starting':
      case 'paused':
        return 'text-muted-foreground'
      case 'ended':
        return 'border-destructive/50 text-destructive'
      default:
        return 'border-amber-500/50 text-amber-700 dark:text-amber-300'
    }
  }

  function timeLabel(ms?: number | null): string {
    return ms ? new Date(ms).toLocaleString() : ''
  }

  function canCreate(event: McpEventDefinition): boolean {
    return !event.webhook_only || view?.ingress.available === true
  }
</script>

<div class="grid gap-5 p-4">
  <div class="flex items-start gap-2">
    <p class="flex-1 text-xs text-muted-foreground">{getMessage('mcpEventsHelp')}</p>
    <button
      type="button"
      class={buttonClass('ghost', 'icon-sm')}
      aria-label={getMessage('mcpEventsReload')}
      title={getMessage('mcpEventsReload')}
      disabled={loading}
      onclick={() => void load()}
    >
      <RefreshCw class={cn('size-3.5', loading && 'animate-spin')} />
    </button>
  </div>

  {#if error}
    <p
      class="rounded-md border border-destructive/30 bg-destructive/5 px-3 py-2 text-sm break-words whitespace-pre-wrap text-destructive"
    >
      {error}
    </p>
  {/if}
  {#if notice}
    <p
      class="rounded-md bg-emerald-50 px-3 py-2 text-sm text-emerald-800 dark:bg-emerald-950/30 dark:text-emerald-200"
    >
      {notice}
    </p>
  {/if}

  {#if loading}
    <div class="grid h-32 place-items-center text-muted-foreground">
      <LoaderCircle class="size-5 animate-spin" />
    </div>
  {:else if view}
    {#if view.error}
      <p class="text-sm text-muted-foreground">{getMessage('mcpEventsError', view.error)}</p>
    {:else if !view.supported}
      <p class="rounded-md border border-dashed p-6 text-center text-sm text-muted-foreground">
        {getMessage('mcpEventsUnsupported')}
      </p>
    {:else if !view.events.length}
      <p class="rounded-md border border-dashed p-6 text-center text-sm text-muted-foreground">
        {getMessage('mcpEventsNone')}
      </p>
    {:else}
      <section class="grid gap-2">
        {#each view.events as event (event.name)}
          <div class="grid gap-1.5 rounded-md border p-3">
            <div class="flex min-w-0 flex-wrap items-center gap-2">
              <Zap class="size-3.5 shrink-0 text-muted-foreground" />
              <span class="font-mono text-sm font-semibold break-all">{event.name}</span>
              {#each event.delivery as mode (mode)}
                <span class={badgeClass('outline', 'h-4 px-1.5 text-[10px]')}>{mode}</span>
              {/each}
              <button
                type="button"
                class={buttonClass('outline', 'sm', 'ms-auto h-7')}
                disabled={!canCreate(event) || !!busy}
                onclick={() => openCreate(event)}
              >
                <Plus class="size-3.5" />
                {getMessage('mcpEventsCreate')}
              </button>
            </div>
            {#if event.description}
              <p class="line-clamp-3 text-xs text-muted-foreground">{event.description}</p>
            {/if}
            {#if event.webhook_only}
              <p class="flex gap-1.5 text-[11px] text-amber-700 dark:text-amber-300">
                <AlertTriangle class="mt-0.5 size-3 shrink-0" />
                {view.ingress.available
                  ? getMessage('mcpEventsWebhookOnly', view.ingress.server_id)
                  : getMessage('mcpEventsNoIngress')}
              </p>
            {/if}
          </div>
        {/each}
      </section>
    {/if}
  {/if}

  {#if !loading}
    <section class="grid gap-2">
      <h2 class="text-sm font-semibold">{getMessage('mcpTriggersTitle')}</h2>
      {#if !triggers.length}
        <p class="text-xs text-muted-foreground">{getMessage('mcpTriggersNone')}</p>
      {/if}
      {#each triggers as trigger (trigger.id)}
        {@const detail = expanded[trigger.id]}
        <div class="grid gap-1.5 rounded-md border p-3">
          <div class="flex min-w-0 flex-wrap items-center gap-2">
            <button
              type="button"
              class="flex min-w-0 items-center gap-1 text-start"
              aria-expanded={!!detail}
              onclick={() => void toggle(trigger)}
            >
              {#if detail}
                <ChevronDown class="size-3.5 shrink-0" />
              {:else}
                <ChevronRight class="size-3.5 shrink-0" />
              {/if}
              <span class="truncate text-sm font-semibold">{trigger.name}</span>
            </button>
            <span
              class={badgeClass('outline', cn('h-4 px-1.5 text-[10px]', stateTone(trigger.state)))}
            >
              {stateLabels[trigger.state] || trigger.state}
            </span>
            {#if trigger.created_by === 'model'}
              <span class={badgeClass('outline', 'h-4 px-1.5 text-[10px] text-muted-foreground')}>
                {getMessage('mcpTriggerByModel')}
              </span>
            {/if}
            <div class="ms-auto flex gap-1">
              {#if trigger.enabled}
                <button
                  type="button"
                  class={buttonClass('ghost', 'icon-sm')}
                  aria-label={getMessage('mcpTriggerPause', trigger.name)}
                  title={getMessage('mcpTriggerPause', trigger.name)}
                  disabled={!!busy}
                  onclick={() => setEnabled(trigger, false)}
                >
                  <Pause class="size-3.5" />
                </button>
              {:else}
                <button
                  type="button"
                  class={buttonClass('ghost', 'icon-sm')}
                  aria-label={getMessage('mcpTriggerResume', trigger.name)}
                  title={getMessage('mcpTriggerResume', trigger.name)}
                  disabled={!!busy}
                  onclick={() => setEnabled(trigger, true)}
                >
                  <Play class="size-3.5" />
                </button>
              {/if}
              <button
                type="button"
                class={buttonClass('ghost', 'icon-sm', 'text-destructive')}
                aria-label={getMessage('mcpTriggerDelete', trigger.name)}
                title={getMessage('mcpTriggerDelete', trigger.name)}
                disabled={!!busy}
                onclick={() => confirmDelete(trigger)}
              >
                <Trash2 class="size-3.5" />
              </button>
            </div>
          </div>
          <div class="flex flex-wrap gap-x-3 gap-y-0.5 text-[11px] text-muted-foreground">
            <span class="font-mono">{trigger.event}</span>
            <span>
              {getMessage('mcpTriggerStats', [
                String(trigger.events_received),
                String(trigger.runs)
              ])}
            </span>
            {#if trigger.pending}
              <span>{getMessage('mcpTriggerPending', String(trigger.pending))}</span>
            {/if}
            {#if trigger.last_run_at}
              <span>{getMessage('mcpTriggerLastRun', timeLabel(trigger.last_run_at))}</span>
            {/if}
            {#if trigger.mode}
              <span>{trigger.mode}</span>
            {/if}
          </div>
          {#if trigger.last_error}
            <p class="text-xs break-words whitespace-pre-wrap text-destructive">
              {trigger.last_error}
            </p>
          {/if}
          {#if trigger.missed_events_at}
            <p class="flex gap-1.5 text-[11px] text-amber-700 dark:text-amber-300">
              <AlertTriangle class="mt-0.5 size-3 shrink-0" />
              {getMessage('mcpTriggerMissed', timeLabel(trigger.missed_events_at))}
            </p>
          {/if}
          {#if detail === 'loading'}
            <LoaderCircle class="size-4 animate-spin text-muted-foreground" />
          {:else if detail}
            <div class="grid gap-2 border-t pt-2 text-xs">
              <p class="whitespace-pre-wrap">{detail.instructions}</p>
              {#if Object.keys(detail.arguments || {}).length}
                <code class="rounded bg-muted/50 px-2 py-1 font-mono text-[11px] break-all"
                  >{JSON.stringify(detail.arguments)}</code
                >
              {/if}
              <div class="font-semibold text-muted-foreground">{getMessage('mcpTriggerRuns')}</div>
              {#if !detail.runs_recent.length}
                <p class="text-muted-foreground">{getMessage('mcpTriggerNoRuns')}</p>
              {/if}
              {#each detail.runs_recent as run (run.id)}
                <div class="flex flex-wrap gap-x-3 text-[11px]">
                  <span class="text-muted-foreground">{timeLabel(run.started_at)}</span>
                  {#if run.error}
                    <span class="break-words text-destructive">
                      {getMessage('mcpTriggerRunFailed', [String(run.events), run.error])}
                    </span>
                  {:else}
                    <span>{getMessage('mcpTriggerRunOk', String(run.events))}</span>
                  {/if}
                </div>
              {/each}
            </div>
          {/if}
        </div>
      {/each}
    </section>
  {/if}
</div>

<McpTriggerDialog
  bind:open={createOpen}
  serverId={server.id}
  event={creating}
  onCreated={created}
/>

{#snippet deleteActions()}
  <button type="button" class={buttonClass('outline', 'sm')} onclick={() => (deleteOpen = false)}>
    {getMessage('cancel')}
  </button>
  <button type="button" class={buttonClass('destructive', 'sm')} onclick={remove}>
    {getMessage('mcpTriggerDeleteAction')}
  </button>
{/snippet}

<Modal
  bind:open={deleteOpen}
  alert
  title={getMessage('mcpTriggerDeleteTitle', deleting?.name || '')}
  footer={deleteActions}
>
  <p class="text-sm">{getMessage('mcpTriggerDeleteConfirm')}</p>
</Modal>
