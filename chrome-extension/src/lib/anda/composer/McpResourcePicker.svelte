<script lang="ts" module>
  import type { McpResourceAttachment, McpResourceListing } from '$lib/anda/client/types'

  /** Where the picker lists and reads resources: the app's `McpApi`. */
  export interface McpResourceSource {
    resources(id?: string): Promise<McpResourceListing[]>
    resourceDirectory(id: string, uri: string): Promise<McpResourceListing>
    readResource(id: string, uri: string): Promise<McpResourceAttachment[]>
  }
</script>

<script lang="ts">
  /**
   * Attaches a resource of a connected MCP server to the message: the
   * servers' resources, searchable. Picking one reads it through the daemon,
   * and its contents join the message like files the user picked. Names and
   * descriptions come from the servers and are shown as plain text.
   */
  import type { ChatAttachment, McpResource } from '$lib/anda/client/types'
  import { mcpResourceToAttachment } from '$lib/anda/composer/attachments'
  import Modal from '$lib/anda/Modal.svelte'
  import { inputClass } from '$lib/anda/ui'
  import { getMessage } from '$lib/i18n'
  import { errorToMessage } from '$lib/service-worker/settings'
  import { formatFileSize } from '$lib/utils/format'
  import { ArrowLeft, FileText, Folder, LoaderCircle, Search } from '@lucide/svelte'
  import { tick, untrack } from 'svelte'

  let {
    open = $bindable(false),
    source,
    onAttach
  }: {
    open?: boolean
    source: McpResourceSource
    onAttach: (attachments: ChatAttachment[]) => void
  } = $props()

  let listings = $state<McpResourceListing[]>([])
  let parents = $state<{ listings: McpResourceListing[]; query: string; directory: string }[]>([])
  let directory = $state('')
  let generation = 0
  let search: HTMLInputElement
  let loading = $state(false)
  let query = $state('')
  let reading = $state('')
  let error = $state('')

  // A fresh listing each time it opens; `load` writes state it would track.
  $effect(() => {
    if (open) untrack(() => void load())
    return () => {
      generation++
    }
  })

  async function load() {
    const current = generation
    parents = []
    directory = ''
    reading = ''
    listings = []
    loading = true
    error = ''
    query = ''
    try {
      const root = await source.resources()
      if (current === generation) listings = root
    } catch (err) {
      if (current === generation) error = errorToMessage(err)
    } finally {
      if (current === generation) loading = false
    }
  }

  function matches(resource: McpResource, text: string): boolean {
    if (!text) return true
    return [resource.name, resource.title, resource.uri, resource.description].some((value) =>
      value?.toLowerCase().includes(text)
    )
  }

  const visible = $derived.by(() => {
    const text = query.trim().toLowerCase()
    return listings.map((listing) => ({
      ...listing,
      resources: (listing.resources ?? []).filter((resource) => matches(resource, text))
    }))
  })
  const empty = $derived(
    !loading && !error && listings.every((listing) => !listing.resources?.length && !listing.error)
  )
  const noMatch = $derived(
    !empty && visible.every((listing) => !listing.resources.length && !listing.error)
  )

  async function back() {
    const parent = parents.pop()
    if (!parent) return
    listings = parent.listings
    query = parent.query
    directory = parent.directory
    error = ''
    await tick()
    search?.focus()
  }

  async function pick(serverId: string, resource: McpResource) {
    if (reading) return
    const current = generation
    reading = `${serverId}\n${resource.uri}`
    error = ''
    try {
      if (resource.mime_type === 'inode/directory') {
        const children = await source.resourceDirectory(serverId, resource.uri)
        if (current !== generation) return
        parents.push({ listings, query, directory })
        listings = [children]
        directory = resource.title || resource.name
        query = ''
        await tick()
        if (current === generation) search?.focus()
        return
      }
      const contents = await source.readResource(serverId, resource.uri)
      if (current !== generation) return
      onAttach(contents.map((item) => mcpResourceToAttachment(serverId, item)))
      open = false
    } catch (err) {
      if (current === generation) error = errorToMessage(err)
    } finally {
      if (current === generation) reading = ''
    }
  }
