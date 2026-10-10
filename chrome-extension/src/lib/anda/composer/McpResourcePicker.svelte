<script lang="ts" module>
  import type { McpResourceAttachment, McpResourceListing } from '$lib/anda/client/types'

  /** Where the picker lists and reads resources: the app's `McpApi`. */
  export interface McpResourceSource {
    resources(id?: string): Promise<McpResourceListing[]>
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
  import { FileText, LoaderCircle, Search } from '@lucide/svelte'
  import { untrack } from 'svelte'

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
  let loading = $state(false)
  let query = $state('')
  let reading = $state('')
  let error = $state('')

  // A fresh listing each time it opens; `load` writes state it would track.
  $effect(() => {
    if (open) untrack(() => void load())
  })

  async function load() {
    loading = true
    error = ''
    query = ''
    try {
      listings = await source.resources()
    } catch (err) {
      listings = []
      error = errorToMessage(err)
    } finally {
      loading = false
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

  async function attach(serverId: string, resource: McpResource) {
    if (reading) return
    reading = `${serverId}\n${resource.uri}`
    error = ''
    try {
      const contents = await source.readResource(serverId, resource.uri)
      onAttach(contents.map((item) => mcpResourceToAttachment(serverId, item)))
      open = false
    } catch (err) {
      error = errorToMessage(err)
    } finally {
      reading = ''
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
        bind:value={query}
      />
    </label>

    {#if error}
      <p class="text-xs break-words whitespace-pre-wrap text-destructive">{error}</p>
    {/if}

    {#if loading}
      <div class="grid h-28 place-items-center text-muted-foreground">
        <LoaderCircle class="size-5 animate-spin" />
      </div>
    {:else if empty}
      <p class="py-6 text-center text-xs text-muted-foreground">
        {getMessage('mcpResourcesNone')}
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
              {#if listing.title}<span class="font-mono font-normal">· {listing.server_id}</span
                >{/if}
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
                onclick={() => void attach(listing.server_id, resource)}
              >
                {#if busy}
                  <LoaderCircle class="mt-0.5 size-3.5 shrink-0 animate-spin" />
                {:else}
                  <FileText class="mt-0.5 size-3.5 shrink-0 text-muted-foreground" />
                {/if}
                <span class="grid min-w-0 flex-1 gap-0.5">
                  <span class="truncate text-sm">{resource.title || resource.name}</span>
                  <span class="truncate font-mono text-[11px] text-muted-foreground"
                    >{resource.uri}</span
                  >
                  {#if resource.description}
                    <span class="line-clamp-2 text-xs text-muted-foreground"
                      >{resource.description}</span
                    >
                  {/if}
                </span>
                {#if resource.size}
                  <span class="shrink-0 text-[11px] text-muted-foreground tabular-nums"
                    >{formatFileSize(resource.size)}</span
                  >
                {/if}
              </button>
            {/each}
          </section>
        {/if}
      {/each}
    {/if}
  </div>
</Modal>
