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
  import { cn } from '$lib/utils'
  import {
    BrainCircuit,
    Check,
    ChevronRight,
    Copy,
    Cpu,
    History,
    Inbox as InboxIcon,
    LoaderCircle,
    RefreshCw,
    ScrollText,
    Sparkles,
    Tag,
    UserRound
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
  /** The entity pages the navigation always offers; a search adds its pick. */
  const roots: Array<{ id: string | null; name: string; label: string; icon: typeof Cpu }> = [
    {
      id: null,
      name: getMessage('memoryYou'),
      label: getMessage('memoryAboutYou'),
      icon: UserRound
    },
    {
      id: '$self',
      name: getMessage('memoryBrainSelf'),
      label: getMessage('memoryBrainSelf'),
      icon: Sparkles
    },
    {
      id: '$system',
      name: getMessage('memoryBrainSystem'),
      label: getMessage('memoryBrainSystem'),
      icon: Cpu
    }
  ]
  const searchedRoot = $derived(
    mode === 'entity' && trail[0] && !roots.some((root) => root.id === trail[0].id)
      ? trail[0]
      : null
  )
  const serviceTone = $derived(
    !overview
      ? error
        ? 'bg-destructive'
        : 'bg-muted-foreground/40'
      : ['reachable', 'available'].includes(overview.memory.state)
        ? 'bg-emerald-500'
        : overview.memory.state === 'not_configured'
          ? 'bg-muted-foreground/40'
          : 'bg-amber-500'
  )
  function navItemClass(active: boolean): string {
    return active
      ? 'bg-background text-foreground shadow-xs'
      : 'text-muted-foreground hover:bg-background/70 hover:text-foreground'
  }

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
    // The navigation keeps the overview on screen, so coming back refreshes it
    // in place instead of clearing it.
    if (next === 'home') void (settings ? refresh() : bindSettings())
  }
  /** Starts a new path, from the navigation or a search. */
  function openRoot(id: string | null, name: string) {
    trail = [{ id, name }]
    if (mode !== 'entity') select('entity')
  }
  /** Follows a link: a record or graph node extends the path. */
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

