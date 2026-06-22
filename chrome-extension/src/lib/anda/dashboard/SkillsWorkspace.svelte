<script lang="ts">
  import { andaClient } from '$lib/anda/client/side-panel.svelte'
  import type {
    ManagedSkill,
    ManagedSkillDetail,
    SkillDiagnostic,
    SkillFileEntry,
    SkillSourceInfo
  } from '$lib/anda/client/types'
  import {
    badgeClass,
    buttonClass,
    inputClass,
    nativeSelectClass,
    textareaClass
  } from '$lib/anda/ui'
  import { getMessage } from '$lib/i18n'
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

  type SourceFilter = 'all' | ManagedSkill['source']
  type StatusFilter = 'all' | 'active' | 'disabled' | 'shadowed' | 'error'
  type DetailTab = 'overview' | 'files' | 'optimize'

  let skills = $state<ManagedSkill[]>([])
  let sources = $state<SkillSourceInfo[]>([])
  let selectedId = $state('')
  let detail = $state<ManagedSkillDetail | null>(null)
  let searchQuery = $state('')
  let sourceFilter = $state<SourceFilter>('all')
  let statusFilter = $state<StatusFilter>('all')
  let activeTab = $state<DetailTab>('overview')
  let loading = $state(false)
  let detailLoading = $state(false)
  let reloading = $state(false)
  let cloning = $state(false)
  let toggling = $state(false)
  let deleting = $state(false)
  let error = $state('')
  let notice = $state('')
  let selectedFilePath = $state('SKILL.md')
  let viewedFileContent = $state('')
  let viewedFileTruncated = $state(false)
  let fileLoading = $state(false)
  let fileError = $state('')
  let optimizeGoal = $state('')
  let optimizing = $state(false)
  let detailRequestId = 0

  const compactNumberFormatter = new Intl.NumberFormat(undefined, {
    maximumFractionDigits: 1,
    notation: 'compact'
  })
  const numberFormatter = new Intl.NumberFormat()

  const visibleSkills = $derived.by(() => {
    const query = searchQuery.trim().toLowerCase()
    return skills.filter((skill) => {
      const matchesQuery =
        !query ||
        skill.name.toLowerCase().includes(query) ||
        (skill.description || '').toLowerCase().includes(query) ||
        skill.path.toLowerCase().includes(query) ||
        skill.directory.toLowerCase().includes(query)
      const matchesSource = sourceFilter === 'all' || skill.source === sourceFilter
      const matchesStatus =
        statusFilter === 'all' ||
        (statusFilter === 'active' && skill.active) ||
        (statusFilter === 'disabled' && skill.disabled) ||
        (statusFilter === 'shadowed' && Boolean(skill.shadowed_by)) ||
        (statusFilter === 'error' && hasError(skill.diagnostics))
      return matchesQuery && matchesSource && matchesStatus
    })
  })
  const selectedSkill = $derived(skills.find((skill) => skill.id === selectedId) || null)
  const selectedFile = $derived(
    detail?.files.find((file) => file.path === selectedFilePath) || null
  )
  const selectedFileContent = $derived(
    detail && selectedFilePath === 'SKILL.md' ? detail.content : viewedFileContent
  )
  const selectedFileLanguage = $derived(skillFileLanguage(selectedFilePath))
  const highlightedFileContent = $derived(
    highlightSkillFileContent(selectedFileContent, selectedFileLanguage)
  )
  const optimizationBusy = $derived(
    optimizing ||
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
      .init()
      .catch(() => undefined)
      .finally(() => {
        void loadLibrary()
      })
  })

  async function loadLibrary(keepSelection = true) {
    loading = true
    error = ''
    try {
      const [nextSources, nextSkills] = await Promise.all([
        andaClient.listSkillSources(),
        andaClient.listManagedSkills(true)
      ])
      sources = nextSources
      skills = sortSkills(nextSkills)
      if (!keepSelection || !skills.some((skill) => skill.id === selectedId)) {
        selectedId = skills.find((skill) => skill.active)?.id || skills[0]?.id || ''
      }
      if (selectedId) {
        await loadDetail(selectedId)
      } else {
        detail = null
        selectedFilePath = 'SKILL.md'
        viewedFileContent = ''
        viewedFileTruncated = false
        fileError = ''
      }
    } catch (err) {
      error = errorToMessage(err)
    } finally {
      loading = false
    }
  }

  async function reloadSkills() {
    reloading = true
    error = ''
    try {
      skills = sortSkills(await andaClient.reloadSkills())
      await loadLibrary(true)
      notice = getMessage('skillsReloaded')
    } catch (err) {
      error = errorToMessage(err)
    } finally {
      reloading = false
    }
  }

  async function loadDetail(id: string) {
    const requestId = ++detailRequestId
    detailLoading = true
    error = ''
    try {
      const next = await andaClient.getManagedSkill(id)
      if (requestId !== detailRequestId) {
        return
      }
      detail = next
      selectedFilePath = 'SKILL.md'
      viewedFileContent = next.content
      viewedFileTruncated = false
      fileError = ''
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

  function selectSkill(id: string) {
    if (selectedId === id) {
      return
    }
    selectedId = id
    activeTab = 'overview'
    void loadDetail(id)
  }

  async function cloneSelected(nextTab: DetailTab = 'files') {
    if (!selectedSkill || cloning) {
      return
    }
    cloning = true
    error = ''
    try {
      const cloned = await andaClient.cloneSkill(selectedSkill.id)
      notice = getMessage('skillCloned')
      selectedId = cloned.id
      detail = cloned
      viewedFileContent = cloned.content
      selectedFilePath = 'SKILL.md'
      viewedFileTruncated = false
      activeTab = nextTab
      await loadLibrary(true)
    } catch (err) {
      error = errorToMessage(err)
    } finally {
      cloning = false
    }
  }

  async function toggleSelected() {
    if (!selectedSkill || toggling) {
      return
    }
    const enabling = selectedSkill.disabled
    toggling = true
    error = ''
    try {
      skills = sortSkills(await andaClient.setSkillEnabled(selectedSkill.id, enabling))
      notice = enabling ? getMessage('skillEnabled') : getMessage('skillDisabled')
      await loadLibrary(true)
    } catch (err) {
      error = errorToMessage(err)
    } finally {
      toggling = false
    }
  }

  async function deleteSelected() {
    if (!selectedSkill?.editable || deleting) {
      return
    }
    if (!confirm(getMessage('skillDeleteConfirm'))) {
      return
    }
    deleting = true
    error = ''
    try {
      await andaClient.deletePersonalSkill(selectedSkill.id)
      selectedId = ''
      detail = null
      viewedFileContent = ''
      selectedFilePath = 'SKILL.md'
      viewedFileTruncated = false
      notice = getMessage('skillDeleted')
      await loadLibrary(false)
    } catch (err) {
      error = errorToMessage(err)
    } finally {
      deleting = false
    }
  }

  async function selectSkillFile(file: SkillFileEntry) {
    if (!detail || file.kind !== 'file' || file.path === selectedFilePath) {
      return
    }
    selectedFilePath = file.path
    fileError = ''
    viewedFileTruncated = false
    if (file.path === 'SKILL.md') {
      viewedFileContent = detail.content
      return
    }
    fileLoading = true
    try {
      const loaded = await andaClient.getManagedSkillFile(detail.id, file.path)
      if (selectedFilePath !== file.path) {
        return
      }
      viewedFileContent = loaded.content
      viewedFileTruncated = loaded.truncated
    } catch (err) {
      if (selectedFilePath === file.path) {
        fileError = errorToMessage(err)
        viewedFileContent = ''
      }
    } finally {
      if (selectedFilePath === file.path) {
        fileLoading = false
      }
    }
  }

  async function sendOptimizationRequest() {
    if (!detail || !canOptimize || optimizationBusy) {
      return
    }
    optimizing = true
    error = ''
    try {
      await chrome.storage.local.set({
        [promptDraftRequestStorageKey]: createPromptDraftRequest(
          skillOptimizationPrompt(detail, optimizeGoal)
        )
      })
      await openSidePanel()
      notice = getMessage('skillOptimizationPromptReady')
    } catch (err) {
      error = errorToMessage(err)
    } finally {
      optimizing = false
    }
  }

  async function openSidePanel() {
    if (chrome.sidePanel?.open) {
      try {
        const tab = await chrome.tabs.getCurrent()
        if (typeof tab?.id === 'number') {
          await chrome.sidePanel.open({ tabId: tab.id })
          return
        }
        if (typeof tab?.windowId === 'number') {
          await chrome.sidePanel.open({ windowId: tab.windowId })
          return
        }
      } catch (_error) {
        try {
          const currentWindow = await chrome.windows.getCurrent()
          if (typeof currentWindow?.id === 'number') {
            await chrome.sidePanel.open({ windowId: currentWindow.id })
            return
          }
        } catch (_fallbackError) {
          // Fall through to opening the side panel page as a tab below.
        }
      }
    }

    const url = chrome.runtime.getURL('index.html')
    chrome.tabs.create({ url, active: true }).catch(() => {
      window.open(url, '_blank', 'noopener,noreferrer')
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

  function hasError(diagnostics: SkillDiagnostic[] = []): boolean {
    return diagnostics.some((diagnostic) => diagnostic.severity === 'error')
  }

  function statusText(skill: ManagedSkill): string {
    if (hasError(skill.diagnostics)) {
      return getMessage('skillStatusError')
    }
    if (skill.disabled) {
      return getMessage('skillStatusDisabled')
    }
    if (skill.shadowed_by) {
      return getMessage('skillStatusShadowed')
    }
    if (skill.active) {
      return getMessage('skillStatusActive')
    }
    return getMessage('skillStatusInactive')
  }

  function sourceCount(source: SkillSourceInfo): number {
    return skills.filter((skill) => skill.source === source.source).length
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
    const filename = path.split('/').pop()?.toLowerCase() || ''
    if (filename === 'skill.md' || filename.endsWith('.md') || filename.endsWith('.markdown')) {
      return 'markdown'
    }
    if (filename === 'package.json' || filename.endsWith('.json')) {
      return 'json'
    }
    if (filename.endsWith('.jsonc')) {
      return 'json'
    }
    if (filename.endsWith('.json5')) {
      return 'json5'
    }
    if (filename.endsWith('.yaml') || filename.endsWith('.yml')) {
      return 'yaml'
    }
    if (filename.endsWith('.toml')) {
      return 'toml'
    }
    if (filename.endsWith('.py')) {
      return 'python'
    }
    if (filename.endsWith('.rs')) {
      return 'rust'
    }
    if (filename.endsWith('.ts') || filename.endsWith('.mts') || filename.endsWith('.cts')) {
      return 'typescript'
    }
    if (filename.endsWith('.tsx')) {
      return 'tsx'
    }
    if (filename.endsWith('.js') || filename.endsWith('.mjs') || filename.endsWith('.cjs')) {
      return 'javascript'
    }
    if (filename.endsWith('.jsx')) {
      return 'jsx'
    }
    if (filename.endsWith('.sh') || filename.endsWith('.bash') || filename.endsWith('.zsh')) {
      return 'bash'
    }
    if (filename.endsWith('.html') || filename.endsWith('.xml') || filename.endsWith('.svg')) {
      return 'markup'
    }
    if (filename.endsWith('.css')) {
      return 'css'
    }
    return ''
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

  function escapeHtml(value: string): string {
    return value
      .replace(/&/g, '&amp;')
      .replace(/</g, '&lt;')
      .replace(/>/g, '&gt;')
      .replace(/"/g, '&quot;')
  }
</script>

<div class="grid h-full min-h-0 grid-cols-[17rem_minmax(0,1fr)] overflow-hidden">
  <aside class="grid min-h-0 border-r bg-sidebar/70">
    <div class="grid min-h-0 grid-rows-[auto_minmax(0,1fr)]">
      <div class="grid gap-2 border-b p-3">
        <div class="flex items-center gap-2">
          <div class="relative min-w-0 flex-1">
            <Search
              class="pointer-events-none absolute left-2.5 top-1/2 size-3.5 -translate-y-1/2 text-muted-foreground"
            />
            <input
              class={inputClass('h-8 pl-8 pr-8 text-xs')}
              bind:value={searchQuery}
              placeholder={getMessage('skillsSearchPlaceholder')}
            />
            {#if searchQuery}
              <button
                type="button"
                class={buttonClass(
                  'ghost',
                  'icon-xs',
                  'absolute right-1.5 top-1/2 -translate-y-1/2 text-muted-foreground hover:text-foreground'
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
            disabled={reloading}
            onclick={reloadSkills}
          >
            {#if reloading}
              <LoaderCircle class="size-3.5 animate-spin" />
            {:else}
              <RefreshCw class="size-3.5" />
            {/if}
          </button>
        </div>
        <div class="grid grid-cols-2 gap-2">
          <select class={nativeSelectClass('h-8 text-xs')} bind:value={sourceFilter}>
            <option value="all">{getMessage('allSources')}</option>
            {#each sources as source (source.source)}
              <option value={source.source}>{source.source_label} ({sourceCount(source)})</option>
            {/each}
          </select>
          <select class={nativeSelectClass('h-8 text-xs')} bind:value={statusFilter}>
            <option value="all">{getMessage('allStatuses')}</option>
            <option value="active">{getMessage('skillStatusActive')}</option>
            <option value="disabled">{getMessage('skillStatusDisabled')}</option>
            <option value="shadowed">{getMessage('skillStatusShadowed')}</option>
            <option value="error">{getMessage('skillStatusError')}</option>
          </select>
        </div>
      </div>

      <div class="min-h-0 overflow-y-auto p-2">
        {#if loading}
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
                  {#if hasError(skill.diagnostics)}
                    <AlertTriangle class="size-3.5 shrink-0 text-destructive" />
                  {:else if skill.disabled}
                    <Ban class="size-3.5 shrink-0 text-muted-foreground" />
                  {:else if skill.active}
                    <CheckCircle2 class="size-3.5 shrink-0 text-emerald-700" />
                  {:else}
                    <FileText class="size-3.5 shrink-0 text-muted-foreground" />
                  {/if}
                  <span class="truncate text-sm font-semibold">{skill.name}</span>
                </div>
                <p class="line-clamp-2 text-xs text-muted-foreground">{skill.description}</p>
                <div class="flex min-w-0 items-center gap-1.5 text-[10px] text-muted-foreground">
                  <span class={badgeClass('outline', 'h-4 px-1.5 text-[10px]')}
                    >{skill.source_label}</span
                  >
                  <span class="truncate">{statusText(skill)}</span>
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
          {selectedSkill
            ? `${selectedSkill.source_label} / ${statusText(selectedSkill)} / ${usageCallsText(selectedSkill)}`
            : getMessage('skillsEmpty')}
        </p>
      </div>
      <div class="flex shrink-0 items-center gap-2">
        {#if selectedSkill}
          <button
            type="button"
            class={buttonClass('outline', 'sm')}
            disabled={cloning}
            onclick={selectedSkill.editable ? () => (activeTab = 'files') : () => cloneSelected()}
          >
            {#if cloning}
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
            disabled={toggling}
            onclick={toggleSelected}
          >
            {#if toggling}
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
              disabled={deleting}
              onclick={deleteSelected}
            >
              {#if deleting}
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
          <div class="grid h-full min-h-0 gap-0 grid-rows-[auto_minmax(0,1fr)]">
            <div class="flex gap-1 border-b px-4 pt-3">
              {#each ['overview', 'files', 'optimize'] as tab}
                <button
                  type="button"
                  class={cn(
                    'rounded-t-md px-3 py-2 text-xs font-semibold',
                    activeTab === tab
                      ? 'bg-muted text-foreground'
                      : 'text-muted-foreground hover:bg-muted/60 hover:text-foreground'
                  )}
                  onclick={() => (activeTab = tab as DetailTab)}
                >
                  {tab === 'overview'
                    ? getMessage('skillOverview')
                    : tab === 'files'
                      ? getMessage('skillFiles')
                      : getMessage('skillOptimize')}
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
                      <div class="break-all font-mono text-xs">{detail.directory}</div>
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
                        {getMessage('skillFileCount', formatNumber(detail.file_count))}
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
                      {getMessage('skillAllowedTools')}
                    </div>
                    <div class="flex flex-wrap gap-1.5">
                      {#if detail.allowed_tools.length}
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
                  class="grid h-full min-h-0 gap-3 grid-rows-[minmax(0,0.45fr)_minmax(0,1fr)] lg:grid-cols-[17rem_minmax(0,1fr)] lg:grid-rows-none"
                >
                  <div class="min-h-0 overflow-auto rounded-md border bg-muted/20 p-2">
                    <div class="grid gap-1">
                      {#each detail.files as file}
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
                          class="skill-file-code h-full overflow-auto whitespace-pre-wrap p-3 font-mono text-xs"><code
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
                      <div class="break-all font-mono text-xs">{detail.directory}</div>
                      <div>
                        <button
                          type="button"
                          class={buttonClass('default', 'sm')}
                          disabled={cloning}
                          onclick={() => cloneSelected('optimize')}
                        >
                          {#if cloning}
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
                      <div class="break-all font-mono text-xs">{detail.directory}</div>
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
