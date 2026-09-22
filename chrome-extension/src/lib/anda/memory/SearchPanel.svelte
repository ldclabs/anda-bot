<script lang="ts">
  import { onDestroy } from 'svelte'
  import { getMessage } from '$lib/i18n'
  import { buttonClass, inputClass } from '../ui'
  import type { MemoryApi, SearchResult } from './api'
  let { api }: { api: MemoryApi } = $props()
  let query = $state('')
  let result = $state<SearchResult | null>(null)
  let error = $state('')
  let busy = $state(false)
  const packet = $derived.by(() => {
    try {
      const value = JSON.parse(result?.packet || 'null')
      return value?.format === 'anda-brain-recall/1' && Array.isArray(value.items)
        ? (value as {
            items: Array<{ id: string; priority: string; content: unknown }>
            coverage: unknown
            status: string
            semantic_complete: boolean
          })
        : null
    } catch {
      return null
    }
  })
  let disposed = false
  let copied = $state(false)
  async function search(event: SubmitEvent) {
    event.preventDefault()
    if (busy || !query.trim()) return
    busy = true
    error = ''
    result = null
    copied = false
    try {
      const next = await api.search(query)
      if (!disposed) result = next
    } catch (e) {
      if (!disposed) error = String(e)
    } finally {
      if (!disposed) busy = false
    }
  }
  async function copy() {
    if (!result) return
    try {
      await navigator.clipboard.writeText(result.packet)
      copied = true
    } catch (e) {
      error = String(e)
    }
  }
  onDestroy(() => {
    disposed = true
  })
</script>

<section class="my-7 border-b border-border pb-6" aria-label={getMessage('memorySearch')}>
  <form onsubmit={search}>
    <label for="memory-search" class="text-sm font-semibold">{getMessage('memorySearch')}</label>
    <div class="mt-3 flex gap-2">
      <input
        id="memory-search"
        class={inputClass('min-w-0 flex-1')}
        bind:value={query}
        disabled={busy}
        required
        placeholder={getMessage('memorySearchPlaceholder')}
      />
      <button class={buttonClass('outline', 'sm')} disabled={busy || !query.trim()}
        >{getMessage('memorySearch')}</button
      >
    </div>
    <p class="mt-2 text-xs leading-relaxed text-muted-foreground">
      {getMessage('memorySearchFee')}
    </p>
    <details class="mt-2 text-xs text-muted-foreground">
      <summary class="cursor-pointer">{getMessage('memoryTechnicalDetails')}</summary>
      <p class="mt-2 leading-relaxed">{getMessage('memorySearchCost')}</p>
    </details>
  </form>
  {#if error}<p role="alert" class="mt-3 break-words text-sm text-destructive">
      {getMessage('memorySearchUnknown')}
      {error}
    </p>{/if}
  {#if result}
    <p class="mt-4 text-xs text-muted-foreground">{getMessage('memorySearchPacket')}</p>
    {#if packet}
      {#each packet.items as item (item.id)}
        <article class="mt-3 rounded border border-border p-3">
          {#if item.priority === 'warning' || item.priority === 'required'}<p
              class="mb-2 text-xs font-semibold"
            >
              {getMessage('memorySearchConstraint')}
            </p>{/if}
          <pre
            class="whitespace-pre-wrap break-words text-sm leading-relaxed">{typeof item.content ===
            'string'
              ? item.content
              : JSON.stringify(item.content, null, 2)}</pre>
        </article>
      {/each}
      {#if packet.items.length === 0}<p class="mt-3 text-sm text-muted-foreground">
          {getMessage('memorySearchEmpty')}
        </p>{/if}
    {/if}
    <details class="mt-3 text-xs">
      <summary class="cursor-pointer text-muted-foreground"
        >{getMessage('memorySearchCoverage')}</summary
      >
      <pre
        class="mt-3 max-h-80 overflow-auto whitespace-pre-wrap break-words rounded bg-muted/40 p-4">{result.packet}</pre>
    </details>
    <button class={buttonClass('ghost', 'xs', 'mt-3')} onclick={copy}
      >{getMessage(copied ? 'memoryCopied' : 'memoryCopyPacket')}</button
    >
  {/if}
</section>
