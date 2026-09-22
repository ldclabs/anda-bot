<script lang="ts">
  import { onMount } from 'svelte'
  import { getMessage } from '$lib/i18n'
  import { buttonClass } from '../ui'
  import type { MemoryApi, RecordWatch } from './api'
  let {
    api,
    recordId,
    scope,
    existing,
    onchanged,
    canCreate = true
  }: {
    api: MemoryApi
    recordId: string
    scope: string
    existing?: RecordWatch
    onchanged: () => void
    canCreate?: boolean
  } = $props()
  let operation = $state('')
  let busy = $state(false)
  let error = $state('')
  let disposed = false
  const key = $derived(`${scope}/watch/${recordId}`)
  const active = $derived(!!existing && existing.state !== 'cancelled')
  async function subscribe() {
    if (busy) return
    busy = true
    error = ''
    try {
      operation ||= crypto.randomUUID()
      localStorage.setItem(key, operation)
      await api.watch(operation, recordId)
      localStorage.removeItem(key)
      if (!disposed) {
        operation = ''
        onchanged()
      }
    } catch (e) {
      if (!disposed) error = String(e)
    } finally {
      if (!disposed) busy = false
    }
  }
  async function cancel() {
    if (!existing || busy) return
    busy = true
    error = ''
    try {
      await api.cancelWatch(existing.operation_id)
      if (!disposed) onchanged()
    } catch (e) {
      if (!disposed) error = String(e)
    } finally {
      if (!disposed) busy = false
    }
  }
  onMount(() => {
    try {
      operation = localStorage.getItem(key) || ''
    } catch (e) {
      error = String(e)
    }
    return () => {
      disposed = true
    }
  })
</script>

<div class="mt-3 text-xs">
  {#if active}
    <p class="text-muted-foreground">{getMessage('memoryWatching')}</p>
    <button class={buttonClass('ghost', 'xs', 'mt-1')} disabled={busy} onclick={cancel}
      >{getMessage('memoryWatchCancel')}</button
    >
  {:else if canCreate || operation}
    <button class={buttonClass('ghost', 'xs')} disabled={busy} onclick={subscribe}
      >{getMessage(operation ? 'memoryWatchRetry' : 'memoryWatch')}</button
    >
  {/if}
  {#if error}<p role="alert" class="mt-2 break-words text-destructive">{error}</p>{/if}
</div>
