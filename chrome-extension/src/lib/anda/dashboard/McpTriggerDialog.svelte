<script lang="ts">
  /**
   * Creates an automation on one of a server's events, or changes one: the
   * agent runs the owner's instructions when the event arrives, and its reply
   * lands in a conversation of the owner's. The subscription arguments come
   * from the event's schema as a form when it is flat, and as JSON otherwise.
   * It also names the server's tools the automation would be refused, since
   * nobody is there to approve them.
   */
  import { useAndaClient } from '$lib/anda/client/context'
  import { automationBlockedTools, eventArgumentFields, eventArguments } from '$lib/anda/client/mcp'
  import type {
    Json,
    McpEventDefinition,
    McpToolView,
    McpTrigger,
    McpTriggerDelivery,
    McpTriggerDetail
  } from '$lib/anda/client/types'
  import DropdownMenu from '$lib/anda/DropdownMenu.svelte'
  import Modal from '$lib/anda/Modal.svelte'
  import { buttonClass, inputClass, textareaClass } from '$lib/anda/ui'
  import { getMessage } from '$lib/i18n'
  import { errorToMessage } from '$lib/service-worker/settings'
  import { ShieldCheck, LoaderCircle } from '@lucide/svelte'
  import { untrack } from 'svelte'

  let {
    open = $bindable(false),
    serverId,
    event,
    editing = null,
    onSaved
  }: {
    open?: boolean
    serverId: string
    /** The event type, when the server offers it now. */
    event: McpEventDefinition | null
    /** The automation to change; a new one is made without it. */
    editing?: McpTrigger | null
    onSaved: (trigger: McpTriggerDetail) => void
  } = $props()

  const mcp = useAndaClient().mcp
  const MAX_RUNS = 120
  const BLOCKED_SHOWN = 12

  let name = $state('')
  let values = $state<Record<string, string>>({})
  let json = $state('{}')
  let instructions = $state('')
  let batchWindow = $state('')
  let maxRuns = $state('')
  let delivery = $state<McpTriggerDelivery>('auto')
  let blocked = $state<McpToolView[]>([])
  let busy = $state(false)
  let error = $state('')

  const eventName = $derived(editing?.event ?? event?.name ?? '')
  const fields = $derived(event ? eventArgumentFields(event.input_schema) : null)
  const booleanItems = [
    { value: '', label: getMessage('mcpTriggerUnset') },
    { value: 'true', label: 'true' },
    { value: 'false', label: 'false' }
  ]
  const deliveryItems = $derived([
    { value: 'auto' as const, label: getMessage('mcpTriggerDeliveryAuto') },
    ...(['push', 'poll', 'webhook'] as const)
      .filter((mode) => !event || event.delivery.includes(mode) || editing?.delivery === mode)
      .map((mode) => ({ value: mode, label: mode }))
  ])

  // A fresh form each time the dialog opens, filled from the automation
  // being changed.
  $effect(() => {
    if (open) {
      untrack(() => {
        const current = editing
        const args = current?.arguments ?? {}
        name = current?.name ?? ''
        values = Object.fromEntries(
          (fields ?? []).map((field) => {
            const value = args[field.name]
            return [field.name, value === undefined || value === null ? '' : String(value)]
          })
        )
        json = JSON.stringify(args, null, 2)
        instructions = current?.instructions ?? ''
        batchWindow = current ? String(current.batch_window_secs) : ''
        maxRuns = current ? String(current.max_runs_per_hour) : ''
        delivery = current?.delivery ?? 'auto'
        error = ''
        blocked = []
        void loadBlocked()
      })
    }
  })

  /** The server's tools an automation would be refused, from its detail. */
  async function loadBlocked() {
    const id = serverId
    try {
      const detail = await mcp.get(id)
      if (id === serverId) blocked = automationBlockedTools(detail, detail.tools)
    } catch {
      blocked = []
    }
  }

  function argumentsOf(): Record<string, Json> {
    if (fields) {
      try {
        return eventArguments(fields, values)
      } catch (err) {
        throw new Error(getMessage('mcpTriggerArgumentsInvalid', (err as Error).message))
      }
    }
    let parsed: unknown
    try {
      parsed = JSON.parse(json.trim() || '{}')
    } catch {
      throw new Error(getMessage('mcpTriggerArgumentsJsonInvalid'))
    }
    if (!parsed || typeof parsed !== 'object' || Array.isArray(parsed)) {
      throw new Error(getMessage('mcpTriggerArgumentsJsonInvalid'))
    }
    return parsed as Record<string, Json>
  }

  /** A whole number field: `undefined` when left empty, `null` when invalid. */
  function wholeNumber(text: string): number | undefined | null {
    const value = text.trim()
    if (!value) return undefined
    return /^\d+$/.test(value) ? Number(value) : null
  }

  async function save() {
    if (!eventName || busy) return
    error = ''
    let args: Record<string, Json>
    try {
      args = argumentsOf()
    } catch (err) {
      error = (err as Error).message
      return
    }
    const seconds = wholeNumber(batchWindow)
    const runs = wholeNumber(maxRuns)
    if (seconds === null || runs === null) {
      error = getMessage('mcpOptionsNumbers')
      return
    }
    if (runs !== undefined && (runs < 1 || runs > MAX_RUNS)) {
      error = getMessage('mcpTriggerMaxRunsRange', String(MAX_RUNS))
      return
    }
    const settings = {
      arguments: args,
      instructions: instructions.trim(),
      delivery,
      ...(name.trim() ? { name: name.trim() } : {}),
      ...(seconds !== undefined ? { batch_window_secs: seconds } : {}),
      ...(runs !== undefined ? { max_runs_per_hour: runs } : {})
    }
    busy = true
    try {
      const result = editing
        ? await mcp.applyTrigger({ op: 'update', id: editing.id, changes: settings })
        : await mcp.applyTrigger({
            op: 'create',
            trigger: { server_id: serverId, event: eventName, ...settings }
          })
      open = false
      onSaved(result as McpTriggerDetail)
    } catch (err) {
      error = errorToMessage(err)
    } finally {
      busy = false
    }
  }
