<script lang="ts">
  import { storeClientState } from '$lib/anda/client/platform'
  import { useAndaClient } from '$lib/anda/client/context'
  const andaClient = useAndaClient()
  import type {
    ManagedSkill,
    ManagedSkillDetail,
    SkillFileEntry,
    SkillSourceInfo,
    SkillSourceKind
  } from '$lib/anda/client/types'
  import { badgeClass, buttonClass, inputClass, textareaClass } from '$lib/anda/ui'
  import DropdownMenu from '$lib/anda/DropdownMenu.svelte'
  import Modal from '$lib/anda/Modal.svelte'
  import { openAndaSidePanel } from '$lib/anda/dashboard/side-panel'
  import { getMessage } from '$lib/i18n'
  import { escapeHtml } from '$lib/utils/format'
  import { createPromptDraftRequest, promptDraftRequestStorageKey } from '$lib/anda/prompt-draft'
  import { errorToMessage } from '$lib/service-worker/settings'
  import { cn } from '$lib/utils'
  import Prism from '$lib/utils/prismjs'
  import {
    Activity,
    AlertTriangle,
    Ban,
    CheckCircle2,
    Copy,
    FileCode2,
    FileText,
    Folder,
    LoaderCircle,
    RefreshCw,
    Search,
    Send,
    Trash2,
    WandSparkles,
    X
  } from '@lucide/svelte'
  import { onMount } from 'svelte'

  type SourceFilter = 'all' | SkillSourceKind
  type SkillStatus = 'active' | 'disabled' | 'shadowed' | 'error' | 'inactive'
  type StatusFilter = 'all' | Exclude<SkillStatus, 'inactive'>
  type DetailTab = 'overview' | 'files' | 'optimize'
  /** One library operation at a time; `load` is the first fetch. */
  type BusyAction = '' | 'load' | 'reload' | 'clone' | 'toggle' | 'delete' | 'optimize'

  const SKILL_MD = 'SKILL.md'

  let skills = $state<ManagedSkill[]>([])
  let sources = $state<SkillSourceInfo[]>([])
  let selectedId = $state('')
  let detail = $state<ManagedSkillDetail | null>(null)
  let searchQuery = $state('')
  let sourceFilter = $state<SourceFilter>('all')
  let statusFilter = $state<StatusFilter>('all')
  let activeTab = $state<DetailTab>('overview')
  let busyAction = $state<BusyAction>('')
  let detailLoading = $state(false)
  let error = $state('')
  let notice = $state('')
  let selectedFilePath = $state(SKILL_MD)
  let viewedFileContent = $state('')
  let viewedFileTruncated = $state(false)
  let fileLoading = $state(false)
  let fileError = $state('')
  let optimizeGoal = $state('')
  let detailRequestId = 0
  let fileRequestId = 0

  const sourceLabels: Record<SkillSourceKind, string> = {
    personal: getMessage('skillSourcePersonal'),
    bundled: getMessage('skillSourceBundled'),
    shared: getMessage('skillSourceShared')
  }
  const statusLabels: Record<SkillStatus, string> = {
    active: getMessage('skillStatusActive'),
    disabled: getMessage('skillStatusDisabled'),
    shadowed: getMessage('skillStatusShadowed'),
    error: getMessage('skillStatusError'),
    inactive: getMessage('skillStatusInactive')
  }
  const statusFilterItems: { value: StatusFilter; label: string }[] = [
    { value: 'all', label: getMessage('allStatuses') },
    { value: 'active', label: statusLabels.active },
    { value: 'disabled', label: statusLabels.disabled },
    { value: 'shadowed', label: statusLabels.shadowed },
    { value: 'error', label: statusLabels.error }
  ]
  const detailTabs: { value: DetailTab; label: string }[] = [
    { value: 'overview', label: getMessage('skillOverview') },
    { value: 'files', label: getMessage('skillFiles') },
    { value: 'optimize', label: getMessage('skillOptimize') }
  ]
  /** File extension to a grammar registered in `$lib/utils/prismjs`. */
  const fileLanguages: Record<string, string> = {
    md: 'markdown',
    markdown: 'markdown',
    json: 'json',
    jsonc: 'json',
    json5: 'json5',
    yaml: 'yaml',
    yml: 'yaml',
    toml: 'toml',
    py: 'python',
    rs: 'rust',
    ts: 'typescript',
    mts: 'typescript',
    cts: 'typescript',
    tsx: 'tsx',
    js: 'javascript',
    mjs: 'javascript',
    cjs: 'javascript',
    jsx: 'jsx',
    sh: 'bash',
    bash: 'bash',
    zsh: 'bash',
    html: 'markup',
    xml: 'markup',
    svg: 'markup',
    css: 'css'
  }

  const compactNumberFormatter = new Intl.NumberFormat(undefined, {
    maximumFractionDigits: 1,
    notation: 'compact'
  })
  const numberFormatter = new Intl.NumberFormat()

  // Several directories can share a kind (more than one Shared folder), but
  // the filter offers each kind once.
  const sourceFilterItems = $derived<{ value: SourceFilter; label: string }[]>([
    { value: 'all', label: getMessage('allSources') },
    ...[...new Set(sources.map((source) => source.source))].map((kind) => ({
      value: kind,
      label: `${sourceLabels[kind] || kind} (${skills.filter((skill) => skill.source === kind).length})`
    }))
  ])
  const incompleteSources = $derived(sources.filter((source) => source.diagnostics?.length))
  const visibleSkills = $derived.by(() => {
    const query = searchQuery.trim().toLowerCase()
    return skills.filter(
      (skill) =>
        (sourceFilter === 'all' || skill.source === sourceFilter) &&
        (statusFilter === 'all' || skillStatus(skill) === statusFilter) &&
        (!query ||
          skill.name.toLowerCase().includes(query) ||
          skill.description.toLowerCase().includes(query))
    )
  })
  const selectedSkill = $derived(skills.find((skill) => skill.id === selectedId) || null)
  const selectedFile = $derived(
    detail?.files.find((file) => file.path === selectedFilePath) || null
  )
  const fileCount = $derived(detail?.files.filter((file) => file.kind === 'file').length ?? 0)
  const selectedFileContent = $derived(
    selectedFilePath === SKILL_MD ? detail?.content || '' : viewedFileContent
  )
  const selectedFileLanguage = $derived(skillFileLanguage(selectedFilePath))
  const highlightedFileContent = $derived(
    highlightSkillFileContent(selectedFileContent, selectedFileLanguage)
  )
  const optimizationBusy = $derived(
    busyAction === 'optimize' ||
      andaClient.sending ||
      Boolean(andaClient.activeChannel?.sending) ||
      ['sending', 'submitted', 'working', 'connecting', 'reconnecting'].includes(andaClient.status)
  )
  const canOptimize = $derived(Boolean(detail && detail.source === 'personal'))
  const primaryActionLabel = $derived.by(() => {
    if (!selectedSkill) {
      return ''
    }
    if (selectedSkill.editable) {
      return getMessage('skillFiles')
    }
    return selectedSkill.source === 'shared'
      ? getMessage('skillImportToAnda')
      : getMessage('skillCustomize')
  })

  onMount(() => {
    andaClient
      .init({ conversations: false })
      .catch(() => undefined)
      .finally(() => {
        void run('load', async () => {
          await loadList()
          await showSkill(pickSelection(selectedId))
          return ''
        })
      })
  })

  /** Runs one library operation, reporting its outcome in the banner. */
  async function run(action: Exclude<BusyAction, ''>, work: () => Promise<string>) {
    if (busyAction) {
      return
    }
    busyAction = action
    error = ''
    notice = ''
    try {
      notice = await work()
    } catch (err) {
      error = errorToMessage(err)
    } finally {
      busyAction = ''
    }
  }

  /** Refreshes sources and skills; `nextSkills` is a list a mutation already returned. */
  async function loadList(nextSkills?: ManagedSkill[]) {
    const [nextSources, listed] = await Promise.all([
      andaClient.skills.listSources(),
      nextSkills ?? andaClient.skills.list(true)
    ])
    sources = nextSources
    skills = sortSkills(listed)
  }

  /** `preferredId` while it is listed, otherwise the first active skill. */
  function pickSelection(preferredId: string): string {
    return skills.some((skill) => skill.id === preferredId)
      ? preferredId
      : skills.find((skill) => skill.active)?.id || skills[0]?.id || ''
  }

  /**
   * Shows `id` in the detail pane. Switching skills shows a spinner; refreshing
   * the one already open swaps its detail in place.
   */
  async function showSkill(id: string) {
    const requestId = ++detailRequestId
    selectedId = id
    if (detail?.id !== id) {
      detail = null
      void openFile()
    }
    if (!id) {
      return
    }
    detailLoading = !detail
    try {
      const next = await andaClient.skills.get(id)
      if (requestId === detailRequestId) {
        showDetail(next)
      }
    } catch (err) {
      if (requestId === detailRequestId) {
        error = errorToMessage(err)
      }
    } finally {
      if (requestId === detailRequestId) {
        detailLoading = false
      }
    }
  }

  /** Puts `next` on screen, keeping the open file while the same skill still has it. */
  function showDetail(next: ManagedSkillDetail) {
    const keepFile =
      detail?.id === next.id &&
      next.files.some((file) => file.kind === 'file' && file.path === selectedFilePath)
    detailRequestId += 1
    detailLoading = false
    selectedId = next.id
    detail = next
    void openFile(keepFile ? selectedFilePath : SKILL_MD)
  }

  function selectSkill(id: string) {
    if (selectedId === id) {
      return
    }
    activeTab = 'overview'
    optimizeGoal = ''
    error = ''
    notice = ''
    void showSkill(id)
  }

  function reloadSkills() {
    void run('reload', async () => {
      await loadList(await andaClient.skills.reload())
      await showSkill(pickSelection(selectedId))
      return getMessage('skillsReloaded')
    })
  }

  function cloneSelected(nextTab: DetailTab = 'files') {
    const skill = selectedSkill
    if (!skill) {
      return
    }
    void run('clone', async () => {
      const cloned = await andaClient.skills.clone(skill.id)
      await loadList()
      showDetail(cloned)
      activeTab = nextTab
      return getMessage('skillCloned')
    })
  }

  function toggleSelected() {
    const skill = selectedSkill
    if (!skill) {
      return
    }
    const enabling = skill.disabled
    void run('toggle', async () => {
      await loadList(await andaClient.skills.setEnabled(skill.id, enabling))
      await showSkill(pickSelection(skill.id))
      return enabling ? getMessage('skillEnabled') : getMessage('skillDisabled')
    })
  }

  let deleteDialogOpen = $state(false)

  function deleteSelected() {
    deleteDialogOpen = false
    const skill = selectedSkill
    if (busyAction || !skill?.editable) {
      return
    }
    void run('delete', async () => {
      await andaClient.skills.deletePersonal(skill.id)
      await loadList()
      await showSkill(pickSelection(''))
      return getMessage('skillDeleted')
    })
  }

  /** Opens `path` in the Files tab; SKILL.md comes with the detail itself. */
  async function openFile(path = SKILL_MD) {
    const requestId = ++fileRequestId
    selectedFilePath = path
    viewedFileContent = ''
    viewedFileTruncated = false
    fileError = ''
    fileLoading = path !== SKILL_MD && Boolean(detail)
    if (!fileLoading || !detail) {
      return
    }
    try {
      const loaded = await andaClient.skills.getFile(detail.id, path)
      if (requestId === fileRequestId) {
        viewedFileContent = loaded.content
        viewedFileTruncated = loaded.truncated
      }
    } catch (err) {
      if (requestId === fileRequestId) {
        fileError = errorToMessage(err)
      }
    } finally {
      if (requestId === fileRequestId) {
        fileLoading = false
      }
    }
  }

  function selectSkillFile(file: SkillFileEntry) {
    if (file.kind === 'file' && file.path !== selectedFilePath) {
      void openFile(file.path)
    }
  }

  function sendOptimizationRequest() {
    const skill = detail
    if (!skill || !canOptimize || optimizationBusy) {
      return
    }
    void run('optimize', async () => {
      await storeClientState({
        [promptDraftRequestStorageKey]: createPromptDraftRequest(
          skillOptimizationPrompt(skill, optimizeGoal)
        )
      })
      await openAndaSidePanel()
      return getMessage('skillOptimizationPromptReady')
    })
  }

  function skillOptimizationPrompt(skill: ManagedSkillDetail, goal: string): string {
    const trimmedGoal = goal.trim() || 'Audit and improve this skill for real user workflows.'
    return [
      'Use $skill-creator to optimize this Anda Bot runtime skill.',
      '',
      `Skill directory: ${skill.directory}`,
      `Skill name: ${skill.name}`,
      '',
      'Optimization goal:',
      trimmedGoal,
      '',
      'Requirements:',
      '- Treat the whole skill directory as the artifact, not only SKILL.md.',
      '- Inspect SKILL.md and any scripts, references, agents metadata, or assets that affect the skill.',
      '- Update the skill files in place.',
      '- Preserve the skill name unless you need to ask before renaming it.',
      '- Validate the result with the relevant skill validation or focused checks, then summarize the changes.'
    ].join('\n')
  }

  function sortSkills(items: ManagedSkill[]): ManagedSkill[] {
    return [...items].sort(
      (left, right) =>
        Number(right.active) - Number(left.active) ||
        skillUsageRequests(right) - skillUsageRequests(left) ||
        left.priority - right.priority ||
        left.name.localeCompare(right.name) ||
        left.id.localeCompare(right.id)
    )
  }

  function skillUsageRequests(skill: ManagedSkill): number {
    return skill.usage?.requests ?? 0
  }

  /** The one status a skill is listed, labelled, and filtered by. */
  function skillStatus(skill: ManagedSkill): SkillStatus {
    if (skill.diagnostics.some((diagnostic) => diagnostic.severity === 'error')) {
      return 'error'
    }
    if (skill.disabled) {
      return 'disabled'
    }
    if (skill.shadowed_by) {
      return 'shadowed'
    }
    return skill.active ? 'active' : 'inactive'
  }

  function sourceLabel(source: { source: SkillSourceKind; source_label: string }): string {
    return sourceLabels[source.source] || source.source_label
  }

  function statusLine(skill: ManagedSkill): string {
    return `${sourceLabel(skill)} / ${statusLabels[skillStatus(skill)]} / ${usageCallsText(skill)}`
  }

  function formatSize(size?: number | null): string {
    if (!size) {
      return ''
    }
    if (size < 1024) {
      return `${size} B`
    }
    return `${(size / 1024).toFixed(1)} KB`
  }

  function formatNumber(value: number): string {
    return numberFormatter.format(value)
  }

  function formatCompactNumber(value: number): string {
    return compactNumberFormatter.format(value)
  }

  function usageCallsText(skill: ManagedSkill, compact = false): string {
    const requests = skill.usage?.requests ?? 0
    if (!requests) {
      return getMessage('skillUsageUnused')
    }
    return getMessage(
      'skillUsageCalls',
      compact ? formatCompactNumber(requests) : formatNumber(requests)
    )
  }

  function usageTokensText(skill: ManagedSkill): string {
    const totalTokens = skill.usage?.total_tokens ?? 0
    if (!totalTokens) {
      return '-'
    }
    return getMessage('skillUsageTokens', formatNumber(totalTokens))
  }

  function usageTokenBreakdownText(skill: ManagedSkill): string {
    if (!skill.usage) {
      return ''
    }
    return getMessage('skillUsageTokenBreakdown', [
      formatNumber(skill.usage.input_tokens),
      formatNumber(skill.usage.output_tokens),
      formatNumber(skill.usage.cached_tokens)
    ])
  }

  function timeLabel(ms?: number | null): string {
    if (!ms) {
      return ''
    }
    return new Date(ms).toLocaleString()
  }

  function skillFileLanguage(path: string): string {
    const extension = /\.([^./]+)$/.exec(path)?.[1].toLowerCase()
    return (extension && fileLanguages[extension]) || ''
  }

  function highlightSkillFileContent(content: string, language: string): string {
    const grammar = language ? Prism.languages[language] : null
    if (!grammar || !language) {
      return escapeHtml(content)
    }
    try {
      return Prism.highlight(content, grammar, language)
    } catch {
      return escapeHtml(content)
    }
  }
