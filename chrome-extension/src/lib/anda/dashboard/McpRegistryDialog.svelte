<script lang="ts">
  /**
   * Browses the official MCP Registry and installs a server from it. The
   * daemon makes the search, through its own proxy settings. Installing
   * builds an mcp.json entry from the server's `server.json`: a remote
   * endpoint by default, since it runs nothing on this computer, or one of
   * its packages. What a secret field holds goes to the secret store, and a
   * local server runs without the daemon's whole environment. The Registry
   * proves only that a server was published, which the page says.
   */
  import { useAndaClient } from '$lib/anda/client/context'
  import {
    defaultRegistryChoice,
    registryChoices,
    registryEntry,
    registryServerId,
    type RegistryChoice
  } from '$lib/anda/client/mcp'
  import { openExternalUrl } from '$lib/anda/client/platform'
  import type { McpEntry, McpRegistryServer, McpTestReport } from '$lib/anda/client/types'
  import DropdownMenu from '$lib/anda/DropdownMenu.svelte'
  import Modal from '$lib/anda/Modal.svelte'
  import { badgeClass, buttonClass, inputClass } from '$lib/anda/ui'
  import { getMessage } from '$lib/i18n'
  import { errorToMessage } from '$lib/service-worker/settings'
  import { cn } from '$lib/utils'
  import { ArrowLeft, ExternalLink, LoaderCircle, Search, ShieldAlert } from '@lucide/svelte'
  import { untrack } from 'svelte'

  let {
    open = $bindable(false),
    revision = '',
    takenIds,
    secretNames,
    onInstalled
  }: {
    open?: boolean
    revision?: string
    /** Server ids in use. */
    takenIds: ReadonlySet<string>
    /** Secret names in use, which an install does not overwrite. */
    secretNames: ReadonlySet<string>
    /** After the install; `needsAuth` when the server's test asked for a sign-in. */
    onInstalled: (id: string, needsAuth: boolean) => void
  } = $props()

  const mcp = useAndaClient().mcp
  const SEARCH_DELAY_MS = 300

  let query = $state('')
  let servers = $state<McpRegistryServer[]>([])
  let cursor = $state<string | undefined>(undefined)
  let loading = $state(false)
  let error = $state('')
  let selected = $state<McpRegistryServer | null>(null)
  let choiceKey = $state('')
  let id = $state('')
  let values = $state<Record<string, string>>({})
  let test = $state<McpTestReport | 'testing' | null>(null)
  let busy = $state(false)
  let searchTimer = 0
  let searchRequest = 0

  // Lists the Registry each time the dialog opens.
  $effect(() => {
    if (open) untrack(() => reset())
  })

  const choices = $derived(selected ? registryChoices(selected) : [])
  const choice = $derived(choices.find((item) => keyOf(item) === choiceKey))
  const choiceItems = $derived(
    choices.map((item) => ({
      value: keyOf(item),
      label: choiceLabel(item),
      description: item.unsupported ? unsupportedText(item) : undefined
    }))
  )
  const built = $derived.by(() => {
    if (!selected || !choice || choice.unsupported) return null
    return registryEntry(selected, choice, id.trim(), values, secretNames)
  })
  const problem = $derived.by(() => {
    if (!selected) return ''
    if (!choices.some((item) => !item.unsupported)) return getMessage('mcpRegistryNothing')
    if (choice?.unsupported) return unsupportedText(choice)
    if (!id.trim()) return ''
    if (takenIds.has(id.trim())) return getMessage('mcpIdTaken', id.trim())
    return ''
  })

  function reset() {
    selected = null
    query = ''
    error = ''
    void search(false)
  }

  function keyOf(item: RegistryChoice): string {
    return `${item.kind}:${item.index}`
  }

  function choiceLabel(item: RegistryChoice): string {
    if (item.kind === 'remote') {
      let host = item.target
      try {
        host = new URL(item.target).host
      } catch {
        // A templated URL reads as it is.
      }
      return getMessage('mcpRegistryRemoteChoice', host)
    }
    return getMessage('mcpRegistryPackageChoice', [item.type, item.target])
  }

  function unsupportedText(item: RegistryChoice): string {
    switch (item.unsupported) {
      case 'sse':
        return getMessage('mcpRegistryUnsupportedSse')
      case 'transport':
        return getMessage('mcpRegistryUnsupportedTransport')
      default:
        return getMessage('mcpRegistryUnsupportedPackage')
    }
  }

  function onQuery() {
    window.clearTimeout(searchTimer)
    searchTimer = window.setTimeout(() => void search(false), SEARCH_DELAY_MS)
  }

  /** Searches again, or with `more` reads the next page. */
  async function search(more: boolean) {
    const request = ++searchRequest
    loading = true
    error = ''
    try {
      const page = await mcp.registrySearch(query.trim(), more ? cursor : undefined)
      if (request !== searchRequest) return
      servers = more ? [...servers, ...page.servers] : page.servers
      cursor = page.next_cursor
    } catch (err) {
      if (request === searchRequest) error = errorToMessage(err)
    } finally {
      if (request === searchRequest) loading = false
    }
  }

  function pick(server: McpRegistryServer) {
    selected = server
    const first = defaultRegistryChoice(registryChoices(server))
    choiceKey = first ? keyOf(first) : ''
    let next = registryServerId(server.name)
    for (let n = 2; takenIds.has(next); n += 1) next = `${registryServerId(server.name)}-${n}`
    id = next
    values = {}
    test = null
    error = ''
  }

  function entrySummary(entry: McpEntry): string {
    if (typeof entry.url === 'string') return entry.url
    const args = Array.isArray(entry.args) ? entry.args.map(String) : []
    return [
      String(entry.command ?? ''),
      ...args.map((arg) => (/\s|^$/.test(arg) ? JSON.stringify(arg) : arg))
    ].join(' ')
  }

  async function runTest() {
    if (!built?.entry || busy) return
    busy = true
    test = 'testing'
    try {
      test = await mcp.test(built.entry, built.secrets)
    } catch (err) {
      test = { status: 'failed', error: errorToMessage(err), tools: [] }
    } finally {
      busy = false
    }
  }

  async function install() {
    if (!selected || !built || busy) return
    if (built.missing.length) {
      error = getMessage('mcpRegistryMissing', built.missing.join(', '))
      return
    }
    if (!built.entry || problem) return
    busy = true
    error = ''
    try {
      for (const [name, value] of Object.entries(built.secrets)) {
        await mcp.apply({ op: 'set_secret', name, value })
      }
      await mcp.apply(
        {
          op: 'add',
          server: built.entry,
          persist: true,
          source: 'registry',
          source_ref: selected.version ? `${selected.name}@${selected.version}` : selected.name
        },
        revision || undefined
      )
      open = false
      onInstalled(
        built.entry.id,
        test !== null && test !== 'testing' && test.status === 'needs_auth'
      )
    } catch (err) {
      error = errorToMessage(err)
    } finally {
      busy = false
    }
  }
