<script lang="ts">
  import { storeClientState } from '$lib/anda/client/platform'
  import { createBookmarkJumpRequest, bookmarkJumpRequestStorageKey } from '../bookmark-jump'
  import { openAndaSidePanel } from '../dashboard/side-panel'
  import Inbox from '../brain/Inbox.svelte'
  import ChangeDialog from './ChangeDialog.svelte'
  import SetupDialog from './SetupDialog.svelte'
  import SearchPanel from './SearchPanel.svelte'
  import EntityPage from './EntityPage.svelte'
  import EntitySearch from './EntitySearch.svelte'
  import RecordCard from './RecordCard.svelte'
  import { entityName, learningLabel } from './labels'
  import WatchControl from './WatchControl.svelte'
  import { ANDA_BOT_SPACE_ID } from '../brain/api'
  import { loadConfigSettings } from '../config/api'
  import type { SettingsState } from '$lib/service-worker/types'
  import {
    MemoryApi,
    type ActivityPage,
    type ChangeKind,
    type Overview,
    type RecordPage
  } from './api'
  import type { MemoryRecord, RecordWatch } from './api'
  import { buttonClass, badgeClass } from '../ui'
  import { getMessage } from '$lib/i18n'
  import {
    ArrowLeft,
    ArrowUpRight,
    BrainCircuit,
    Check,
    ChevronRight,
    Copy,
    History,
    RefreshCw
  } from '@lucide/svelte'
  import { onMount } from 'svelte'

  let mode = $state<'home' | 'inbox' | 'entity'>('home')
  /** Entity pages opened from the home page, oldest first; null is the caller. */
  let trail = $state<Array<{ id: string | null; name: string }>>([])
  let entityRevision = $state(0)
  let settings = $state<SettingsState | null>(null)
  let overview = $state<Overview | null>(null)
  let activity = $state<ActivityPage | null>(null)
  let records = $state<RecordPage | null>(null)
  let recordError = $state('')
  let recordCursor = $state<string | null>(null)
  let activityCursor = $state<string | null>(null)
  let guideOpen = $state(false)
  let setupOpen = $state(false)
  let watches = $state<RecordWatch[]>([])
  let watchError = $state('')
  let watchesComplete = $state(true)
  let change = $state<{
    record: MemoryRecord | null
    kind: ChangeKind
    restored: boolean
  } | null>(null)
  let changeStorageKey = $state('')
  let hasPendingChange = $state(false)
  let error = $state('')
  let activityError = $state('')
  let busy = $state(false)
  let copied = $state(false)
  let generation = 0
  let timer: ReturnType<typeof setTimeout> | undefined
  let controller: AbortController | undefined
  let disposed = false

  const stateLabels: Record<string, string> = {
    recalled: getMessage('memoryState_recalled'),
    recall_failed: getMessage('memoryState_recall_failed'),
    reachable: getMessage('memoryState_reachable'),
    available: getMessage('memoryState_available'),
    not_configured: getMessage('memoryState_not_configured'),
    unauthorized: getMessage('memoryState_unauthorized'),
    forbidden: getMessage('memoryState_forbidden'),
    unavailable: getMessage('memoryState_unavailable'),
    timeout: getMessage('memoryState_timeout'),
    submitting: getMessage('memoryState_submitting'),
    accepted: getMessage('memoryState_accepted'),
    processing: getMessage('memoryState_processing'),
    completed: getMessage('memoryState_completed'),
    rejected: getMessage('memoryState_rejected'),
    failed: getMessage('memoryState_failed'),
    legacy_unattributed: getMessage('memoryState_legacy_unattributed'),
    unknown: getMessage('memoryState_unknown'),
    suppressed: getMessage('memoryState_suppressed')
  }
  const label = (state: string) => stateLabels[state] || stateLabels.unavailable
  const entitiesAvailable = $derived(overview?.capabilities.entities?.state === 'available')
  const canChange = $derived(
    !!changeStorageKey && overview?.capabilities.changes?.state === 'available'
  )

  function stop() {
    clearTimeout(timer)
    controller?.abort()
    generation++
  }

  async function refresh(cursor: string | null = activityCursor) {
    if (!settings || mode !== 'home' || document.hidden) return
    stop()
    const current = generation
    controller = new AbortController()
    const signal = controller.signal
    const api = new MemoryApi(settings)
    busy = true
    error = ''
    activityError = ''
    recordError = ''
    activityCursor = cursor
    try {
      const next = await api.overview(signal)
      if (disposed || current !== generation) return
      overview = next
      changeStorageKey = next.caller
        ? `anda-memory-change/v1/${JSON.stringify([settings.baseUrl, ANDA_BOT_SPACE_ID, next.caller])}`
        : ''
      try {
        hasPendingChange = !!changeStorageKey && !!localStorage.getItem(changeStorageKey)
      } catch {
        hasPendingChange = false
      }
      const [activityResult, recordResult, watchResult] = await Promise.allSettled([
        next.capabilities.activity?.state === 'available'
          ? api.activity(cursor, signal)
          : Promise.resolve(null),
        next.capabilities.records?.state === 'available'
          ? api.records(recordCursor, signal)
          : Promise.resolve(null),
        next.capabilities.record_watches?.state === 'available'
          ? api.watches(signal)
          : Promise.resolve(null)
      ])
      if (disposed || current !== generation) return
      if (activityResult.status === 'fulfilled') activity = activityResult.value
      else activityError = String(activityResult.reason)
      if (recordResult.status === 'fulfilled') records = recordResult.value
      else recordError = String(recordResult.reason)
      if (watchResult.status === 'fulfilled') {
        watches = watchResult.value?.items || []
        watchesComplete = watchResult.value?.complete ?? true
        watchError = ''
      } else watchError = String(watchResult.reason)
    } catch (e) {
      if (current === generation) {
        overview = null
        activity = null
        records = null
        error = String(e)
      }
    } finally {
      if (current === generation) {
        busy = false
        const pending = activity?.items.some(
          (item) =>
            ![
              'completed',
              'failed',
              'rejected',
              'legacy_unattributed',
              'suppressed',
              'recalled',
              'recall_failed'
            ].includes(item.state)
        )
        if (
          mode === 'home' &&
          !document.hidden &&
          (pending || activity?.partial_reason || overview?.memory.formation_active)
        ) {
          const active = activity?.items.some((item) =>
            ['submitting', 'accepted', 'processing'].includes(item.state)
          )
          timer = setTimeout(() => void refresh(), active ? 5000 : 60000)
        }
      }
    }
  }
  async function bindSettings() {
    stop()
    const current = generation
    settings = null
    setupOpen = false
    watches = []
    watchError = ''
    change = null
    changeStorageKey = ''
    hasPendingChange = false
    overview = null
    activity = null
    records = null
    recordCursor = null
    activityCursor = null
    try {
      const saved = await loadConfigSettings()
      if (disposed || current !== generation) return
      settings = saved
      await refresh()
    } catch (e) {
      if (current === generation) error = String(e)
    }
  }
  function select(next: typeof mode) {
    stop()
    busy = false
    mode = next
    if (next !== 'entity') trail = []
    if (next === 'home') void bindSettings()
  }
  function openEntity(id: string | null, name: string) {
    const index = trail.findIndex((item) => item.id === id)
    trail =
      mode !== 'entity'
        ? [{ id, name }]
        : index >= 0
          ? trail.slice(0, index + 1)
          : [...trail, { id, name }].slice(-8)
    if (mode !== 'entity') select('entity')
  }
  function editRecord(record: MemoryRecord, kind: ChangeKind) {
    change = { record, kind, restored: false }
  }
  async function openSource(source: MemoryRecord['sources'][number]) {
    if (!source.conversation || !source.index || !source.source) return
    const conversation = Number(source.conversation)
    if (!Number.isSafeInteger(conversation)) return
    try {
      await storeClientState({
        [bookmarkJumpRequestStorageKey]: createBookmarkJumpRequest({
          message_id: `m-${source.conversation}-${source.index}`,
          conversation,
          source: source.source
        })
      })
      await openAndaSidePanel()
    } catch (e) {
      error = String(e)
    }
  }
  async function copyExample() {
    try {
      await navigator.clipboard.writeText(getMessage('memoryExample'))
      copied = true
    } catch (e) {
      error = String(e)
    }
  }
  onMount(() => {
    void bindSettings()
    const visibility = () => {
      if (document.hidden) {
        stop()
        busy = false
      } else if (mode === 'home') void refresh()
    }
    const changed = (changes: Record<string, unknown>) => {
      if (['baseUrl', 'token'].some((key) => key in changes)) {
        mode = 'home'
        void bindSettings()
      }
    }
    document.addEventListener('visibilitychange', visibility)
    if (typeof chrome !== 'undefined') chrome.storage?.onChanged?.addListener(changed)
    return () => {
      disposed = true
      stop()
      document.removeEventListener('visibilitychange', visibility)
      if (typeof chrome !== 'undefined') chrome.storage?.onChanged?.removeListener(changed)
    }
  })
