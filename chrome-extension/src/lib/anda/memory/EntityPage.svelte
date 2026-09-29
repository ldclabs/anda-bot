<script lang="ts">
  import { onDestroy, untrack } from 'svelte'
  import { getMessage } from '$lib/i18n'
  import { badgeClass, buttonClass } from '../ui'
  import type {
    ChangeKind,
    EntityClaim,
    MemoryApi,
    MemoryApiError,
    MemoryEntity,
    MemoryRecord
  } from './api'
  import { beliefKey, groupClaims, isCurrent } from './entity'
  import EntityGraph from './EntityGraph.svelte'
  import RecordCard from './RecordCard.svelte'

  let {
    api,
    id,
    revision = 0,
    changes = false,
    changeDisabled = false,
    onchange,
    onsource,
    onopen,
    onloaded
  }: {
    api: MemoryApi
    /** A Concept id, or null for the caller. */
    id: string | null
    /** Bumped after a confirmed change to reload the page. */
    revision?: number
    changes?: boolean
    changeDisabled?: boolean
    onchange: (record: MemoryRecord, kind: ChangeKind) => void
    onsource: (source: MemoryRecord['sources'][number]) => void
    onopen: (id: string, label: string) => void
    onloaded?: (entity: MemoryEntity) => void
  } = $props()

  let entity = $state<MemoryEntity | null>(null)
  let claims = $state<EntityClaim[]>([])
  let cursor = $state<string | null>(null)
  let partial = $state<string | null>(null)
  let missing = $state(false)
  let error = $state('')
  let busy = $state(false)
  let generation = 0
  let controller: AbortController | undefined

  const beliefLabels: Record<string, string> = {
    memoryBelief_accepted: getMessage('memoryBelief_accepted'),
    memoryBelief_contested: getMessage('memoryBelief_contested'),
    memoryBelief_uncertain: getMessage('memoryBelief_uncertain'),
    memoryBelief_rejected: getMessage('memoryBelief_rejected'),
    memoryBelief_insufficient: getMessage('memoryBelief_insufficient'),
    memoryBelief_ended: getMessage('memoryBelief_ended'),
    memoryBelief_excluded: getMessage('memoryBelief_excluded')
  }
  const groups = $derived(groupClaims(claims))
  const name = $derived(entity?.about_owner ? getMessage('memoryYou') : entity?.name || '')
  const current = $derived(claims.filter(isCurrent).length)

  async function load(next: string | null) {
    controller?.abort()
    const version = ++generation
    controller = new AbortController()
    busy = true
    error = ''
    if (!next) {
      entity = null
      claims = []
      cursor = null
      partial = null
      missing = false
    }
    try {
      const page = await api.entity(id, next, controller.signal)
      if (version !== generation) return
      entity = page.entity
      const seen = new Set(claims.map((claim) => `${claim.direction}:${claim.record.id}`))
      claims = [
        ...claims,
        ...page.items.filter((claim) => !seen.has(`${claim.direction}:${claim.record.id}`))
      ]
      cursor = page.next_cursor
      partial = page.partial_reason
      onloaded?.(page.entity)
    } catch (e) {
      if (version !== generation) return
      if ((e as MemoryApiError).code === 'not_found') missing = true
      else error = String(e)
    } finally {
      if (version === generation) busy = false
    }
  }

  $effect(() => {
    void id
    void revision
    untrack(() => void load(null))
  })
  onDestroy(() => {
    generation++
    controller?.abort()
  })
</script>

{#snippet belief(claim: EntityClaim)}
  {@const key = beliefKey(claim)}
  {#if key}<span
      class={badgeClass(
        claim.belief?.status === 'contested'
          ? 'destructive'
          : key === 'memoryBelief_accepted'
            ? 'secondary'
            : 'outline'
      )}>{beliefLabels[key]}</span
    >{/if}
{/snippet}

{#snippet card(claim: EntityClaim)}
  <RecordCard
    record={claim.record}
    {changes}
    {changeDisabled}
    onchange={(kind) => onchange(claim.record, kind)}
    {onsource}
    onentity={onopen}
  >
    {#snippet status()}{@render belief(claim)}{/snippet}
  </RecordCard>
{/snippet}

<section class="mx-auto max-w-3xl" aria-busy={busy} aria-label={name || getMessage('memoryTitle')}>
  {#if entity}
    <header>
      <p class="flex flex-wrap items-center gap-2 text-xs text-muted-foreground">
        <span class={badgeClass('outline')}>{entity.type}</span>
        {#if entity.about_owner}<span>{getMessage('memoryAboutYou')}</span>{/if}
      </p>
      <h1 class="mt-3 text-2xl font-semibold tracking-tight break-words">{entity.name}</h1>
      <p class="mt-3 text-sm leading-relaxed text-muted-foreground">
        {getMessage('memoryEntityIntro')}
      </p>
      {#if claims.length}<p class="mt-2 text-xs text-muted-foreground">
          {getMessage('memoryEntityShown', [String(claims.length), String(current)])}
        </p>{/if}
    </header>
    <EntityGraph {entity} {claims} {onopen} />
    {#each groups as group (group.key)}
      <section class="my-7" aria-label={group.predicate}>
        <h2 class="text-sm font-semibold">
          {group.direction === 'outgoing'
            ? `${name} · ${group.predicate}`
            : `… · ${group.predicate} · ${name}`}
        </h2>
        {#each group.current as claim (claim.record.id)}{@render card(claim)}{/each}
        {#if group.earlier.length}
          <details class="mt-3" open={!group.current.length}>
            <summary class="cursor-pointer text-xs text-muted-foreground"
              >{getMessage('memoryEntityEarlier', String(group.earlier.length))}</summary
            >
            {#each group.earlier as claim (claim.record.id)}{@render card(claim)}{/each}
          </details>
        {/if}
      </section>
    {/each}
    {#if !claims.length && !busy}<p class="my-7 text-sm text-muted-foreground">
        {getMessage(entity.about_owner ? 'memoryEntitySelfEmpty' : 'memoryEntityNotFound')}
      </p>{/if}
    {#if partial}<p class="mt-3 text-xs text-muted-foreground">
        {#if partial === 'response_size_limit'}
          {getMessage('memoryPageSizeLimit')}
        {:else if partial === 'scan_limit'}
          {getMessage('memoryEntityScanLimit')}
        {:else}
          {getMessage('memorySourceUnavailable')}
        {/if}
      </p>{/if}
    {#if cursor}<button
        class={buttonClass('outline', 'sm', 'mt-4')}
        disabled={busy}
        onclick={() => load(cursor)}>{getMessage('brainNextPage')}</button
      >{/if}
    <details class="mt-8 text-xs text-muted-foreground">
      <summary class="cursor-pointer">{getMessage('memoryBeliefTitle')}</summary>
      <p class="mt-2 leading-relaxed">{getMessage('memoryBeliefHint')}</p>
    </details>
  {:else if missing}
    <p class="text-sm text-muted-foreground">
      {getMessage(id === null ? 'memoryEntitySelfEmpty' : 'memoryEntityNotFound')}
    </p>
  {:else if busy}
    <p class="text-sm text-muted-foreground">{getMessage('loading')}</p>
  {/if}
  {#if error}<p
      role="alert"
      class="mt-4 rounded-md border border-destructive/30 p-3 text-sm break-words text-destructive"
    >
      {error}
    </p>{/if}
</section>
