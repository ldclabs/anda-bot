<script lang="ts">
  import { onDestroy } from 'svelte'
  import { getMessage } from '$lib/i18n'
  import { badgeClass, buttonClass, inputClass } from '../ui'
  import type { MemoryApi, MemoryEntity } from './api'

  let {
    api,
    onopen
  }: {
    api: MemoryApi
    /** A null id opens the page about the caller. */
    onopen: (id: string | null, label: string) => void
  } = $props()

  let query = $state('')
  let results = $state<MemoryEntity[] | null>(null)
  let error = $state('')
  let busy = $state(false)
  let disposed = false

  async function search(event: SubmitEvent) {
    event.preventDefault()
    if (busy || !query.trim()) return
    busy = true
    error = ''
    results = null
    try {
      const page = await api.entitySearch(query)
      if (!disposed) results = page.items
    } catch (e) {
      if (!disposed) error = String(e)
    } finally {
      if (!disposed) busy = false
    }
  }
  onDestroy(() => {
    disposed = true
  })
</script>

<section class="my-7 border-b border-border pb-6" aria-labelledby="memory-entities">
  <div class="flex flex-wrap items-center justify-between gap-3">
    <h2 id="memory-entities" class="text-sm font-semibold">{getMessage('memoryEntities')}</h2>
    <button
      class={buttonClass('outline', 'sm')}
      onclick={() => onopen(null, getMessage('memoryYou'))}>{getMessage('memoryAboutYou')}</button
    >
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
        disabled={busy}
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
  </form>
  {#if error}<p role="alert" class="mt-3 text-sm break-words text-destructive">{error}</p>{/if}
  {#if results}
    {#if results.length}
      <ul class="mt-4 divide-y divide-border" aria-live="polite">
        {#each results as entity (entity.id)}
          <li>
            <button
              class="flex w-full cursor-pointer items-center justify-between gap-3 py-3 text-left text-sm hover:text-foreground/80 focus-visible:outline-2 focus-visible:outline-ring"
              onclick={() => onopen(entity.id, entity.name)}
            >
              <span class="min-w-0 font-medium break-words"
                >{entity.name}{#if entity.about_owner}
                  · {getMessage('memoryYou')}{/if}</span
              >
              <span class={badgeClass('outline')}>{entity.type}</span>
            </button>
          </li>
        {/each}
      </ul>
    {:else}
      <p class="mt-4 text-sm text-muted-foreground" aria-live="polite">
        {getMessage('memoryEntitySearchEmpty')}
      </p>
    {/if}
  {/if}
</section>