{#snippet navItem(
  Icon: typeof Cpu,
  text: string,
  active: boolean,
  onclick: () => void,
  meta = '',
  disabled = false
)}
  <button
    type="button"
    class={cn(
      'flex min-w-0 items-center gap-2 rounded-md px-2.5 py-2 text-left text-sm transition disabled:pointer-events-none disabled:opacity-60',
      navItemClass(active)
    )}
    aria-current={active ? 'page' : undefined}
    {disabled}
    {onclick}
  >
    <Icon class="size-3.5 shrink-0" />
    <span class="min-w-0 flex-1 truncate">{text}</span>
    {#if meta}<span class="max-w-[45%] shrink-0 truncate text-[11px] text-muted-foreground"
        >{meta}</span
      >{/if}
  </button>
{/snippet}

<div class="grid h-full min-h-0 grid-cols-[15rem_minmax(0,1fr)] overflow-hidden">
  <aside
    class="grid min-h-0 grid-rows-[auto_minmax(0,1fr)_auto] border-r bg-sidebar/70"
    aria-label={getMessage('memoryTitle')}
  >
    <div class="border-b px-3 py-3">
      <h1 class="flex items-center gap-2 text-sm font-bold">
        <BrainCircuit class="size-4" />{getMessage('memoryTitle')}
      </h1>
      <p class="mt-1 text-xs leading-relaxed text-muted-foreground">
        {getMessage('memoryIntro')}
      </p>
    </div>

    <nav class="grid min-h-0 content-start gap-1 overflow-y-auto p-2">
      {@render navItem(ScrollText, getMessage('memoryRecords'), mode === 'home', () =>
        select('home')
      )}
      {#if overview?.inbox.state === 'available'}
        {@render navItem(
          InboxIcon,
          getMessage('brainInbox'),
          mode === 'inbox',
          () => select('inbox'),
          overview.inbox.visible_items ? String(overview.inbox.visible_items) : ''
        )}
      {:else if overview?.inbox.state === 'not_configured' && overview.capabilities.inbox_setup?.state === 'available'}
        {@render navItem(
          InboxIcon,
          getMessage('memorySetupTitle'),
          false,
          () => (setupOpen = true)
        )}
      {:else if overview}
        {@render navItem(
          InboxIcon,
          getMessage('brainInbox'),
          false,
          () => {},
          label(overview.inbox.state),
          true
        )}
      {/if}

      {#if settings && entitiesAvailable}
        <p class="mt-3 px-2.5 pb-1 text-[11px] font-semibold text-muted-foreground">
          {getMessage('memoryEntities')}
        </p>
        {#each roots as root (root.id ?? '')}
          {@render navItem(
            root.icon,
            root.label,
            mode === 'entity' && trail[0]?.id === root.id,
            () => openRoot(root.id, root.name)
          )}
        {/each}
        {#if searchedRoot}
          {@render navItem(Tag, searchedRoot.name, true, () =>
            openRoot(searchedRoot.id, searchedRoot.name)
          )}
        {/if}
        {#key changeStorageKey}<EntitySearch
            api={new MemoryApi(settings)}
            onopen={openRoot}
          />{/key}
      {/if}
    </nav>

    <div class="grid gap-1 border-t px-3 py-2.5 text-xs" aria-live="polite">
      <p class="flex min-w-0 items-center gap-2">
        <span class={cn('size-2 shrink-0 rounded-full', serviceTone)} aria-hidden="true"></span>
        <span class="truncate text-muted-foreground">{getMessage('memoryService')}</span>
        <span class="ml-auto shrink-0 truncate font-medium"
          >{overview
            ? label(overview.memory.state)
            : error
              ? label('unavailable')
              : getMessage('loading')}</span
        >
      </p>
      {#if overview?.memory.formation_active}
        <p class="flex items-center gap-2 text-muted-foreground">
          <LoaderCircle class="size-3 shrink-0 animate-spin" />
          <span class="truncate">{getMessage('memoryState_processing')}</span>
        </p>
      {/if}
    </div>
  </aside>

  <section class="grid min-h-0 min-w-0 grid-rows-[auto_minmax(0,1fr)]">
    {#if mode === 'inbox'}
      <div class="row-span-2 min-h-0">
        {#if settings}<Inbox {settings} />{/if}
      </div>
    {:else if mode === 'entity'}
      {@const current = trail[trail.length - 1]}
      <div class="flex min-h-14 min-w-0 items-center justify-between gap-3 border-b px-4 py-2">
        <nav
          class="flex min-w-0 items-center gap-1 overflow-x-auto"
          aria-label={getMessage('memoryEntityPath')}
        >
          {#each trail as item, index (item.id ?? '')}
            {#if index}<ChevronRight
                class="size-3.5 shrink-0 text-muted-foreground"
                aria-hidden="true"
              />{/if}
            {#if index === trail.length - 1}
              <span class="max-w-60 truncate px-1 text-base font-bold" aria-current="page"
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
        <button class={buttonClass('outline', 'sm', 'shrink-0')} onclick={() => entityRevision++}
          ><RefreshCw class="size-3.5" />{getMessage('refresh')}</button
        >
      </div>
      <div class="@container min-h-0 overflow-y-auto px-5 py-6 sm:px-8">
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
                if (last && last.id === current.id) last.name = entityName(entity)
              }}
            />
          {/key}
        {/if}
      </div>
    {:else}
      <div class="flex min-h-14 min-w-0 items-center justify-between gap-3 border-b px-4 py-2">
        <h2 class="min-w-0 truncate text-base font-bold">{getMessage('memoryRecords')}</h2>
        <button
          class={buttonClass('outline', 'sm', 'shrink-0')}
          disabled={busy || !settings}
          onclick={() => refresh()}
          ><RefreshCw class={busy ? 'size-3.5 animate-spin' : 'size-3.5'} />{getMessage(
            'refresh'
          )}</button
        >
      </div>
      <div class="@container min-h-0 overflow-y-auto" aria-label={getMessage('memoryTitle')}>
        <!-- Records and their processing activity side by side once the pane
             is wide enough; below that the activity follows the records. -->
        <div
          class="mx-auto grid max-w-6xl gap-x-8 px-5 py-6 sm:px-8 @4xl:grid-cols-[minmax(0,1fr)_minmax(16rem,20rem)]"
        >
          <div class="min-w-0">
            {#if error}<p
                role="alert"
                class="mb-6 rounded-md border border-destructive/30 p-3 text-sm break-words text-destructive"
              >
                {getMessage('memoryConnectionHelp')}<br />{error}
              </p>{/if}
            {#if settings && overview?.capabilities.search?.state === 'available'}
              {#key changeStorageKey}<SearchPanel api={new MemoryApi(settings)} />{/key}
            {/if}
            {#if hasPendingChange && changeStorageKey}<button
                class={buttonClass('outline', 'sm', 'mb-4')}
                onclick={() => {
                  let kind: ChangeKind = 'correct'
                  try {
                    kind =
                      JSON.parse(localStorage.getItem(changeStorageKey) || '{}').input?.kind ||
                      'correct'
                  } catch {}
                  change = { record: null, kind, restored: true }
                }}>{getMessage('memoryResumeChange')}</button
              >{/if}
            {#if guideOpen || !records?.items.length}
              <section class="mb-7 rounded-md bg-muted/40 p-5" aria-label={getMessage('memoryTry')}>
                <h3 class="text-sm font-semibold">{getMessage('memoryTry')}</h3>
                <p class="mt-2 text-sm leading-relaxed">{getMessage('memoryExample')}</p>
                <div class="mt-4 flex flex-wrap items-center gap-3">
                  <button class={buttonClass('outline', 'sm')} onclick={copyExample}
                    >{#if copied}<Check class="size-4" />{:else}<Copy
                        class="size-4"
                      />{/if}{getMessage(copied ? 'memoryCopied' : 'memoryCopy')}</button
                  >
                  <p class="text-xs leading-relaxed text-muted-foreground">
                    {getMessage('memoryTryHint')}
                  </p>
                </div>
              </section>
            {/if}
            {#if records || recordError}
              <section aria-label={getMessage('memoryRecords')}>
                {#if recordError}<p role="alert" class="mb-3 text-sm break-words text-destructive">
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
                            canCreate={watchesComplete &&
                              !watchError &&
                              record.state !== 'archived'}
                            recordId={record.id}
                            scope={changeStorageKey}
                            existing={watches.find(
                              (watch) =>
                                watch.target_id === record.id && watch.state !== 'cancelled'
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
                    class="text-sm text-muted-foreground"
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
          </div>

          <div
            class="mt-8 min-w-0 border-t pt-6 @4xl:mt-0 @4xl:border-t-0 @4xl:border-l @4xl:pt-0 @4xl:pl-8"
          >
            <section aria-label={getMessage('memoryActivity')}>
              <h3 class="flex items-center gap-2 text-sm font-semibold">
                <History class="size-4" />{getMessage('memoryActivity')}
              </h3>
              <p class="mt-2 text-xs leading-relaxed text-muted-foreground">
                {getMessage('memoryEvidenceHint')}
              </p>
              {#if activityError}<p role="alert" class="my-4 text-sm break-words text-destructive">
                  {activityError}
                </p>{/if}
              {#if activity?.items.length}
                <div class="mt-2 divide-y divide-border">
                  {#each activity.items as item (item.id)}
                    <article class="py-3">
                      <div class="flex flex-wrap items-center justify-between gap-2">
                        <p class="text-sm font-medium">
                          {getMessage('memoryConversation')} #{item.conversation}
                        </p>
                        <span class={badgeClass('outline')}>{label(item.state)}</span>
                      </div>
                      <p class="mt-1.5 text-xs text-muted-foreground">
                        {new Date(item.submitted_at).toLocaleString()}{#if item.stale}
                          · {getMessage('memoryStale')}{/if}
                      </p>
                    </article>
                  {/each}
                </div>
              {:else if activity && !busy}<p class="py-5 text-sm text-muted-foreground">
                  {getMessage('memoryNoActivity')}
                </p>{/if}
              {#if activity?.partial_reason}<p class="mt-3 text-xs text-muted-foreground">
                  {getMessage('memoryPartial')}
                </p>{/if}
              {#if activity?.next_cursor}<button
                  class={buttonClass('outline', 'sm', 'mt-4')}
                  disabled={busy}
                  onclick={() => refresh(activity?.next_cursor)}
                  >{getMessage('brainNextPage')}</button
                >{/if}
            </section>
            <footer
              class="mt-8 border-t border-border pt-5 text-xs leading-relaxed text-muted-foreground"
            >
              {getMessage('memoryPrivacyHint')}
              <button
                class={buttonClass('ghost', 'xs', 'mt-3')}
                onclick={() => (guideOpen = !guideOpen)}>{getMessage('memoryTry')}</button
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
                <pre class="mt-3 overflow-auto break-all whitespace-pre-wrap">{JSON.stringify(
                    overview,
                    null,
                    2
                  )}</pre>
              </details>
            </footer>
          </div>
        </div>
      </div>
    {/if}
  </section>
</div>

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
