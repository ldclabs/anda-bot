<script lang="ts">
  import { onMount } from 'svelte'
  import { getMessage } from '$lib/i18n'
  import Modal from '../Modal.svelte'
  import { buttonClass } from '../ui'
  import type { MemoryApi, SetupPreview } from './api'
  let { api, onclose }: { api: MemoryApi; onclose: () => void } = $props()
  let open = $state(true)
  let view = $state<SetupPreview | null>(null)
  let busy = $state(false)
  let error = $state('')
  let disposed = false
  async function prepare() {
    busy = true
    error = ''
    try {
      const result = await api.setupPreview()
      if (!disposed) view = result
    } catch (e) {
      if (!disposed) error = String(e)
    } finally {
      if (!disposed) busy = false
    }
  }
  async function apply() {
    if (!view || busy) return
    busy = true
    error = ''
    try {
      const result = await api.setupCommit(view.preview_digest)
      if (!disposed) view = result
    } catch (e) {
      if (!disposed) error = String(e)
    } finally {
      if (!disposed) busy = false
    }
  }
  onMount(() => {
    void prepare()
    return () => {
      disposed = true
    }
  })
</script>

{#snippet actions()}
  <button class={buttonClass('outline', 'sm')} onclick={() => (open = false)}
    >{getMessage('close')}</button
  >
  {#if view?.state === 'prepared'}<button
      class={buttonClass('default', 'sm')}
      disabled={busy}
      onclick={apply}>{getMessage('memorySetupApply')}</button
    >{/if}
  {#if error}<button class={buttonClass('outline', 'sm')} disabled={busy} onclick={prepare}
      >{getMessage('refresh')}</button
    >{/if}
{/snippet}

<Modal
  bind:open
  onOpenChangeComplete={(value) => {
    if (!value) onclose()
  }}
  title={getMessage('memorySetupTitle')}
  contentClass="sm:max-w-lg"
  footer={actions}
>
  <p class="text-sm leading-relaxed text-muted-foreground">{getMessage('memorySetupScope')}</p>
  {#if error}<p role="alert" class="mt-4 text-sm break-words text-destructive">{error}</p>{/if}
  {#if view?.state === 'restart_required'}
    <p role="status" class="mt-4 text-sm">{getMessage('memorySetupRestart')}</p>
    <code class="mt-3 block rounded bg-muted p-3 text-sm">anda restart</code>
  {:else if view}
    <p class="mt-4 text-sm">{view.config_file} → {view.runtime_file}</p>
    <p class="mt-2 text-xs text-muted-foreground">{getMessage('memorySetupPreview')}</p>
    <details class="mt-3 text-xs">
      <summary class="cursor-pointer">{getMessage('memoryTechnicalDetails')}</summary>
      <pre class="mt-2 overflow-auto break-words whitespace-pre-wrap">{JSON.stringify(
          view.changes,
          null,
          2
        )}
{view.managed_runtime}</pre>
    </details>
  {/if}
</Modal>