</script>

{#if mode === 'inbox'}
  <div class="flex h-full min-h-0 flex-col">
    <div class="border-b border-border p-3">
      <button class={buttonClass('ghost', 'sm')} onclick={() => select('home')}
        ><ArrowLeft class="size-4" />{getMessage('memoryTitle')}</button
      >
    </div>
    <div class="min-h-0 flex-1">
      {#if settings}<Inbox {settings} />{/if}
    </div>
  </div>
{:else if mode === 'entity'}
  {@const current = trail[trail.length - 1]}
  <div class="flex h-full min-h-0 flex-col">
    <nav
      class="flex min-w-0 items-center gap-1 overflow-x-auto border-b border-border p-3"
      aria-label={getMessage('memoryEntityPath')}
    >
      <button class={buttonClass('ghost', 'sm', 'shrink-0')} onclick={() => select('home')}
        ><ArrowLeft class="size-4" />{getMessage('memoryTitle')}</button
      >
      {#each trail as item, index (item.id ?? '')}
        <ChevronRight class="size-3.5 shrink-0 text-muted-foreground" aria-hidden="true" />
        {#if index === trail.length - 1}
          <span class="max-w-48 truncate px-1 text-sm font-medium" aria-current="page"
            >{item.name}</span
          >
        {:else}
          <button
            class={buttonClass('ghost', 'sm', 'max-w-40 shrink-0 truncate')}
            onclick={() => openEntity(item.id, item.name)}>{item.name}</button
          >
        {/if}
      {/each}
    </nav>
    <div class="min-h-0 flex-1 overflow-y-auto px-5 py-7 sm:px-10 sm:py-10">
      {#if settings && current}
        {#key current.id}
          <EntityPage
            api={new MemoryApi(settings)}
            id={current.id}
            revision={entityRevision}
            changes={canChange}
            changeDisabled={hasPendingChange}
            onchange={editRecord}
            onsource={openSource}
            onopen={openEntity}
            onloaded={(entity) => {
              const last = trail[trail.length - 1]
              if (last && last.id === current.id)
                last.name = entityName(entity)
            }}
          />
        {/key}
      {/if}
    </div>
  </div>
{:else}
  <section
    class="h-full overflow-y-auto px-5 py-7 sm:px-10 sm:py-10"
    aria-label={getMessage('memoryTitle')}
  >
    <div class="mx-auto max-w-3xl">
      <header class="flex flex-wrap items-start justify-between gap-4">
        <div class="max-w-xl">
          <p class="mb-3 flex items-center gap-2 text-xs text-muted-foreground">
            <BrainCircuit class="size-4" />ANDA
          </p>
          <h1 class="text-2xl font-semibold tracking-tight">{getMessage('memoryTitle')}</h1>
          <p class="mt-3 text-sm leading-relaxed text-muted-foreground">
            {getMessage('memoryIntro')}
          </p>
        </div>
        <button
          class={buttonClass('outline', 'sm')}
          disabled={busy || !settings}
          onclick={() => refresh()}
          ><RefreshCw class={busy ? 'size-4 animate-spin' : 'size-4'} />{getMessage(
            'refresh'
          )}</button
        >
      </header>
      {#if error}<p
          role="alert"
          class="mt-6 break-words rounded-md border border-destructive/30 p-3 text-sm text-destructive"
        >
          {getMessage('memoryConnectionHelp')}<br />{error}
        </p>{/if}
      {#if overview}
        <div class="my-7 grid gap-5 border-y border-border py-5 sm:grid-cols-2" aria-live="polite">
          <div>
            <p class="text-xs text-muted-foreground">{getMessage('memoryService')}</p>
            <p class="mt-2 text-sm font-medium">{label(overview.memory.state)}</p>
            {#if overview.memory.formation_active}<p class="mt-1 text-xs text-muted-foreground">
                {getMessage('memoryState_processing')}
              </p>{/if}
          </div>
          <div>
            <p class="text-xs text-muted-foreground">{getMessage('brainInbox')}</p>
            <p class="mt-2 text-sm font-medium">{label(overview.inbox.state)}</p>
            {#if overview.inbox.state === 'available'}<button
                class={buttonClass('ghost', 'xs', 'mt-1')}
                onclick={() => select('inbox')}
                >{getMessage('memoryOpenInbox')}<ArrowUpRight class="size-3" /></button
              >{/if}
            {#if overview.inbox.state === 'not_configured' && overview.capabilities.inbox_setup?.state === 'available'}
              <button class={buttonClass('ghost', 'xs', 'mt-1')} onclick={() => (setupOpen = true)}
                >{getMessage('memorySetupTitle')}</button
              >
            {/if}
          </div>
        </div>
      {/if}
      {#if settings && overview?.capabilities.search?.state === 'available'}
        {#key changeStorageKey}<SearchPanel api={new MemoryApi(settings)} />{/key}
      {/if}
      {#if settings && entitiesAvailable}
        {#key changeStorageKey}<EntitySearch
            api={new MemoryApi(settings)}
            onopen={openEntity}
          />{/key}
      {/if}
      {#if hasPendingChange && changeStorageKey}<button
          class={buttonClass('outline', 'sm', 'mb-4')}
          onclick={() => {
            let kind: ChangeKind = 'correct'
            try {
              kind =
                JSON.parse(localStorage.getItem(changeStorageKey) || '{}').input?.kind || 'correct'
            } catch {}
            change = { record: null, kind, restored: true }
          }}>{getMessage('memoryResumeChange')}</button
        >{/if}
      {#if guideOpen || !records?.items.length}
        <section class="my-7 rounded-md bg-muted/40 p-5" aria-label={getMessage('memoryTry')}>
          <h2 class="text-sm font-semibold">{getMessage('memoryTry')}</h2>
          <p class="mt-2 text-sm leading-relaxed">{getMessage('memoryExample')}</p>
          <div class="mt-4 flex flex-wrap items-center gap-3">
            <button class={buttonClass('outline', 'sm')} onclick={copyExample}
              >{#if copied}<Check class="size-4" />{:else}<Copy class="size-4" />{/if}{getMessage(
                copied ? 'memoryCopied' : 'memoryCopy'
              )}</button
            >
            <p class="text-xs leading-relaxed text-muted-foreground">
              {getMessage('memoryTryHint')}
            </p>
          </div>
        </section>
      {/if}
      {#if records || recordError}
        <section class="my-7" aria-label={getMessage('memoryRecords')}>
          <h2 class="text-sm font-semibold">{getMessage('memoryRecords')}</h2>
          {#if recordError}<p role="alert" class="mt-3 break-words text-sm text-destructive">
              {recordError}
            </p>{/if}
          {#each records?.items || [] as record (record.id)}
            <RecordCard
              {record}
              changes={canChange}
              changeDisabled={hasPendingChange}
              onchange={(kind) => editRecord(record, kind)}
              onsource={openSource}
              onentity={entitiesAvailable ? openEntity : undefined}
            >
              {#snippet footer()}
                {#if settings && changeStorageKey && overview?.capabilities.record_watches?.state === 'available'}
                  {#key changeStorageKey}<WatchControl
                      api={new MemoryApi(settings)}
                      canCreate={watchesComplete && !watchError && record.state !== 'archived'}
                      recordId={record.id}
                      scope={changeStorageKey}
                      existing={watches.find(
                        (watch) => watch.target_id === record.id && watch.state !== 'cancelled'
                      )}
                      onchanged={() => void refresh()}
                    />{/key}
                {/if}
              {/snippet}
            </RecordCard>
          {/each}
          {#if watchError}<p role="alert" class="mt-3 text-xs text-destructive">
              {watchError}
            </p>{/if}
          {#if !watchesComplete}<p class="mt-3 text-xs text-muted-foreground">
              {getMessage('memoryPartial')}
            </p>{/if}
          {#if records?.items.length === 0 && !recordError}<p
              class="mt-4 text-sm text-muted-foreground"
            >
              {getMessage('memoryNoRecords')}
            </p>{/if}
          {#if records?.partial_reason}<p class="mt-3 text-xs text-muted-foreground">
              {#if records.partial_reason === 'response_size_limit'}
                {getMessage('memoryPageSizeLimit')}
              {:else}
                {getMessage('memorySourceUnavailable')}
              {/if}
            </p>{/if}
          {#if records?.next_cursor}<button
              class={buttonClass('outline', 'sm', 'mt-4')}
              disabled={busy}
              onclick={() => {
                recordCursor = records?.next_cursor || null
                void refresh()
              }}>{getMessage('brainNextPage')}</button
            >{/if}
        </section>
      {/if}
      <section aria-label={getMessage('memoryActivity')}>
        <h2 class="flex items-center gap-2 text-sm font-semibold">
          <History class="size-4" />{getMessage('memoryActivity')}
        </h2>
        <p class="mt-2 text-xs leading-relaxed text-muted-foreground">
          {getMessage('memoryEvidenceHint')}
        </p>
        {#if activityError}<p role="alert" class="my-4 break-words text-sm text-destructive">
            {activityError}
          </p>{/if}
        {#if activity?.items.length}
          <div class="mt-4 divide-y divide-border">
            {#each activity.items as item (item.id)}
              <article class="py-4">
                <div class="flex flex-wrap items-center justify-between gap-2">
                  <p class="text-sm font-medium">
                    {getMessage('memoryConversation')} #{item.conversation}
                  </p>
                  <span class={badgeClass('outline')}>{label(item.state)}</span>
                </div>
                <p class="mt-2 text-xs text-muted-foreground">
                  {new Date(item.submitted_at).toLocaleString()}{#if item.stale}
                    · {getMessage('memoryStale')}{/if}
                </p>
              </article>
            {/each}
          </div>
        {:else if activity && !busy}<p class="py-7 text-sm text-muted-foreground">
            {getMessage('memoryNoActivity')}
          </p>{/if}
        {#if activity?.partial_reason}<p class="mt-3 text-xs text-muted-foreground">
            {getMessage('memoryPartial')}
          </p>{/if}
        {#if activity?.next_cursor}<button
            class={buttonClass('outline', 'sm', 'mt-4')}
            disabled={busy}
            onclick={() => refresh(activity?.next_cursor)}>{getMessage('brainNextPage')}</button
          >{/if}
      </section>
      <footer
        class="mt-8 border-t border-border pt-5 text-xs leading-relaxed text-muted-foreground"
      >
        {getMessage('memoryPrivacyHint')}
        <button class={buttonClass('ghost', 'xs', 'mt-3')} onclick={() => (guideOpen = !guideOpen)}
          >{getMessage('memoryTry')}</button
        >
        <details class="mt-3">
          <summary class="cursor-pointer">{getMessage('memoryLearningTitle')}</summary>
          <p class="mt-3">{getMessage('memoryLearningScope')}</p>
          <p class="mt-2">{learningLabel(overview?.learning?.state || 'unavailable')}</p>
          <p class="mt-2">{getMessage('memoryEvaluationCli')}</p>
          <code class="mt-2 block">anda memory evaluate plan --help</code>
        </details>
        <details class="mt-3">
          <summary class="cursor-pointer">{getMessage('memoryTechnicalDetails')}</summary>
          <pre class="mt-3 overflow-auto whitespace-pre-wrap break-all">{JSON.stringify(
              overview,
              null,
              2
            )}</pre>
        </details>
      </footer>
    </div>
  </section>
{/if}

{#if change && settings && changeStorageKey}
  {#key changeStorageKey}
    <ChangeDialog
      api={new MemoryApi(settings)}
      record={change.record}
      kind={change.kind}
      restored={change.restored}
      storageKey={changeStorageKey}
      onclose={() => {
        change = null
        try {
          hasPendingChange = !!localStorage.getItem(changeStorageKey)
        } catch {
          hasPendingChange = false
        }
      }}
      onchanged={() => {
        hasPendingChange = false
        entityRevision++
        void refresh()
      }}
    />
  {/key}
{/if}

{#if setupOpen && settings}
  {#key changeStorageKey}<SetupDialog
      api={new MemoryApi(settings)}
      onclose={() => {
        setupOpen = false
        void refresh()
      }}
    />{/key}
{/if}
