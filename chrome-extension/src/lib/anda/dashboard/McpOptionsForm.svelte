<script lang="ts">
  /**
   * A server's advanced settings: when its tools are discovered, how the
   * protocol is negotiated, which tools may run at once, its timeouts and
   * result limit, a local server's environment, and long-running tasks. A
   * field left empty takes the default, shown as its placeholder. Saving
   * replaces them all, and the daemon reconnects the server when its
   * connection changed.
   */
  import type { McpServerOptions, McpServerView } from '$lib/anda/client/types'
  import DropdownMenu from '$lib/anda/DropdownMenu.svelte'
  import { buttonClass, inputClass } from '$lib/anda/ui'
  import { getMessage } from '$lib/i18n'
  import { LoaderCircle } from '@lucide/svelte'

  let {
    server,
    busy = false,
    saving = false,
    onSave
  }: {
    server: McpServerView
    busy?: boolean
    saving?: boolean
    onSave: (options: McpServerOptions) => void
  } = $props()

  // The engine's defaults, shown where a field is left empty.
  const DEFAULTS = { setup: 90, list: 30, request: 180, call: 600, output: 32768, wait: 300 }

  type Draft = {
    startup: string
    lifecycle: string
    concurrency: string
    setup: string
    list: string
    request: string
    call: string
    output: string
    env: string
    tasks: string
    wait: string
  }

  let draft = $state<Draft>(fromOptions(undefined))
  let loadedFor = ''

  // A new server, or settings saved from elsewhere, replace the draft.
  $effect(() => {
    const key = `${server.id}\n${JSON.stringify(server.options ?? {})}`
    if (key !== loadedFor) {
      loadedFor = key
      draft = fromOptions(server.options)
    }
  })

  const startupItems = [
    { value: '', label: getMessage('mcpOptionStartupBackground') },
    { value: 'eager', label: getMessage('mcpOptionStartupEager') }
  ]
  const lifecycleItems = [
    { value: '', label: getMessage('mcpOptionLifecycleAuto') },
    { value: 'discover', label: getMessage('mcpOptionLifecycleDiscover') },
    { value: 'initialize', label: getMessage('mcpOptionLifecycleInitialize') }
  ]
  const concurrencyItems = [
    { value: '', label: getMessage('mcpOptionConcurrencySerial') },
    { value: 'read_only_parallel', label: getMessage('mcpOptionConcurrencyReadOnly') },
    { value: 'parallel', label: getMessage('mcpOptionConcurrencyParallel') }
  ]
  const envItems = [
    { value: '', label: getMessage('mcpOptionEnvInherit') },
    { value: 'off', label: getMessage('mcpOptionEnvIsolate') }
  ]
  const timeoutFields = [
    { key: 'setup', label: getMessage('mcpOptionSetup') },
    { key: 'list', label: getMessage('mcpOptionList') },
    { key: 'request', label: getMessage('mcpOptionRequest') },
    { key: 'call', label: getMessage('mcpOptionCall') }
  ] as const
  const taskItems = [
    { value: '', label: getMessage('mcpOptionTasksOff') },
    { value: 'on', label: getMessage('mcpOptionTasksOn') }
  ]

  const next = $derived(toOptions(draft))
  const changed = $derived(JSON.stringify(next) !== JSON.stringify(normalized(server.options)))
  const invalid = $derived(
    [draft.setup, draft.list, draft.request, draft.call, draft.output, draft.wait].some(
      (value) => value.trim() !== '' && !/^\d+$/.test(value.trim())
    )
  )

  function fromOptions(options: McpServerOptions | undefined): Draft {
    const number = (value: number | undefined) => (value === undefined ? '' : String(value))
    return {
      startup: options?.startup === 'eager' ? 'eager' : '',
      lifecycle: options?.lifecycle && options.lifecycle !== 'auto' ? options.lifecycle : '',
      concurrency:
        options?.concurrency && options.concurrency !== 'serial' ? options.concurrency : '',
      setup: number(options?.timeouts?.setup_secs),
      list: number(options?.timeouts?.list_secs),
      request: number(options?.timeouts?.request_secs),
      call: number(options?.timeouts?.call_secs),
      output: number(options?.limits?.output_text_bytes),
      env: options?.inherit_env === false ? 'off' : '',
      tasks: options?.tasks ? 'on' : '',
      wait: number(options?.tasks?.max_wait_secs)
    }
  }

  function toOptions(value: Draft): McpServerOptions {
    const number = (text: string) => (/^\d+$/.test(text.trim()) ? Number(text.trim()) : undefined)
    const options: McpServerOptions = {}
    if (value.startup) options.startup = 'eager'
    if (value.lifecycle) options.lifecycle = value.lifecycle as McpServerOptions['lifecycle']
    if (value.concurrency)
      options.concurrency = value.concurrency as McpServerOptions['concurrency']
    const timeouts = {
      setup_secs: number(value.setup),
      list_secs: number(value.list),
      request_secs: number(value.request),
      call_secs: number(value.call),
      // Not on the form: kept as it is.
      elicitation_secs: server.options?.timeouts?.elicitation_secs
    }
    const set = Object.fromEntries(
      Object.entries(timeouts).filter(([, secs]) => secs !== undefined)
    )
    if (Object.keys(set).length) options.timeouts = set
    const output = number(value.output)
    if (output !== undefined) options.limits = { output_text_bytes: output }
    if (server.transport === 'stdio' && value.env === 'off') options.inherit_env = false
    if (value.tasks) {
      const wait = number(value.wait)
      options.tasks = wait === undefined ? {} : { max_wait_secs: wait }
    }
    return options
  }

  /** Settings as the form would write them, to tell whether the draft changed. */
  function normalized(options: McpServerOptions | undefined): McpServerOptions {
    return toOptions(fromOptions(options))
  }
