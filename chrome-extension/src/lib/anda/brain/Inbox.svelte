<script lang="ts">
  import { getMessage } from '$lib/i18n'
  import { buttonClass, textareaClass, badgeClass } from '$lib/anda/ui'
  import {
    BrainApi,
    type BrainGraphSettings,
    type AttentionPage,
    type RuntimeStatus,
    type AttentionResponse,
    brainPendingStorageKey
  } from './api'
  import { onMount } from 'svelte'

  let { settings }: { settings: BrainGraphSettings } = $props()
  let page = $state<AttentionPage | null>(null)
  let status = $state<RuntimeStatus | null>(null)
  let error = $state('')
  let receipt = $state('')
  let busy = $state(false)
  let drafts = $state<Record<string, string>>({})
  let pending = $state<Record<string, AttentionResponse>>({})
  let storageKey = ''
  let identity = 0

  // Persist a response before submitting it, so a lost ACK or a page reload
  // retries exactly the same logical event. Separate every credential/Space.
  async function bindIdentity() {
    const version = ++identity
    page = null
    status = null
    pending = {}
    drafts = {}
    error = ''
    receipt = ''
    storageKey = ''
    busy = true
    try {
      const api = new BrainApi(settings)
      const runtime = await api.runtimeStatus()
      const key = await brainPendingStorageKey(settings, runtime.caller)
      if (version !== identity) return
      storageKey = key
      try {
        const saved = JSON.parse(localStorage.getItem(storageKey) || '{}')
        pending = saved && typeof saved === 'object' && !Array.isArray(saved) ? saved : {}
      } catch {
        pending = {}
      }
      status = runtime
      const next = runtime.configured ? await api.attention() : null
      if (version !== identity) return
      page = next
    } catch (e) {
      if (version === identity) error = String(e)
    } finally {
      if (version === identity) busy = false
    }
  }
  onMount(() => {
    void bindIdentity()
  })

  function responseText(response: AttentionResponse): string {
    return response.kind === 'clarification' ? response.answer : response.statement
  }
  function itemText(value: unknown): string {
    if (!value || typeof value !== 'object') return ''
    const data = value as Record<string, unknown>
    for (const key of ['question', 'message']) if (typeof data[key] === 'string') return data[key]
    return data.payload && data.payload !== value ? itemText(data.payload) : ''
  }

  async function refresh(cursor?: string) {
    const version = identity
    busy = true
    error = ''
    try {
      const api = new BrainApi(settings)
      const runtime = await api.runtimeStatus()
      const next = runtime.configured ? await api.attention(cursor) : null
      if (version !== identity) return
      status = runtime
      page = next
    } catch (e) {
      if (version === identity) error = String(e)
    } finally {
      if (version === identity) busy = false
    }
  }
  async function send(id: string, clarification: boolean) {
    const version = identity
    busy = true
    error = ''
    receipt = ''
    try {
      if (!pending[id]) {
        const text = drafts[id]?.trim()
        if (!text) return
        pending[id] = clarification
          ? { kind: 'clarification', event_key: crypto.randomUUID(), answer: text }
          : { kind: 'agent_statement', event_key: crypto.randomUUID(), statement: text }
        localStorage.setItem(storageKey, JSON.stringify(pending))
      }
      const result = await new BrainApi(settings).respond(id, pending[id])
      if (version !== identity) return
      delete pending[id]
      delete drafts[id]
      localStorage.setItem(storageKey, JSON.stringify(pending))
      receipt = result.status
      await refresh()
    } catch (e) {
      if (version === identity) error = String(e)
    } finally {
      if (version === identity) busy = false
    }
  }
</script>

<section
  class="flex h-full min-h-0 flex-col gap-4 overflow-y-auto bg-background p-4 sm:p-6"
  aria-label={getMessage('brainInbox')}