</script>

<Modal
  bind:open
  title={getMessage('mcpResourcesTitle')}
  description={getMessage('mcpResourcesDescription')}
  contentClass="sm:max-w-xl"
>
  <div class="grid gap-3">
    <label class="relative block">
      <Search
        class="pointer-events-none absolute start-2.5 top-1/2 size-3.5 -translate-y-1/2 text-muted-foreground"
      />
      <input
        class={inputClass('h-8 ps-8 text-xs')}
        placeholder={getMessage('mcpResourcesSearch')}
        aria-label={getMessage('mcpResourcesSearch')}
        bind:this={search}
        bind:value={query}
      />
    </label>

    {#if parents.length}
      <div class="flex min-w-0 items-center gap-2 text-xs">
        <button
          type="button"
          class="inline-flex items-center gap-1 rounded px-2 py-1 hover:bg-muted"
          disabled={Boolean(reading)}
          onclick={back}
        >
          <ArrowLeft class="size-3.5 rtl:rotate-180" />{getMessage('mcpRegistryBack')}
        </button>
        <span class="truncate">{directory}</span>
      </div>
    {/if}

    {#if error}
      <p class="text-xs break-words whitespace-pre-wrap text-destructive">{error}</p>
    {/if}

    {#if loading}
      <div class="grid h-28 place-items-center text-muted-foreground">
        <LoaderCircle class="size-5 animate-spin" />
      </div>
    {:else if empty}
      <p class="py-6 text-center text-xs text-muted-foreground">
        {parents.length ? getMessage('mcpResourcesEmptyDirectory') : getMessage('mcpResourcesNone')}
      </p>
    {:else if noMatch}
      <p class="py-6 text-center text-xs text-muted-foreground">
        {getMessage('mcpResourcesNoMatch')}
      </p>
    {:else}
      {#each visible as listing (listing.server_id)}
        {#if listing.resources.length || listing.error}
          <section class="grid gap-1">
            <h3 class="truncate text-xs font-semibold text-muted-foreground">
              {listing.title || listing.server_id}
              {#if listing.title}<span class="font-mono font-normal">
                  · {listing.server_id}
                </span>{/if}
            </h3>
            {#if listing.error}
              <p class="text-xs break-words text-destructive">{listing.error}</p>
            {/if}
            {#each listing.resources as resource (resource.uri)}
              {@const busy = reading === `${listing.server_id}\n${resource.uri}`}
              <button
                type="button"
                class="flex w-full min-w-0 items-start gap-2 rounded-md px-2 py-1.5 text-start hover:bg-muted disabled:opacity-60"
                disabled={Boolean(reading)}
                aria-label={resource.mime_type === 'inode/directory'
                  ? getMessage('mcpResourcesOpenDirectory', resource.title || resource.name)
                  : undefined}
                onclick={() => void pick(listing.server_id, resource)}
              >
                {#if busy}
                  <LoaderCircle class="mt-0.5 size-3.5 shrink-0 animate-spin" />
                {:else if resource.mime_type === 'inode/directory'}
                  <Folder class="mt-0.5 size-3.5 shrink-0 text-muted-foreground" />
                {:else}
                  <FileText class="mt-0.5 size-3.5 shrink-0 text-muted-foreground" />
                {/if}
                <span class="grid min-w-0 flex-1 gap-0.5">
                  <span class="truncate text-sm">{resource.title || resource.name}</span>
                  <span class="truncate font-mono text-[11px] text-muted-foreground">
                    {resource.uri}
                  </span>
                  {#if resource.description}
                    <span class="line-clamp-2 text-xs text-muted-foreground">
                      {resource.description}
                    </span>
                  {/if}
                </span>
                {#if resource.size}
                  <span class="shrink-0 text-[11px] text-muted-foreground tabular-nums">
                    {formatFileSize(resource.size)}
                  </span>
                {/if}
              </button>
            {/each}
          </section>
        {/if}
      {/each}
    {/if}
  </div>
</Modal>