</script>

<section class="grid gap-3 rounded-md border p-3">
  <div class="grid gap-0.5">
    <h2 class="text-sm font-semibold">{getMessage('mcpOptionsTitle')}</h2>
    <p class="text-xs text-muted-foreground">{getMessage('mcpOptionsHelp')}</p>
  </div>
  <div class="grid gap-3 sm:grid-cols-3">
    <div class="grid gap-1 text-xs font-medium">
      {getMessage('mcpOptionStartup')}
      <DropdownMenu
        class="h-8 text-xs"
        items={startupItems}
        bind:value={draft.startup}
        ariaLabel={getMessage('mcpOptionStartup')}
      />
    </div>
    <div class="grid gap-1 text-xs font-medium">
      {getMessage('mcpOptionLifecycle')}
      <DropdownMenu
        class="h-8 text-xs"
        items={lifecycleItems}
        bind:value={draft.lifecycle}
        ariaLabel={getMessage('mcpOptionLifecycle')}
      />
    </div>
    <div class="grid gap-1 text-xs font-medium">
      {getMessage('mcpOptionConcurrency')}
      <DropdownMenu
        class="h-8 text-xs"
        items={concurrencyItems}
        bind:value={draft.concurrency}
        ariaLabel={getMessage('mcpOptionConcurrency')}
      />
    </div>
  </div>

  <div class="grid gap-1">
    <div class="text-xs font-medium">{getMessage('mcpOptionTimeouts')}</div>
    <div class="grid gap-2 sm:grid-cols-4">
      {#each timeoutFields as field (field.key)}
        <label class="grid gap-1 text-[11px] text-muted-foreground">
          {field.label}
          <input
            class={inputClass('h-8 text-xs tabular-nums')}
            inputmode="numeric"
            placeholder={String(DEFAULTS[field.key])}
            bind:value={draft[field.key]}
          />
        </label>
      {/each}
    </div>
  </div>

  <div class="grid gap-3 sm:grid-cols-3">
    <label class="grid gap-1 text-xs font-medium">
      {getMessage('mcpOptionOutput')}
      <input
        class={inputClass('h-8 text-xs tabular-nums')}
        inputmode="numeric"
        placeholder={String(DEFAULTS.output)}
        bind:value={draft.output}
      />
    </label>
    {#if server.transport === 'stdio'}
      <div class="grid gap-1 text-xs font-medium">
        {getMessage('mcpOptionEnv')}
        <DropdownMenu
          class="h-8 text-xs"
          items={envItems}
          bind:value={draft.env}
          ariaLabel={getMessage('mcpOptionEnv')}
        />
      </div>
    {/if}
    <div class="grid gap-1 text-xs font-medium">
      {getMessage('mcpOptionTasks')}
      <DropdownMenu
        class="h-8 text-xs"
        items={taskItems}
        bind:value={draft.tasks}
        ariaLabel={getMessage('mcpOptionTasks')}
      />
    </div>
    {#if draft.tasks}
      <label class="grid gap-1 text-xs font-medium">
        {getMessage('mcpOptionTasksWait')}
        <input
          class={inputClass('h-8 text-xs tabular-nums')}
          inputmode="numeric"
          placeholder={String(DEFAULTS.wait)}
          bind:value={draft.wait}
        />
      </label>
    {/if}
  </div>

  {#if invalid}
    <p class="text-xs text-destructive">{getMessage('mcpOptionsNumbers')}</p>
  {/if}
  <div class="flex flex-wrap justify-end gap-2">
    <button
      type="button"
      class={buttonClass('outline', 'sm')}
      disabled={busy || JSON.stringify(next) === '{}'}
      onclick={() => (draft = fromOptions(undefined))}
    >
      {getMessage('mcpOptionsDefaults')}
    </button>
    <button
      type="button"
      class={buttonClass('default', 'sm')}
      disabled={busy || !changed || invalid}
      onclick={() => onSave(next)}
    >
      {#if saving}
        <LoaderCircle class="size-3.5 animate-spin" />
      {/if}
      {getMessage('mcpOptionsSave')}
    </button>
  </div>
</section>
