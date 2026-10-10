<script lang="ts">
  /**
   * Creates an automation on one of a server's events: the agent runs the
   * owner's instructions when the event arrives, and its reply lands in a
   * conversation of the owner's. The subscription arguments come from the
   * event's schema as a form when it is flat, and as JSON otherwise.
   */
  import { useAndaClient } from '$lib/anda/client/context'
  import { eventArgumentFields, eventArguments } from '$lib/anda/client/mcp'
  import type { Json, McpEventDefinition, McpTriggerDetail } from '$lib/anda/client/types'
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
    onCreated
  }: {
    open?: boolean
    serverId: string
    event: McpEventDefinition | null
    onCreated: (trigger: McpTriggerDetail) => void
  } = $props()

  const mcp = useAndaClient().mcp

  let name = $state('')
  let values = $state<Record<string, string>>({})
  let json = $state('{}')
  let instructions = $state('')
  let batchWindow = $state('')
  let busy = $state(false)
  let error = $state('')

  const fields = $derived(event ? eventArgumentFields(event.input_schema) : null)
  const booleanItems = [
    { value: '', label: getMessage('mcpTriggerUnset') },
    { value: 'true', label: 'true' },
    { value: 'false', label: 'false' }
  ]

  // A fresh form each time the dialog opens.
  $effect(() => {
    if (open) {
      untrack(() => {
        name = ''
        values = Object.fromEntries((fields ?? []).map((field) => [field.name, '']))
        json = '{}'
        instructions = ''
        batchWindow = ''
        error = ''
      })
    }
  })

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

  async function create() {
    if (!event || busy) return
    error = ''
    let args: Record<string, Json>
    try {
      args = argumentsOf()
    } catch (err) {
      error = (err as Error).message
      return
    }
    const seconds = batchWindow.trim()
    if (seconds && !/^\d+$/.test(seconds)) {
      error = getMessage('mcpOptionsNumbers')
      return
    }
    busy = true
    try {
      const result = await mcp.applyTrigger({
        op: 'create',
        trigger: {
          server_id: serverId,
          event: event.name,
          arguments: args,
          instructions: instructions.trim(),
          ...(name.trim() ? { name: name.trim() } : {}),
          ...(seconds ? { batch_window_secs: Number(seconds) } : {})
        }
      })
      open = false
      onCreated(result as McpTriggerDetail)
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
    onclick={create}
  >
    {#if busy}
      <LoaderCircle class="size-3.5 animate-spin" />
    {/if}
    {getMessage('mcpTriggerCreate')}
  </button>
{/snippet}

<Modal
  bind:open
  title={getMessage('mcpTriggerDialogTitle')}
  description={event ? getMessage('mcpTriggerDialogDescription', [serverId, event.name]) : ''}
  contentClass="sm:max-w-xl"
  footer={actions}
>
  {#if event}
    <div class="grid gap-4">
      {#if event.description}
        <p class="text-xs whitespace-pre-wrap text-muted-foreground">{event.description}</p>
      {/if}

      <label class="grid gap-1 text-xs font-medium">
        {getMessage('mcpTriggerName')}
        <input
          class={inputClass('h-8 text-sm')}
          placeholder={`${event.name} on ${serverId}`}
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

      <label class="grid gap-1 text-xs font-medium sm:max-w-56">
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

      <div
        class="flex gap-2 rounded-md border border-amber-500/30 bg-amber-50 px-3 py-2 text-xs text-amber-900 dark:bg-amber-950/30 dark:text-amber-200"
      >
        <ShieldCheck class="mt-0.5 size-3.5 shrink-0" />
        <span>{getMessage('mcpTriggerUnattended')}</span>
      </div>

      {#if error}
        <p class="text-xs break-words whitespace-pre-wrap text-destructive">{error}</p>
      {/if}
    </div>
  {/if}
</Modal>
