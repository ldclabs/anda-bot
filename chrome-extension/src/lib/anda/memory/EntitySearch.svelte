<script lang="ts">
  import { onDestroy } from 'svelte'
  import { LoaderCircle, Search } from '@lucide/svelte'
  import { getMessage } from '$lib/i18n'
  import Modal from '../Modal.svelte'
  import { badgeClass, buttonClass, inputClass } from '../ui'
  import type { MemoryApi, MemoryEntity } from './api'
  import { entityName } from './labels'

  let {
    api,
    onopen
  }: {
    api: MemoryApi
    onopen: (id: string, label: string) => void
  } = $props()

  let query = $state('')
  let results = $state<MemoryEntity[] | null>(null)
  let error = $state('')
  let busy = $state(false)
  /** The matches open in a modal; `searched` is the query they answer. */
  let open = $state(false)
  let searched = $state('')
  let disposed = false

  async function search(event: SubmitEvent) {
    event.preventDefault()
    if (busy || !query.trim()) return
    busy = true
    error = ''
    results = null
    searched = query.trim()
    open = true
    try {
      const page = await api.entitySearch(query)
      if (!disposed) results = page.items
    } catch (e) {
      if (!disposed) error = String(e)
    } finally {
      if (!disposed) busy = false
    }
  }
  function pick(entity: MemoryEntity, name: string) {
    open = false
    onopen(entity.id, name)
  }
  onDestroy(() => {
    disposed = true
  })
</script>

<form class="mt-2 grid gap-1.5 px-0.5" onsubmit={search}>
  <div class="grid grid-cols-[minmax(0,1fr)_auto] gap-1.5">
    <div class="relative min-w-0">
      <Search
        class="pointer-events-none absolute top-1/2 left-2.5 size-3.5 -translate-y-1/2 text-muted-foreground"
      />
      <input
        class={inputClass('h-8 pl-8 text-xs')}
        bind:value={query}
        required
        maxlength="200"
        placeholder={getMessage('memoryEntitySearchLabel')}
        aria-label={getMessage('memoryEntitySearchLabel')}
      />
    </div>
    <button class={buttonClass('outline', 'sm', 'h-8')} disabled={busy || !query.trim()}
      >{getMessage('memoryEntitySearch')}</button
    >
  </div>
  <p class="px-0.5 text-[11px] leading-relaxed text-muted-foreground">
    {getMessage('memoryEntitySearchHint')}
  </p>
  {#if !open && (busy || results || error)}<button
      type="button"
      class={buttonClass('ghost', 'xs', 'justify-self-start')}
      onclick={() => (open = true)}>{getMessage('memoryShowResults')}</button
    >{/if}
</form>

<Modal bind:open title={getMessage('memoryEntities')} description={searched}>
  <div aria-live="polite">
    {#if busy}<p role="status" class="flex items-center gap-2 text-sm text-muted-foreground">
        <LoaderCircle class="size-4 animate-spin" />{getMessage('loading')}
      </p>{/if}
    {#if error}<p role="alert" class="text-sm break-words text-destructive">{error}</p>{/if}
    {#if results?.length}
      <ul class="-my-1 divide-y divide-border">
        {#each results as entity (entity.id)}
          {@const name = entityName(entity)}
          <li>
            <button
              class="-mx-2 flex w-[calc(100%+1rem)] cursor-pointer items-center justify-between gap-3 rounded-md px-2 py-3 text-start text-sm hover:bg-muted focus-visible:outline-2 focus-visible:outline-ring"
              onclick={() => pick(entity, name)}
            >
              <span class="min-w-0 font-medium break-words">{name}</span>
              <span class={badgeClass('outline')}>{entity.type}</span>
            </button>
          </li>
        {/each}
      </ul>
    {:else if results}
      <p class="text-sm text-muted-foreground">{getMessage('memoryEntitySearchEmpty')}</p>
    {/if}
  </div>
</Modal>
