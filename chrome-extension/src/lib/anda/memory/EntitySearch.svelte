<script lang="ts">
  import { onDestroy } from 'svelte'
  import { LoaderCircle } from '@lucide/svelte'
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
    /** A null id opens the page about the caller; `$self` and `$system` the
     * Brain's own actors. */
    onopen: (id: string | null, label: string) => void
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

<section class="my-7 border-b border-border pb-6" aria-labelledby="memory-entities">
  <div class="flex flex-wrap items-center justify-between gap-3">
    <h2 id="memory-entities" class="text-sm font-semibold">{getMessage('memoryEntities')}</h2>
    <div class="flex flex-wrap gap-2">
      <button
        class={buttonClass('outline', 'sm')}
        onclick={() => onopen(null, getMessage('memoryYou'))}>{getMessage('memoryAboutYou')}</button
      >
      <button
        class={buttonClass('outline', 'sm')}
        onclick={() => onopen('$self', getMessage('memoryBrainSelf'))}
        >{getMessage('memoryBrainSelf')}</button
      >
      <button
        class={buttonClass('outline', 'sm')}
        onclick={() => onopen('$system', getMessage('memoryBrainSystem'))}
        >{getMessage('memoryBrainSystem')}</button
      >
    </div>
  </div>
  <form class="mt-4" onsubmit={search}>
    <label for="memory-entity-search" class="text-xs text-muted-foreground"
      >{getMessage('memoryEntitySearchLabel')}</label
    >
    <div class="mt-2 flex gap-2">
      <input
        id="memory-entity-search"
        class={inputClass('min-w-0 flex-1')}
        bind:value={query}
        required
        maxlength="200"
        placeholder={getMessage('memoryEntitySearchPlaceholder')}
      />
      <button class={buttonClass('outline', 'sm')} disabled={busy || !query.trim()}
        >{getMessage('memoryEntitySearch')}</button
      >
    </div>
    <p class="mt-2 text-xs leading-relaxed text-muted-foreground">
      {getMessage('memoryEntitySearchHint')}
    </p>
    {#if !open && (busy || results || error)}<button
        type="button"
        class={buttonClass('ghost', 'xs', 'mt-2')}
        onclick={() => (open = true)}>{getMessage('memoryShowResults')}</button
      >{/if}
  </form>
</section>

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
