<script lang="ts">
  import { onMount } from 'svelte'
  import { MemoryApi, type MemoryRecord, type ChangeInput, type ChangeView } from './api'
  import { buttonClass, textareaClass } from '../ui'
  import { getMessage } from '$lib/i18n'

  let {
    api,
    record,
    kind,
    storageKey,
    restored = false,
    onclose,
    onchanged
  }: {
    api: MemoryApi
    record: MemoryRecord | null
    kind: 'correct' | 'suppress' | 'delete'
    storageKey: string
    restored?: boolean
    onclose: () => void
    onchanged: () => void
  } = $props()
  const isCorrection = $derived(kind === 'correct')
  const isSuppression = $derived(kind === 'suppress')
  let dialog: HTMLDialogElement
  let text = $state('')
  let input = $state<ChangeInput | null>(null)
  let view = $state<ChangeView | null>(null)
  let busy = $state(false)
  let error = $state('')
  let unknown = $state(false)
  let disposed = false
  let timer: ReturnType<typeof setTimeout> | undefined

  function save() {
    if (!input) throw new Error('missing_change_identity')
    localStorage.setItem(storageKey, JSON.stringify({ input, view }))
  }
  async function prepare() {
    if (!record || busy) return
    busy = true
    error = ''
    unknown = false
    try {
      if (!input)
        input = {
          operation_id: crypto.randomUUID(),
          record_id: record.id,
          expected_revision: record.revision,
          kind,
          new_value: kind === 'correct' ? text : null
        }
      save()
      const result = await api.prepareChange(input)
      if (disposed) return
      view = result
      save()
    } catch (e) {
      if (!disposed) error = String(e)
    } finally {
      if (!disposed) busy = false
    }
  }
  async function confirm() {
    if (!input || !view || busy || unknown || view.state !== 'prepared') return
    busy = true
    error = ''
    try {
      save()
      const result = await api.commitChange(input.operation_id, view.preview_digest)
      if (disposed) return
      receive(result)
    } catch (e) {
      if (!disposed) {
        unknown = true
        error = String(e)
      }
    } finally {
      if (!disposed) busy = false
    }
  }
  function receive(result: ChangeView) {
    view = result
    unknown = false
    if (result.state === 'confirmed' || result.state === 'discarded') {
      localStorage.removeItem(storageKey)
      onchanged()
    } else {
      save()
      if (['committing', 'reconciling', 'cleanup_pending'].includes(result.state))
        timer = setTimeout(() => void check(), 5000)
    }
  }
  async function check() {
    if (!input || busy) return
    clearTimeout(timer)
    busy = true
    error = ''
    try {
      const result = await api.changeStatus(input.operation_id)
      if (!disposed) receive(result)
    } catch (e) {
      if (!disposed) {
        unknown = true
        error = String(e)
      }
    } finally {
      if (!disposed) busy = false
    }
  }
  async function undo() {
    if (!view?.replacement_record || !view.before || busy) return
    busy = true
    error = ''
    try {
      const current = await api.record(view.replacement_record)
      if (disposed) return
      if (!current.allowed_actions.includes('correct') || current.object_label !== view.new_value)
        throw new Error('revision_conflict')
      text = view.before.object_label
      record = current
      kind = 'correct'
      input = null
      view = null
      unknown = false
      restored = false
    } catch (e) {
      if (!disposed) error = String(e)
    } finally {
      if (!disposed) busy = false
    }
  }
  async function discard() {
    if (!input || busy) return
    busy = true
    error = ''
    try {
      await api.discardChange(input.operation_id)
      localStorage.removeItem(storageKey)
      if (!disposed) {
        onchanged()
        dialog.close()
      }
    } catch (e) {
      if (!disposed) error = String(e)
    } finally {
      if (!disposed) busy = false
    }
  }
  onMount(() => {
    text = record?.object_label || ''
    dialog.showModal()
    if (restored) {
      try {
        const saved = JSON.parse(localStorage.getItem(storageKey) || '{}')
        if (typeof saved.input?.operation_id !== 'string')
          throw new Error('missing_change_identity')
        input = saved.input
        // Refresh server state before allowing a restored draft to be confirmed.
        unknown = true
        void check()
      } catch (e) {
        error = String(e)
      }
    }
    return () => {
      disposed = true
      clearTimeout(timer)
    }
  })
</script>

<dialog
  bind:this={dialog}
  {onclose}
  class="m-auto w-[calc(100%_-_2rem)] max-w-lg rounded-lg border border-border bg-background p-0 text-foreground shadow-xl backdrop:bg-black/40"
  aria-labelledby="memory-change-title"