>
  <header class="flex flex-wrap items-start justify-between gap-3 border-b border-border pb-4">
    <div>
      <h2 class="text-base font-semibold">{getMessage('brainInbox')}</h2>
      <p class="mt-1 max-w-xl text-xs text-muted-foreground">{getMessage('brainInboxHint')}</p>
    </div>
    <button class={buttonClass('outline', 'sm')} disabled={busy} onclick={() => refresh()}
      >{getMessage('refresh')}</button
    >
  </header>
  {#if error}<p role="alert" class="break-words text-sm text-destructive">{error}</p>{/if}
  {#if receipt}<p role="status" class="text-sm">{receipt}</p>{/if}
  {#if status}
    <div class="flex flex-wrap gap-2">
      <span class={badgeClass('outline')}
        >{getMessage(
          status.configured ? 'brainRuntimeConfigured' : 'brainRuntimeUnconfigured'
        )}</span
      >
      <span class={badgeClass('outline')}>{getMessage('brainInbox')}: {status.visible_items}</span>
    </div>
    <details class="rounded-md border border-border p-3 text-xs">
      <summary class="cursor-pointer font-medium">{getMessage('brainRuntimeStatus')}</summary>
      <pre class="mt-3 max-h-80 overflow-auto whitespace-pre-wrap break-all">{JSON.stringify(
          status,
          null,
          2
        )}</pre>
    </details>
  {/if}
  {#if page}
    {#if page.items.length === 0}<p class="py-6 text-sm text-muted-foreground">
        {getMessage('brainInboxEmpty')}
      </p>{/if}
    {#each page.items as item (item.id)}
      <article class="border-b border-border pb-5">
        <div class="flex flex-wrap items-center justify-between gap-2">
          <h3 class="text-sm font-medium">{item.summary}</h3>
          <span class={badgeClass('secondary')}>{item.state}</span>
        </div>
        {#if item.reason}<p class="mt-2 text-xs text-muted-foreground">{item.reason}</p>{/if}
        {#if item.clarification || item.delivery}
          <pre class="my-3 whitespace-pre-wrap break-words text-xs">{itemText(item.clarification) ||
              itemText(item.delivery)}</pre>
        {/if}
        {#if pending[item.id]}
          <p class="my-3 text-xs text-muted-foreground">{getMessage('brainResponsePending')}</p>
          <pre class="mb-3 whitespace-pre-wrap break-words text-xs">{responseText(
              pending[item.id]
            )}</pre>
        {:else}
          <label class="my-3 block text-xs">
            {getMessage(item.clarification ? 'brainClarificationAnswer' : 'brainAgentStatement')}
            <textarea
              class={textareaClass('mt-2 min-h-20')}
              bind:value={drafts[item.id]}
              disabled={busy}></textarea>
          </label>
        {/if}
        <button
          class={buttonClass('outline', 'sm')}
          disabled={busy || (!pending[item.id] && !drafts[item.id]?.trim())}
          onclick={() => send(item.id, !!item.clarification)}
        >
          {getMessage(pending[item.id] ? 'brainRetryResponse' : 'brainSendResponse')}
        </button>
      </article>
    {/each}
    {#if page.next_cursor}<button
        class={buttonClass('outline', 'sm')}
        disabled={busy}
        onclick={() => refresh(page?.next_cursor)}>{getMessage('brainNextPage')}</button
      >{/if}
    <p class="text-xs text-muted-foreground">{getMessage('brainPageCompleteHint')}</p>
  {/if}
  {#if Object.keys(pending).some((id) => !page?.items.some((item) => item.id === id))}
    <details class="border border-border p-3 text-xs">
      <summary>{getMessage('brainResponsePending')}</summary>
      {#each Object.entries(pending).filter(([id]) => !page?.items.some((item) => item.id === id)) as [id, response]}
        <pre class="my-2 whitespace-pre-wrap break-all">{responseText(response)}</pre>
        <button
          class={buttonClass('outline', 'sm')}
          disabled={busy}
          onclick={() => send(id, response.kind === 'clarification')}
          >{getMessage('brainRetryResponse')}</button
        >
      {/each}
    </details>
  {/if}
</section>
