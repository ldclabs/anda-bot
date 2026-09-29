<script lang="ts">
  import type { Snippet } from 'svelte'
  import { getMessage } from '$lib/i18n'
  import { buttonClass } from '../ui'
  import { REVISION_KINDS, type ChangeKind, type MemoryRecord } from './api'
  import { actorLabel } from './labels'

  type Source = MemoryRecord['sources'][number]
  let {
    record,
    changes = false,
    changeDisabled = false,
    onchange,
    onsource,
    onentity,
    status,
    footer
  }: {
    record: MemoryRecord
    /** Whether confirmed changes are available for this record. */
    changes?: boolean
    changeDisabled?: boolean
    onchange: (kind: ChangeKind) => void
    onsource: (source: Source) => void
    /** Makes the subject and object open their entity pages. */
    onentity?: (id: string, label: string) => void
    status?: Snippet
    footer?: Snippet
  } = $props()

  const subject = $derived(
    record.about_owner ? getMessage('memoryYou') : actorLabel(record.subject_label)
  )
  const object = $derived(actorLabel(record.object_label))
  const revision = $derived(REVISION_KINDS.find((kind) => record.allowed_actions.includes(kind)))
  const stateLabel = $derived(
    record.state === 'active'
      ? getMessage('memoryRecordCurrent')
      : record.state === 'superseded'
        ? getMessage('memoryRecordSuperseded')
        : record.state === 'retracted'
          ? getMessage('memoryRecordRetracted')
          : record.state === 'archived'
            ? getMessage('memoryRecordSuppressed')
            : getMessage('memoryRecordUnknown')
  )
  const stanceLabel = $derived(
    record.stance === 'support'
      ? getMessage('memoryClaimSupport')
      : record.stance === 'reject'
        ? getMessage('memoryClaimReject')
        : getMessage('memoryClaimUncertain')
  )
  const entityLink =
    'cursor-pointer rounded-sm text-left underline decoration-border underline-offset-4 hover:decoration-current focus-visible:outline-2 focus-visible:outline-ring'
</script>

<article class="border-b border-border py-5">
  <p class="text-xs text-muted-foreground">
    {#if onentity && record.subject_id}<button
        class={entityLink}
        aria-label={getMessage('memoryOpenEntity', subject)}
        onclick={() => onentity(record.subject_id!, subject)}>{subject}</button
      >{:else}{subject}{/if} · {record.predicate_label}
  </p>
  <p class="mt-2 text-sm font-medium break-words whitespace-pre-wrap">
    {#if onentity && record.object_id}<button
        class={entityLink}
        aria-label={getMessage('memoryOpenEntity', object)}
        onclick={() => onentity(record.object_id!, object)}>{object}</button
      >{:else}{object}{/if}
  </p>
  <p class="mt-2 flex flex-wrap items-center gap-x-2 gap-y-1 text-xs text-muted-foreground">
    {@render status?.()}<span>{stanceLabel} · {stateLabel}</span>
  </p>
  {#if !record.sources_complete}<p class="mt-2 text-xs text-muted-foreground">
      {#if record.sources.length}
        {getMessage('memorySourceUnavailable')}
      {:else}
        {getMessage('memorySourceUnrecorded')}
      {/if}
    </p>{/if}
  {#if record.sources.length}
    <details class="mt-3 text-xs">
      <summary class="cursor-pointer text-muted-foreground"
        >{getMessage('memorySource')} ({record.sources.length})</summary
      >
      {#each record.sources as source}
        <div class="mt-3">
          <p class="text-muted-foreground">
            {#if source.conversation}{getMessage('memoryConversation')} #{source.conversation}{:else}{getMessage(
                'memoryCorrectionSource'
              )}{/if}
          </p>
          <blockquote
            class="mt-2 border-l-2 border-border pl-3 leading-relaxed break-words whitespace-pre-wrap"
          >
            {source.text || getMessage('memorySourceUnavailable')}
          </blockquote>
          {#if source.text_truncated}<p class="mt-2 text-muted-foreground">
              {getMessage('memorySourceTruncated')}
            </p>{/if}
          {#if source.conversation && source.index && source.source && Number.isSafeInteger(Number(source.conversation))}
            <button class={buttonClass('ghost', 'xs', 'mt-2')} onclick={() => onsource(source)}
              >{getMessage('memoryOpenSource')}</button
            >
          {/if}
        </div>
      {/each}
    </details>
  {/if}
  {#if changes}
    <div class="mt-3 flex gap-2">
      {#if revision}<button
          class={buttonClass('ghost', 'xs')}
          disabled={changeDisabled}
          onclick={() => onchange(revision)}>{getMessage('memoryCorrect')}</button
        >{/if}
      {#if record.allowed_actions.includes('suppress')}<button
          class={buttonClass('ghost', 'xs')}
          disabled={changeDisabled}
          onclick={() => onchange('suppress')}>{getMessage('memorySuppress')}</button
        >{/if}
      {#if record.allowed_actions.includes('delete')}<button
          class={buttonClass('ghost', 'xs')}
          disabled={changeDisabled}
          onclick={() => onchange('delete')}>{getMessage('memoryDelete')}</button
        >{/if}
    </div>
  {/if}
  {@render footer?.()}
</article>