>
  <div class="max-h-[85vh] overflow-y-auto p-5 sm:p-6">
    <h2 id="memory-change-title" class="text-lg font-semibold">
      {getMessage(
        isCorrection ? 'memoryCorrect' : isSuppression ? 'memorySuppress' : 'memoryDelete'
      )}
    </h2>
    {#if view?.state === 'confirmed'}
      <p role="status" class="mt-4 text-sm">{getMessage('memoryChangeConfirmed')}</p>
      {#if view.kind === 'correct' && view.replacement_record && view.before}<button
          class={buttonClass('outline', 'sm', 'mt-4')}
          disabled={busy}
          onclick={undo}>{getMessage('memoryUndoPreview')}</button
        >{/if}
    {:else if view?.state === 'discarded'}
      <p role="status" class="mt-4 text-sm">{getMessage('memoryChangeDiscarded')}</p>
    {:else if view}
      {#if view.before}<p class="mt-4 whitespace-pre-wrap break-words text-sm">
          {view.before.object_label}
        </p>{/if}
      {#if view.new_value}<p
          class="mt-3 whitespace-pre-wrap break-words rounded-md bg-muted/50 p-3 text-sm"
        >
          → {view.new_value}
        </p>{/if}
      <details class="mt-4 text-sm">
        <summary class="cursor-pointer"
          >{getMessage('memoryChangeAffected', String(view.targets.length))}</summary
        >
        <ul class="mt-2 list-inside list-disc space-y-2">
          {#each view.affected_records || [] as affected (affected.id)}<li class="break-words">
              {affected.text}
            </li>{/each}
        </ul>
        <p class="mt-2 break-all text-xs text-muted-foreground">{view.targets.join(', ')}</p>
      </details>
      <p class="mt-3 text-xs leading-relaxed text-muted-foreground">
        {getMessage(isSuppression ? 'memorySuppressScope' : 'memoryChangeScope')}
      </p>
      {#if view.state !== 'prepared'}<p role="status" class="mt-4 text-sm">
          {getMessage('memoryChangePending')}
        </p>{/if}
    {:else if !restored}
      <p class="mt-4 whitespace-pre-wrap break-words text-sm">{record?.object_label}</p>
      {#if kind === 'correct'}<label class="mt-4 block text-sm"
          >{getMessage('memoryNewValue')}<textarea
            class={textareaClass('mt-2 min-h-24')}
            bind:value={text}
            disabled={busy || !!input}></textarea></label
        >{/if}
      <p class="mt-3 text-xs leading-relaxed text-muted-foreground">
        {getMessage('memoryReviewHint')}
      </p>
    {/if}
    {#if error}<p role="alert" class="mt-4 break-words text-sm text-destructive">{error}</p>{/if}
    {#if unknown}<p class="mt-3 text-xs text-muted-foreground">
        {getMessage('memoryUnknownHint')}
      </p>{/if}
    <div class="mt-6 flex flex-wrap justify-end gap-2">
      <button class={buttonClass('outline', 'sm')} onclick={() => dialog.close()}
        >{getMessage('memoryClose')}</button
      >
      {#if input && !unknown && (!view || view.state === 'prepared')}<button
          class={buttonClass('ghost', 'sm')}
          disabled={busy}
          onclick={discard}>{getMessage('memoryDiscardDraft')}</button
        >{/if}
      {#if view?.state !== 'confirmed' && view?.state !== 'discarded'}
        {#if unknown || (view && view.state !== 'prepared') || (restored && !view)}
          <button class={buttonClass('default', 'sm')} disabled={busy || !input} onclick={check}
            >{getMessage('memoryCheckResult')}</button
          >
        {:else if view}
          <button
            class={buttonClass(kind === 'delete' ? 'destructive' : 'default', 'sm')}
            disabled={busy || !!view.error || Date.now() > view.expires_at}
            onclick={confirm}
            >{getMessage(
              isCorrection
                ? 'memoryConfirmCorrection'
                : isSuppression
                  ? 'memoryConfirmSuppress'
                  : 'memoryConfirmDelete'
            )}</button
          >
        {:else}
          <button
            class={buttonClass('default', 'sm')}
            disabled={busy ||
              (kind === 'correct' && (!text.trim() || text === record?.object_label))}
            onclick={prepare}>{getMessage('memoryReviewChange')}</button
          >
        {/if}
      {/if}
    </div>
    {#if input && view?.state !== 'confirmed' && view?.state !== 'discarded'}<p
        class="mt-4 text-xs leading-relaxed text-muted-foreground"
      >
        {getMessage('memoryDraftStored')}
      </p>{/if}
  </div>
</dialog>