</script>

{#snippet actions()}
  <button type="button" class={buttonClass('outline', 'sm')} onclick={() => (open = false)}>
    {getMessage('cancel')}
  </button>
  <button
    type="button"
    class={buttonClass('default', 'sm')}
    disabled={busy || !instructions.trim()}
    onclick={save}
  >
    {#if busy}
      <LoaderCircle class="size-3.5 animate-spin" />
    {/if}
    {getMessage(editing ? 'mcpTriggerSave' : 'mcpTriggerCreate')}
  </button>
{/snippet}

<Modal
  bind:open
  title={getMessage(editing ? 'mcpTriggerEditTitle' : 'mcpTriggerDialogTitle')}
  description={eventName ? getMessage('mcpTriggerDialogDescription', [serverId, eventName]) : ''}
  contentClass="sm:max-w-xl"
  footer={actions}
>
  {#if eventName}
    <div class="grid gap-4">
      {#if event?.description}
        <p class="text-xs whitespace-pre-wrap text-muted-foreground">{event.description}</p>
      {/if}

      <label class="grid gap-1 text-xs font-medium">
        {getMessage('mcpTriggerName')}
        <input
          class={inputClass('h-8 text-sm')}
          placeholder={`${eventName} on ${serverId}`}
          bind:value={name}
        />
      </label>

      <div class="grid gap-2">
        <div class="text-xs font-medium">{getMessage('mcpTriggerArguments')}</div>
        {#if fields}
          {#if !fields.length}
            <p class="text-xs text-muted-foreground">{getMessage('mcpTriggerNoArguments')}</p>
          {/if}
          {#each fields as field (field.name)}
            <div class="grid gap-1 text-xs">
              <span class="font-mono">
                {field.name}{field.required ? ' *' : ''}
              </span>
              {#if field.options}
                <DropdownMenu
                  class="h-8 text-xs"
                  items={[
                    { value: '', label: getMessage('mcpTriggerUnset') },
                    ...field.options.map((option) => ({ value: option, label: option }))
                  ]}
                  bind:value={values[field.name]}
                  ariaLabel={field.name}
                />
              {:else if field.type === 'boolean'}
                <DropdownMenu
                  class="h-8 text-xs"
                  items={booleanItems}
                  bind:value={values[field.name]}
                  ariaLabel={field.name}
                />
              {:else}
                <input
                  class={inputClass('h-8 text-xs')}
                  inputmode={field.type === 'string' ? 'text' : 'decimal'}
                  aria-label={field.name}
                  bind:value={values[field.name]}
                />
              {/if}
              {#if field.description}
                <span class="text-[11px] text-muted-foreground">{field.description}</span>
              {/if}
            </div>
          {/each}
        {:else}
          <textarea
            class={textareaClass('min-h-20 font-mono text-xs')}
            aria-label={getMessage('mcpTriggerArgumentsJson')}
            bind:value={json}></textarea>
          <span class="text-[11px] text-muted-foreground">
            {getMessage('mcpTriggerArgumentsJson')}
          </span>
        {/if}
      </div>

      <label class="grid gap-1 text-xs font-medium">
        {getMessage('mcpTriggerInstructions')}
        <textarea
          class={textareaClass('min-h-24 text-sm')}
          placeholder={getMessage('mcpTriggerInstructionsPlaceholder')}
          bind:value={instructions}></textarea>
      </label>

      <div class="grid gap-3 sm:grid-cols-3">
        <label class="grid content-start gap-1 text-xs font-medium">
          {getMessage('mcpTriggerBatchWindow')}
          <input
            class={inputClass('h-8 text-xs tabular-nums')}
            inputmode="numeric"
            placeholder="30"
            bind:value={batchWindow}
          />
          <span class="text-[11px] font-normal text-muted-foreground">
            {getMessage('mcpTriggerBatchWindowHelp')}
          </span>
        </label>
        <label class="grid content-start gap-1 text-xs font-medium">
          {getMessage('mcpTriggerMaxRuns')}
          <input
            class={inputClass('h-8 text-xs tabular-nums')}
            inputmode="numeric"
            placeholder="12"
            bind:value={maxRuns}
          />
          <span class="text-[11px] font-normal text-muted-foreground">
            {getMessage('mcpTriggerMaxRunsHelp')}
          </span>
        </label>
        <div class="grid content-start gap-1 text-xs font-medium">
          {getMessage('mcpTriggerDelivery')}
          <DropdownMenu
            class="h-8 text-xs"
            items={deliveryItems}
            bind:value={delivery}
            ariaLabel={getMessage('mcpTriggerDelivery')}
          />
        </div>
      </div>

      <div
        class="grid gap-1.5 rounded-md border border-amber-500/30 bg-amber-50 px-3 py-2 text-xs text-amber-900 dark:bg-amber-950/30 dark:text-amber-200"
      >
        <div class="flex gap-2">
          <ShieldCheck class="mt-0.5 size-3.5 shrink-0" />
          <span>{getMessage('mcpTriggerUnattended')}</span>
        </div>
        {#if blocked.length}
          <p class="ps-5.5">
            {getMessage('mcpTriggerBlockedTools', String(blocked.length))}
            <span class="font-mono break-all"
              >{blocked
                .slice(0, BLOCKED_SHOWN)
                .map((tool) => tool.remote_name)
                .join(', ')}{blocked.length > BLOCKED_SHOWN ? ', …' : ''}</span
            >
          </p>
          <p class="ps-5.5 text-[11px] opacity-80">{getMessage('mcpTriggerBlockedToolsHint')}</p>
        {/if}
      </div>

      {#if error}
        <p class="text-xs break-words whitespace-pre-wrap text-destructive">{error}</p>
      {/if}
    </div>
  {/if}
</Modal>