</script>

{#snippet actions()}
  {#if selected}
    <button
      type="button"
      class={cn(buttonClass('ghost', 'sm'), 'me-auto')}
      disabled={busy}
      onclick={() => (selected = null)}
    >
      <ArrowLeft class="size-3.5 rtl:rotate-180" />
      {getMessage('mcpRegistryBack')}
    </button>
    <button
      type="button"
      class={buttonClass('outline', 'sm')}
      disabled={busy || !built?.entry || Boolean(problem)}
      onclick={runTest}
    >
      {#if test === 'testing'}
        <LoaderCircle class="size-3.5 animate-spin" />
      {/if}
      {getMessage('mcpTestConnection')}
    </button>
    <button
      type="button"
      class={buttonClass('default', 'sm')}
      disabled={busy || !built || !id.trim() || Boolean(problem)}
      onclick={install}
    >
      {#if busy && test !== 'testing'}
        <LoaderCircle class="size-3.5 animate-spin" />
      {/if}
      {getMessage('mcpRegistryInstall')}
    </button>
  {:else}
    <button type="button" class={buttonClass('outline', 'sm')} onclick={() => (open = false)}>
      {getMessage('cancel')}
    </button>
  {/if}
{/snippet}

<Modal
  bind:open
  title={selected ? selected.title || selected.name : getMessage('mcpRegistryTitle')}
  description={selected ? selected.name : getMessage('mcpRegistryDescription')}
  contentClass="sm:max-w-2xl"
  footer={actions}
>
  {#if !selected}
    <div class="grid gap-3">
      <div class="relative">
        <Search
          class="pointer-events-none absolute top-1/2 left-2.5 size-3.5 -translate-y-1/2 text-muted-foreground"
        />
        <input
          class={inputClass('h-9 pl-8 text-sm')}
          placeholder={getMessage('mcpRegistrySearch')}
          aria-label={getMessage('mcpRegistrySearch')}
          bind:value={query}
          oninput={onQuery}
        />
      </div>
      {#if error}
        <p class="text-sm text-destructive" role="alert">{error}</p>
      {/if}
      <div class="grid gap-1.5">
        {#each servers as server (`${server.name}@${server.version}`)}
          <button
            type="button"
            class="grid min-w-0 gap-1 rounded-md border px-3 py-2 text-left transition hover:border-primary/30 hover:bg-muted/40"
            onclick={() => pick(server)}
          >
            <span class="flex min-w-0 items-center gap-2">
              <span class="truncate text-sm font-semibold">{server.title || server.name}</span>
              {#if server.remotes?.length}
                <span class={badgeClass('outline', 'h-4 px-1.5 text-[10px]')}>
                  {getMessage('mcpRemote')}
                </span>
              {/if}
              {#if server.packages?.length}
                <span class={badgeClass('outline', 'h-4 px-1.5 text-[10px]')}>
                  {getMessage('mcpLocal')}
                </span>
              {/if}
              {#if server.version}
                <span class="ml-auto shrink-0 text-[11px] text-muted-foreground tabular-nums">
                  {server.version}
                </span>
              {/if}
            </span>
            {#if server.title}
              <span class="truncate font-mono text-[11px] text-muted-foreground">{server.name}</span
              >
            {/if}
            {#if server.description}
              <span class="line-clamp-2 text-xs text-muted-foreground">{server.description}</span>
            {/if}
          </button>
        {:else}
          {#if !loading}
            <p
              class="rounded-md border border-dashed p-6 text-center text-sm text-muted-foreground"
            >
              {getMessage('mcpRegistryEmpty')}
            </p>
          {/if}
        {/each}
      </div>
      {#if loading}
        <div class="grid h-16 place-items-center text-muted-foreground">
          <LoaderCircle class="size-5 animate-spin" />
        </div>
      {:else if cursor}
        <button
          type="button"
          class={buttonClass('outline', 'sm')}
          onclick={() => void search(true)}
        >
          {getMessage('mcpRegistryLoadMore')}
        </button>
      {/if}
    </div>
  {:else}
    <div class="grid gap-3">
      <div
        class="flex items-start gap-2 rounded-md border border-amber-500/40 bg-amber-50 px-3 py-2 text-xs text-amber-900 dark:bg-amber-950/30 dark:text-amber-100"
      >
        <ShieldAlert class="mt-0.5 size-4 shrink-0" />
        <p>{getMessage('mcpRegistryTrust')}</p>
      </div>
      {#if selected.description}
        <p class="text-sm text-muted-foreground">{selected.description}</p>
      {/if}
      <div class="flex flex-wrap items-center gap-3 text-xs text-muted-foreground">
        {#if selected.version}
          <span>{getMessage('mcpRegistryVersion', selected.version)}</span>
        {/if}
        {#if selected.repository?.url}
          <button
            type="button"
            class="inline-flex items-center gap-1 hover:text-foreground"
            onclick={() =>
              selected?.repository?.url && void openExternalUrl(selected.repository.url)}
          >
            <ExternalLink class="size-3" />
            {getMessage('mcpRegistrySource')}
          </button>
        {/if}
        {#if selected.websiteUrl}
          <button
            type="button"
            class="inline-flex items-center gap-1 hover:text-foreground"
            onclick={() => selected?.websiteUrl && void openExternalUrl(selected.websiteUrl)}
          >
            <ExternalLink class="size-3" />
            {getMessage('mcpRegistryWebsite')}
          </button>
        {/if}
      </div>

      {#if choiceItems.length}
        <div class="grid gap-1 text-xs font-medium">
          {getMessage('mcpRegistryHowToRun')}
          <DropdownMenu
            class="h-8 text-xs"
            items={choiceItems}
            bind:value={choiceKey}
            ariaLabel={getMessage('mcpRegistryHowToRun')}
          />
        </div>
      {/if}

      {#if choice && !choice.unsupported}
        {#each choice.fields as field (field.key)}
          <label class="grid gap-1 text-xs font-medium">
            <span class="flex flex-wrap items-center gap-1.5">
              <span class="font-mono">{field.name}</span>
              {#if field.required}
                <span class="text-[10px] text-muted-foreground"
                  >{getMessage('mcpRegistryRequired')}</span
                >
              {/if}
              {#if field.secret}
                <span class={badgeClass('outline', 'h-4 px-1.5 text-[10px]')}>
                  {getMessage('mcpRegistrySecretField')}
                </span>
              {/if}
            </span>
            {#if field.choices?.length}
              <DropdownMenu
                class="h-8 text-xs"
                items={field.choices.map((value) => ({ value, label: value }))}
                bind:value={values[field.key]}
                ariaLabel={field.name}
              />
            {:else}
              <input
                class={inputClass(cn('h-8 text-xs', !field.secret && 'font-mono'))}
                type={field.secret ? 'password' : 'text'}
                autocomplete="off"
                placeholder={field.default || ''}
                bind:value={values[field.key]}
              />
            {/if}
            {#if field.description}
              <span class="font-normal text-muted-foreground">{field.description}</span>
            {/if}
          </label>
        {/each}

        <label class="grid gap-1 text-xs font-medium">
          {getMessage('mcpServerId')}
          <input class={inputClass('h-8 font-mono text-xs')} bind:value={id} />
        </label>

        {#if built?.entry}
          <div class="grid gap-1.5 rounded-md border p-3">
            {#if choice.kind === 'package'}
              <p class="text-[11px] text-muted-foreground">{getMessage('mcpLocalWarning')}</p>
            {/if}
            <code class="rounded bg-muted/50 px-2 py-1 font-mono text-xs break-all"
              >{entrySummary(built.entry)}</code
            >
            {#if test === 'testing'}
              <LoaderCircle class="size-3.5 animate-spin text-muted-foreground" />
            {:else if test}
              <span
                class={cn(
                  'text-xs font-semibold',
                  test.status === 'ready'
                    ? 'text-emerald-700 dark:text-emerald-300'
                    : test.status === 'needs_auth'
                      ? 'text-amber-700 dark:text-amber-300'
                      : 'text-destructive'
                )}
              >
                {test.status === 'ready'
                  ? getMessage('mcpTestReady', String(test.tools.length))
                  : test.status === 'needs_auth'
                    ? getMessage('mcpTestNeedsAuth')
                    : getMessage('mcpTestFailed')}
              </span>
              {#if test.error}
                <p class="text-xs break-words whitespace-pre-wrap text-destructive">{test.error}</p>
              {/if}
            {/if}
          </div>
        {/if}
      {/if}

      {#if problem}
        <p class="text-sm text-destructive">{problem}</p>
      {/if}
      {#if error}
        <p class="text-sm whitespace-pre-wrap text-destructive" role="alert">{error}</p>
      {/if}
    </div>
  {/if}
</Modal>