</script>

<div class="grid h-full min-h-0 grid-cols-[17rem_minmax(0,1fr)] overflow-hidden">
  <aside class="grid min-h-0 border-r bg-sidebar/70">
    <div class="grid min-h-0 grid-rows-[auto_minmax(0,1fr)]">
      <div class="grid gap-2 border-b p-3">
        <div class="flex items-center gap-2">
          <div class="relative min-w-0 flex-1">
            <Search
              class="pointer-events-none absolute top-1/2 left-2.5 size-3.5 -translate-y-1/2 text-muted-foreground"
            />
            <input
              class={inputClass('h-8 pr-8 pl-8 text-xs')}
              bind:value={searchQuery}
              placeholder={getMessage('skillsSearchPlaceholder')}
            />
            {#if searchQuery}
              <button
                type="button"
                class={buttonClass(
                  'ghost',
                  'icon-xs',
                  'absolute top-1/2 right-1.5 -translate-y-1/2 text-muted-foreground hover:text-foreground'
                )}
                title={getMessage('clearSearch')}
                aria-label={getMessage('clearSearch')}
                onclick={() => (searchQuery = '')}
              >
                <X class="size-3" />
              </button>
            {/if}
          </div>
          <button
            type="button"
            class={buttonClass('outline', 'icon-sm')}
            title={getMessage('reloadSkills')}
            aria-label={getMessage('reloadSkills')}
            disabled={Boolean(busyAction)}
            onclick={reloadSkills}
          >
            {#if busyAction === 'reload'}
              <LoaderCircle class="size-3.5 animate-spin" />
            {:else}
              <RefreshCw class="size-3.5" />
            {/if}
          </button>
        </div>
        <div class="grid grid-cols-2 gap-2">
          <DropdownMenu class="h-8 text-xs" items={sourceFilterItems} bind:value={sourceFilter} />
          <DropdownMenu class="h-8 text-xs" items={statusFilterItems} bind:value={statusFilter} />
        </div>
        {#each incompleteSources as source (source.path)}
          <div
            class="flex min-w-0 items-center gap-1.5 text-[11px] text-amber-700 dark:text-amber-300"
            title={source.diagnostics?.map((diagnostic) => diagnostic.message).join('\n')}
          >
            <AlertTriangle class="size-3 shrink-0" />
            <span class="truncate"
              >{getMessage('skillSourceScanIncomplete', [
                sourceLabel(source),
                formatNumber(source.diagnostics?.length ?? 0)
              ])}</span
            >
          </div>
        {/each}
      </div>

      <div class="min-h-0 overflow-y-auto p-2">
        {#if busyAction === 'load' && skills.length === 0}
          <div class="grid h-28 place-items-center text-muted-foreground">
            <LoaderCircle class="size-5 animate-spin" />
          </div>
        {:else if visibleSkills.length === 0}
          <div
            class="grid w-full place-items-center gap-2 rounded-md border border-dashed p-6 text-center text-sm text-muted-foreground"
          >
            <WandSparkles class="size-6" />
            <span>{getMessage('skillsEmpty')}</span>
          </div>
        {:else}
          <div class="grid gap-1.5">
            {#each visibleSkills as skill (skill.id)}
              {@const status = skillStatus(skill)}
              <button
                type="button"
                class={cn(
                  'grid min-w-0 gap-1 rounded-md border px-2.5 py-2 text-left transition',
                  selectedId === skill.id
                    ? 'border-primary/30 bg-background shadow-xs'
                    : 'border-transparent hover:border-border hover:bg-background/80'
                )}
                onclick={() => selectSkill(skill.id)}
              >
                <div class="flex min-w-0 items-center gap-2">
                  {#if status === 'error'}
                    <AlertTriangle class="size-3.5 shrink-0 text-destructive" />
                  {:else if status === 'disabled'}
                    <Ban class="size-3.5 shrink-0 text-muted-foreground" />
                  {:else if status === 'active'}
                    <CheckCircle2 class="size-3.5 shrink-0 text-emerald-700" />
                  {:else}
                    <FileText class="size-3.5 shrink-0 text-muted-foreground" />
                  {/if}
                  <span class="truncate text-sm font-semibold">{skill.name}</span>
                </div>
                <p class="line-clamp-2 text-xs text-muted-foreground">{skill.description}</p>
                <div class="flex min-w-0 items-center gap-1.5 text-[10px] text-muted-foreground">
                  <span class={badgeClass('outline', 'h-4 px-1.5 text-[10px]')}
                    >{sourceLabel(skill)}</span
                  >
                  <span class="truncate">{statusLabels[status]}</span>
                  <span class="ml-auto flex min-w-0 shrink items-center gap-1 tabular-nums">
                    <Activity class="size-3 shrink-0" />
                    <span class="truncate">{usageCallsText(skill, true)}</span>
                  </span>
                  {#if skill.diagnostics.length}
                    <span class="shrink-0">{skill.diagnostics.length}</span>
                  {/if}
                </div>
              </button>
            {/each}
          </div>
        {/if}
      </div>
    </div>
  </aside>

  <section class="grid min-h-0 min-w-0 grid-rows-[auto_minmax(0,1fr)]">
    <div class="flex min-h-14 min-w-0 items-center justify-between gap-3 border-b px-4">
      <div class="min-w-0">
        <h1 class="truncate text-base font-bold">{selectedSkill?.name || getMessage('skills')}</h1>
        <p class="truncate text-xs text-muted-foreground">
          {selectedSkill ? statusLine(selectedSkill) : getMessage('skillsEmpty')}
        </p>
      </div>
      <div class="flex shrink-0 items-center gap-2">
        {#if selectedSkill}
          <button
            type="button"
            class={buttonClass('outline', 'sm')}
            disabled={Boolean(busyAction)}
            onclick={selectedSkill.editable ? () => (activeTab = 'files') : () => cloneSelected()}
          >
            {#if busyAction === 'clone'}
              <LoaderCircle class="size-3.5 animate-spin" />
            {:else if selectedSkill.editable}
              <FileText class="size-3.5" />
            {:else}
              <Copy class="size-3.5" />
            {/if}
            <span class="truncate">{primaryActionLabel}</span>
          </button>
          <button
            type="button"
            class={buttonClass('outline', 'sm')}
            disabled={Boolean(busyAction)}
            onclick={toggleSelected}
          >
            {#if busyAction === 'toggle'}
              <LoaderCircle class="size-3.5 animate-spin" />
            {:else}
              <Ban class="size-3.5" />
            {/if}
            {selectedSkill.disabled ? getMessage('enableSkill') : getMessage('disableSkill')}
          </button>
          {#if selectedSkill.editable}
            <button
              type="button"
              class={buttonClass('destructive', 'sm')}
              disabled={Boolean(busyAction)}
              onclick={() => (deleteDialogOpen = true)}
            >
              {#if busyAction === 'delete'}
                <LoaderCircle class="size-3.5 animate-spin" />
              {:else}
                <Trash2 class="size-3.5" />
              {/if}
              {getMessage('deleteSkill')}
            </button>
          {/if}
        {/if}
      </div>
    </div>

    <div class="grid min-h-0 min-w-0 grid-rows-[auto_minmax(0,1fr)] overflow-hidden">
      <div>
        {#if error}
          <div class="border-b bg-destructive/5 px-4 py-2 text-sm text-destructive">{error}</div>
        {:else if notice}
          <div
            class="border-b bg-emerald-50 px-4 py-2 text-sm text-emerald-800 dark:bg-emerald-950/30 dark:text-emerald-200"
          >
            {notice}
          </div>
        {/if}
      </div>

      <div class="min-h-0 min-w-0">
        {#if detailLoading}
          <div class="grid h-48 place-items-center text-muted-foreground">
            <LoaderCircle class="size-6 animate-spin" />
          </div>
        {:else if detail && selectedSkill}
          <div class="grid h-full min-h-0 grid-rows-[auto_minmax(0,1fr)] gap-0">
            <div class="flex gap-1 border-b px-4 pt-3">
              {#each detailTabs as tab (tab.value)}
                <button
                  type="button"
                  class={cn(
                    'rounded-t-md px-3 py-2 text-xs font-semibold',
                    activeTab === tab.value
                      ? 'bg-muted text-foreground'
                      : 'text-muted-foreground hover:bg-muted/60 hover:text-foreground'
                  )}
                  onclick={() => (activeTab = tab.value)}
                >
                  {tab.label}
                </button>
              {/each}
            </div>

            {#if activeTab === 'overview'}
              <div class="min-h-0 overflow-y-auto">
                <div class="grid gap-5 p-4">
                  <div class="grid gap-3 md:grid-cols-2 xl:grid-cols-3">
                    <div class="grid gap-1 rounded-md border p-3">
                      <div class="text-xs font-semibold text-muted-foreground">
                        {getMessage('skillDirectory')}
                      </div>
                      <div class="font-mono text-xs break-all">{detail.directory}</div>
                    </div>
                    <div class="grid gap-1 rounded-md border p-3">
                      <div class="text-xs font-semibold text-muted-foreground">
                        {getMessage('skillAgentName')}
                      </div>
                      <div class="font-mono text-xs">{detail.agent_name}</div>
                    </div>
                    <div class="grid gap-1 rounded-md border p-3">
                      <div class="text-xs font-semibold text-muted-foreground">
                        {getMessage('skillUpdated')}
                      </div>
                      <div class="text-xs">{timeLabel(detail.updated_at) || '-'}</div>
                    </div>
                    <div class="grid gap-1 rounded-md border p-3">
                      <div class="text-xs font-semibold text-muted-foreground">
                        {getMessage('skillFiles')}
                      </div>
                      <div class="text-xs">
                        {getMessage('skillFileCount', formatNumber(fileCount))}
                      </div>
                      <div class="text-[11px] text-muted-foreground">
                        SKILL.md {formatSize(detail.size) || '-'}
                      </div>
                    </div>
                    <div class="grid gap-1 rounded-md border p-3">
                      <div
                        class="flex items-center gap-1.5 text-xs font-semibold text-muted-foreground"
                      >
                        <Activity class="size-3.5" />
                        <span>{getMessage('skillUsage')}</span>
                      </div>
                      <div class="text-xs">{usageCallsText(detail)}</div>
                    </div>
                    <div class="grid gap-1 rounded-md border p-3">
                      <div class="text-xs font-semibold text-muted-foreground">
                        {getMessage('skillUsageTokensTitle')}
                      </div>
                      <div class="text-xs">{usageTokensText(detail)}</div>
                      {#if detail.usage}
                        <div class="text-[11px] text-muted-foreground">
                          {usageTokenBreakdownText(detail)}
                        </div>
                      {/if}
                    </div>
                  </div>

                  <div class="grid gap-2">
                    <div class="text-xs font-semibold text-muted-foreground">
                      {getMessage('skillExecution')}
                    </div>
                    <div>
                      <span class={badgeClass('outline')}>{detail.execution}</span>
                    </div>
                  </div>

                  <div class="grid gap-2">
                    <div class="text-xs font-semibold text-muted-foreground">
                      {getMessage('skillAllowedTools')}
                    </div>
                    <div class="flex flex-wrap gap-1.5">
                      {#if detail.execution !== 'subagent'}
                        <!-- An inline skill gets no tool grant of its own: the
                             agent that reads it keeps using its own tools. -->
                        <span class="text-xs text-muted-foreground"
                          >{getMessage('skillInlineTools')}</span
                        >
                      {:else if detail.allowed_tools.length}
                        {#each detail.allowed_tools as tool}
                          <span class={badgeClass('secondary')}>{tool}</span>
                        {/each}
                      {:else}
                        <span class="text-xs text-muted-foreground"
                          >{getMessage('skillDefaultTools')}</span
                        >
                      {/if}
                    </div>
                  </div>

                  <div class="grid gap-2">
                    <div class="text-xs font-semibold text-muted-foreground">
                      {getMessage('skillDiagnostics')}
                    </div>
                    {#if detail.diagnostics.length}
                      <div class="grid gap-2">
                        {#each detail.diagnostics as diagnostic}
                          <div
                            class={cn(
                              'rounded-md border px-3 py-2 text-sm',
                              diagnostic.severity === 'error'
                                ? 'border-destructive/30 bg-destructive/5 text-destructive'
                                : 'bg-muted/35'
                            )}
                          >
                            <span class="font-semibold">{diagnostic.code}</span>
                            <span> - {diagnostic.message}</span>
                          </div>
                        {/each}
                      </div>
                    {:else}
                      <div class="text-sm text-muted-foreground">
                        {getMessage('skillDiagnosticsClear')}
                      </div>
                    {/if}
                  </div>
                </div>
              </div>
            {:else if activeTab === 'files'}
              <div class="grid h-full min-h-0 p-4">
                <div
                  class="grid h-full min-h-0 grid-rows-[minmax(0,0.45fr)_minmax(0,1fr)] gap-3 lg:grid-cols-[17rem_minmax(0,1fr)] lg:grid-rows-none"
                >
                  <div class="min-h-0 overflow-auto rounded-md border bg-muted/20 p-2">
                    <div class="grid gap-1">
                      {#each detail.files as file (file.path)}
                        <button
                          type="button"
                          class={cn(
                            'flex min-w-0 items-center gap-2 rounded px-2 py-1.5 text-left text-xs transition',
                            selectedFilePath === file.path
                              ? 'bg-background text-foreground shadow-xs'
                              : file.kind === 'file'
                                ? 'text-muted-foreground hover:bg-background/70 hover:text-foreground'
                                : 'cursor-default text-muted-foreground'
                          )}
                          disabled={file.kind !== 'file'}
                          onclick={() => selectSkillFile(file)}
                        >
                          {#if file.kind === 'directory'}
                            <Folder class="size-3.5 shrink-0" />
                          {:else if file.path.endsWith('.md')}
                            <FileText class="size-3.5 shrink-0" />
                          {:else}
                            <FileCode2 class="size-3.5 shrink-0" />
                          {/if}
                          <span class="truncate font-mono">{file.path}</span>
                          {#if file.kind === 'file'}
                            <span class="ml-auto shrink-0 tabular-nums"
                              >{formatSize(file.size) || '-'}</span
                            >
                          {/if}
                        </button>
                      {/each}
                    </div>
                  </div>
                  <div
                    class="grid min-h-0 grid-rows-[auto_minmax(0,1fr)] overflow-hidden rounded-md border"
                  >
                    <div
                      class="flex min-h-10 items-center justify-between gap-2 border-b bg-muted/25 px-3"
                    >
                      <div class="min-w-0">
                        <div class="truncate font-mono text-xs font-semibold">
                          {selectedFilePath}
                        </div>
                        {#if selectedFile}
                          <div class="text-[11px] text-muted-foreground">
                            {selectedFile.kind === 'file'
                              ? formatSize(selectedFile.size) || '-'
                              : getMessage('skillFolder')}
                          </div>
                        {/if}
                      </div>
                      {#if viewedFileTruncated}
                        <span class={badgeClass('outline', 'shrink-0 text-[10px]')}
                          >{getMessage('skillFileTruncated')}</span
                        >
                      {/if}
                    </div>
                    <div class="min-h-0 overflow-hidden">
                      {#if fileLoading}
                        <div class="grid h-full min-h-64 place-items-center text-muted-foreground">
                          <LoaderCircle class="size-5 animate-spin" />
                        </div>
                      {:else if fileError}
                        <div class="p-3 text-sm text-destructive">{fileError}</div>
                      {:else}
                        <pre
                          class="skill-file-code h-full overflow-auto p-3 font-mono text-xs whitespace-pre-wrap"><code
                            class={selectedFileLanguage
                              ? `language-${selectedFileLanguage}`
                              : 'language-text'}>{@html highlightedFileContent}</code
                          ></pre>
                      {/if}
                    </div>
                  </div>
                </div>
              </div>
            {:else}
              <div class="min-h-0 overflow-y-auto">
                <div class="grid gap-4 p-4">
                  {#if !canOptimize}
                    <div
                      class="grid max-w-3xl gap-3 rounded-md border bg-muted/25 p-3 text-sm text-muted-foreground"
                    >
                      <div>{getMessage('skillOptimizePersonalOnly')}</div>
                      <div class="font-mono text-xs break-all">{detail.directory}</div>
                      <div>
                        <button
                          type="button"
                          class={buttonClass('default', 'sm')}
                          disabled={Boolean(busyAction)}
                          onclick={() => cloneSelected('optimize')}
                        >
                          {#if busyAction === 'clone'}
                            <LoaderCircle class="size-3.5 animate-spin" />
                          {:else}
                            <Copy class="size-3.5" />
                          {/if}
                          {getMessage('copySkillToPersonal')}
                        </button>
                      </div>
                    </div>
                  {:else}
                    <label class="grid max-w-3xl gap-1 text-xs font-medium">
                      {getMessage('skillOptimizeGoal')}
                      <textarea class={textareaClass('min-h-36 text-sm')} bind:value={optimizeGoal}
                      ></textarea>
                    </label>
                    <div
                      class="grid max-w-3xl gap-1 rounded-md border bg-muted/25 px-3 py-2 text-sm text-muted-foreground"
                    >
                      <div class="font-mono text-xs break-all">{detail.directory}</div>
                      <div class="text-xs">{getMessage('skillOptimizeAndaHint')}</div>
                    </div>
                    <div class="flex items-center gap-2">
                      <button
                        type="button"
                        class={buttonClass('default', 'sm')}
                        disabled={optimizationBusy}
                        onclick={sendOptimizationRequest}
                      >
                        {#if optimizationBusy}
                          <LoaderCircle class="size-3.5 animate-spin" />
                        {:else}
                          <Send class="size-3.5" />
                        {/if}
                        {getMessage('optimizeSkillWithAnda')}
                      </button>
                    </div>
                  {/if}
                </div>
              </div>
            {/if}
          </div>
        {:else}
          <div class="grid h-64 place-items-center text-sm text-muted-foreground">
            {getMessage('skillsEmpty')}
          </div>
        {/if}
      </div>
    </div>
  </section>
</div>

{#snippet deleteActions()}
  <button
    type="button"
    class={buttonClass('outline', 'sm')}
    onclick={() => (deleteDialogOpen = false)}
  >
    {getMessage('cancel')}
  </button>
  <button type="button" class={buttonClass('destructive', 'sm')} onclick={deleteSelected}>
    {getMessage('deleteSkill')}
  </button>
{/snippet}

<Modal
  alert
  bind:open={deleteDialogOpen}
  title={getMessage('deleteSkill')}
  contentClass="min-h-0 sm:max-w-sm"
  footer={deleteActions}
>
  <p class="text-sm leading-relaxed text-muted-foreground">{getMessage('skillDeleteConfirm')}</p>
</Modal>

<style>
  .skill-file-code {
    tab-size: 2;
    color: color-mix(in oklab, var(--foreground) 92%, transparent);
  }

  .skill-file-code code {
    display: block;
    min-width: 100%;
  }

  .skill-file-code :global(.token.comment),
  .skill-file-code :global(.token.prolog),
  .skill-file-code :global(.token.doctype),
  .skill-file-code :global(.token.cdata) {
    color: color-mix(in oklab, var(--muted-foreground) 88%, transparent);
  }

  .skill-file-code :global(.token.punctuation),
  .skill-file-code :global(.token.operator) {
    color: color-mix(in oklab, var(--muted-foreground) 78%, var(--foreground));
  }

  .skill-file-code :global(.token.property),
  .skill-file-code :global(.token.tag),
  .skill-file-code :global(.token.constant),
  .skill-file-code :global(.token.symbol),
  .skill-file-code :global(.token.deleted) {
    color: #b91c1c;
  }

  .skill-file-code :global(.token.boolean),
  .skill-file-code :global(.token.number) {
    color: #b45309;
  }

  .skill-file-code :global(.token.selector),
  .skill-file-code :global(.token.attr-name),
  .skill-file-code :global(.token.string),
  .skill-file-code :global(.token.char),
  .skill-file-code :global(.token.builtin),
  .skill-file-code :global(.token.inserted) {
    color: #047857;
  }

  .skill-file-code :global(.token.keyword),
  .skill-file-code :global(.token.atrule),
  .skill-file-code :global(.token.attr-value) {
    color: #6d28d9;
  }

  .skill-file-code :global(.token.function),
  .skill-file-code :global(.token.class-name) {
    color: #1d4ed8;
  }

  .skill-file-code :global(.token.regex),
  .skill-file-code :global(.token.important),
  .skill-file-code :global(.token.variable) {
    color: #be123c;
  }

  .skill-file-code :global(.token.important),
  .skill-file-code :global(.token.bold) {
    font-weight: 600;
  }

  .skill-file-code :global(.token.italic) {
    font-style: italic;
  }

  :global(.dark) .skill-file-code :global(.token.property),
  :global(.dark) .skill-file-code :global(.token.tag),
  :global(.dark) .skill-file-code :global(.token.constant),
  :global(.dark) .skill-file-code :global(.token.symbol),
  :global(.dark) .skill-file-code :global(.token.deleted) {
    color: #f87171;
  }

  :global(.dark) .skill-file-code :global(.token.boolean),
  :global(.dark) .skill-file-code :global(.token.number) {
    color: #fbbf24;
  }

  :global(.dark) .skill-file-code :global(.token.selector),
  :global(.dark) .skill-file-code :global(.token.attr-name),
  :global(.dark) .skill-file-code :global(.token.string),
  :global(.dark) .skill-file-code :global(.token.char),
  :global(.dark) .skill-file-code :global(.token.builtin),
  :global(.dark) .skill-file-code :global(.token.inserted) {
    color: #34d399;
  }

  :global(.dark) .skill-file-code :global(.token.keyword),
  :global(.dark) .skill-file-code :global(.token.atrule),
  :global(.dark) .skill-file-code :global(.token.attr-value) {
    color: #c4b5fd;
  }

  :global(.dark) .skill-file-code :global(.token.function),
  :global(.dark) .skill-file-code :global(.token.class-name) {
    color: #93c5fd;
  }

  :global(.dark) .skill-file-code :global(.token.regex),
  :global(.dark) .skill-file-code :global(.token.important),
  :global(.dark) .skill-file-code :global(.token.variable) {
    color: #fb7185;
  }
</style>
