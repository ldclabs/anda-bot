<script lang="ts">
  import Inbox from '$lib/anda/brain/Inbox.svelte'
  import { getMessage } from '$lib/i18n'
  import { escapeHtml } from '$lib/utils/format'
  import {
    ANDA_BOT_SPACE_ID,
    BrainApi,
    loadBrainGraphSettings,
    saveBrainGraphSettings
  } from '$lib/anda/brain/api'
  import type { BrainGraphSettings } from '$lib/anda/brain/api'
  import {
    BrainGraphData,
    type Concept,
    type GraphSnapshot,
    type Proposition
  } from '$lib/anda/brain/graph.svelte'
  import {
    buildBrainGraphView,
    isBrainSummaryNodeId,
    type BrainGraphUiState,
    type BrainGraphView,
    type RelatedMemoryGroup,
    type RelatedMemoryItem,
    typeColor
  } from '$lib/anda/brain/view-model'
  import { applyAppearanceTheme } from '$lib/anda/theme'
  import {
    badgeClass,
    buttonClass,
    inputClass,
    nativeSelectClass,
    separatorClass,
    textareaClass
  } from '$lib/anda/ui'
  import { cn } from '$lib/utils'
  import Prism from '$lib/utils/prismjs'
  import { defaultSettings, errorToMessage } from '$lib/service-worker/settings'
  import { BrainGraphRenderer, type BrainGraphHighlights } from '$lib/anda/brain/renderer'
  import {
    Braces,
    Check,
    Copy,
    GitFork,
    LoaderCircle,
    LocateFixed,
    Maximize2,
    BrainCircuit,
    Pin,
    PinOff,
    RefreshCw,
    Route,
    Search,
    Settings,
    FileBracesCorner,
    X,
    ZoomIn,
    ZoomOut
  } from '@lucide/svelte'
  import { onMount, tick } from 'svelte'

  let { embedded = false }: { embedded?: boolean } = $props()

  const initialSettings: BrainGraphSettings = {
    ...defaultSettings,
    spaceId: ANDA_BOT_SPACE_ID
  }

  let settings = $state<BrainGraphSettings>(initialSettings)
  let graphData = new BrainGraphData(new BrainApi(initialSettings))
  let container: HTMLDivElement | null = $state(null)
  const renderer = new BrainGraphRenderer({
    onSelectNode: (id) => selectGraphNode(id),
    onExpandNode: (id) => void expandNode(id),
    onSelectEdge: (id) => {
      selectedEdgeId = id
      selectedSummaryId = ''
    },
    onCanvasClick: () => handleCanvasClick()
  })

  let loading = $state(false)
  let saving = $state(false)
  let errorMessage = $state('')
  let statusText = $state(getMessage('brainStatusIdle'))
  let settingsOpen = $state(false)
  let queryOpen = $state(false)
  let inboxOpen = $state(false)
  let inspectorCopyState = $state<'idle' | 'copied'>('idle')

  let selectedNodeId = $state('')
  let selectedEdgeId = $state('')
  let selectedSummaryId = $state('')
  let expandingNodeId = $state('')
  let searchQuery = $state('')
  let searchResults = $state<string[]>([])
  let searchIndex = $state(0)
  let expandedNodeIds = $state<string[]>([])
  let pinnedNodeIds = $state<string[]>([])
  let pathRequested = $state(false)
  let breadcrumb = $state<Array<{ id: string; label: string }>>([])
  let kipCommand = $state(`FIND(?node)
WHERE {
  ?node CONCEPT {}
}
LIMIT 100`)
  let queryOutput = $state('')

  let inspectorCopyTimer: ReturnType<typeof setTimeout> | null = null

  const snapshot = $derived.by<GraphSnapshot>(() => {
    return graphData.snapshot()
  })
  const graphUiState = $derived.by<BrainGraphUiState>(() => {
    return {
      anchorNodeId: selectedNodeId,
      selectedEdgeId,
      searchResultIds: searchResults,
      expandedNodeIds: new Set(expandedNodeIds),
      pinnedNodeIds: new Set(pinnedNodeIds),
      pathRequested,
      breadcrumb
    }
  })
  const graphDataset = $derived.by<BrainGraphView>(() => {
    return buildBrainGraphView(graphData, snapshot, graphUiState)
  })
  const selectedNode = $derived.by<Concept | null>(() =>
    selectedNodeId ? graphData.nodes.get(selectedNodeId) || null : null
  )
  const selectedEdge = $derived.by<Proposition | null>(() =>
    selectedEdgeId ? graphData.links.get(selectedEdgeId) || null : null
  )
  const selectedSummaryGroup = $derived.by<RelatedMemoryGroup | null>(() => {
    if (!selectedSummaryId) {
      return null
    }
    const summaryNode = graphDataset.summaryNodes.find((node) => node.id === selectedSummaryId)
    const groupId = selectedSummaryId.startsWith('brain-cluster:')
      ? `atlas-type:${selectedSummaryId.slice('brain-cluster:'.length)}`
      : summaryNode?.groupId || selectedSummaryId
    return graphDataset.relatedGroups.find((group) => group.id === groupId) || null
  })
  const searchResultNodes = $derived.by<Concept[]>(() =>
    searchResults
      .map((id) => graphData.nodes.get(id))
      .filter((node): node is Concept => Boolean(node))
  )
  const pinnedNodes = $derived.by<Concept[]>(() =>
    pinnedNodeIds
      .map((id) => graphData.nodes.get(id))
      .filter((node): node is Concept => Boolean(node))
  )
  const remainingRelatedCount = $derived.by(() =>
    graphDataset.relatedGroups.reduce((total, group) => total + group.hidden, 0)
  )
  const summary = $derived.by(() => graphDataset.summary)

  $effect(() => applyAppearanceTheme(settings.appearanceTheme))

  const highlights = $derived<BrainGraphHighlights>({
    selectedNodeId,
    selectedEdgeId,
    selectedSummaryId,
    expandingNodeId,
    searchResults,
    pinnedNodeIds
  })

  let syncTimer: ReturnType<typeof setTimeout> | null = null
  $effect(() => {
    const dataset = graphDataset
    if (!renderer.rendered) {
      return
    }
    if (syncTimer) {
      clearTimeout(syncTimer)
    }
    syncTimer = setTimeout(() => {
      syncTimer = null
      renderer.sync(dataset, highlights).catch((error) => {
        errorMessage = errorToMessage(error)
      })
    }, 120)
  })

  // Give instant selection feedback via element states; the debounced dataset
  // sync converges the underlying styles afterwards.
  $effect(() => {
    renderer.applyHighlights(highlights)
  })

  onMount(() => {
    if (container) {
      renderer.mount(container, graphDataset, highlights).catch((error) => {
        errorMessage = errorToMessage(error)
      })
    }
    loadBrainGraphSettings()
      .then(async (saved) => {
        settings = saved
        graphData.setApi(new BrainApi(settings))
        await loadGraph()
      })
      .catch((error) => {
        errorMessage = errorToMessage(error)
        statusText = getMessage('brainStatusSettingsUnavailable')
      })

    return () => {
      if (syncTimer) {
        clearTimeout(syncTimer)
      }
      if (inspectorCopyTimer) {
        clearTimeout(inspectorCopyTimer)
      }
      renderer.destroy()
    }
  })

  async function loadGraph() {
    loading = true
    errorMessage = ''
    statusText = getMessage('brainStatusLoadingGraph')
    try {
      graphData.clear()
      graphData.setApi(new BrainApi(settings))
      const stats = await graphData.loadOverview()
      statusText = getMessage(
        'brainStatusOverviewNodes',
        graphData.status
          ? `${graphData.nodes.size}/${graphData.status.concepts}`
          : String(stats.concepts)
      )
    } catch (error) {
      errorMessage = errorToMessage(error)
      statusText = getMessage('brainStatusLoadFailed')
      try {
        await graphData.loadSchema()
      } catch (_schemaError) {
        // Keep the original load error visible.
      }
    } finally {
      loading = false
    }
  }

  async function saveAndReload() {
    saving = true
    errorMessage = ''
    try {
      await saveBrainGraphSettings(settings)
      await loadGraph()
      settingsOpen = false
    } catch (error) {
      errorMessage = errorToMessage(error)
    } finally {
      saving = false
    }
  }

  async function runSearch() {
    const query = searchQuery.trim()
    if (!query) {
      searchResults = []
      searchIndex = 0
      selectedNodeId = ''
      selectedEdgeId = ''
      selectedSummaryId = ''
      pathRequested = false
      return
    }

    loading = true
    errorMessage = ''
    statusText = getMessage('brainStatusSearching')
    try {
      const results = await graphData.searchConcepts(query)
      searchResults = results.map((result) => result.id)
      searchIndex = 0
      if (results[0]) {
        setAnchorNode(results[0].id)
        await graphData.expandConcept(results[0].id).catch(() => undefined)
      }
      statusText = getMessage('brainStatusSearchResults', String(results.length))
    } catch (error) {
      errorMessage = errorToMessage(error)
      statusText = getMessage('brainStatusSearchFailed')
    } finally {
      loading = false
    }
  }

  async function stepSearch(delta: number) {
    if (!searchResults.length) {
      return
    }
    searchIndex = (searchIndex + delta + searchResults.length) % searchResults.length
    const id = searchResults[searchIndex] || ''
    if (id) {
      setAnchorNode(id)
      await graphData.expandConcept(id).catch(() => undefined)
    }
  }

  async function selectSearchResult(id: string, index: number) {
    searchIndex = index
    setAnchorNode(id)
    await graphData.expandConcept(id).catch(() => undefined)
  }

  async function runKipQuery() {
    const command = kipCommand.trim()
    if (!command) {
      return
    }
    loading = true
    errorMessage = ''
    statusText = getMessage('brainStatusRunningKip')
    try {
      const result = await graphData.executeQuery(command)
      queryOutput = JSON.stringify(result, null, 2)
      statusText = getMessage('brainStatusKipComplete')
    } catch (error) {
      errorMessage = errorToMessage(error)
      statusText = getMessage('brainStatusKipFailed')
    } finally {
      loading = false
    }
  }

  async function copyInspectorJson(
    value: Concept | Proposition | null = selectedNode || selectedEdge
  ) {
    if (!value) {
      return
    }
    try {
      if (!navigator.clipboard?.writeText) {
        throw new Error(getMessage('brainClipboardUnavailable'))
      }
      await navigator.clipboard.writeText(JSON.stringify(value, null, 2))
      inspectorCopyState = 'copied'
      if (inspectorCopyTimer) {
        clearTimeout(inspectorCopyTimer)
      }
      inspectorCopyTimer = setTimeout(() => {
        inspectorCopyState = 'idle'
        inspectorCopyTimer = null
      }, 1200)
    } catch (error) {
      errorMessage = errorToMessage(error)
    }
  }

  async function expandNode(id = selectedNodeId) {
    if (!id) {
      return
    }
    expandingNodeId = id
    errorMessage = ''
    try {
      await graphData.expandConcept(id)
      setAnchorNode(id)
      if (!expandedNodeIds.includes(id)) {
        expandedNodeIds = [...expandedNodeIds, id]
      }
      renderer.requestFocus(id)
    } catch (error) {
      errorMessage = errorToMessage(error)
    } finally {
      expandingNodeId = ''
    }
  }

  function selectGraphNode(id: string) {
    if (graphData.nodes.has(id)) {
      selectNode(id)
      return
    }
    if (isBrainSummaryNodeId(id) || id.startsWith('brain-cluster:')) {
      selectedSummaryId = id
      selectedEdgeId = ''
      return
    }
  }

  function selectNode(id: string) {
    setAnchorNode(id)
  }

  function setAnchorNode(id: string) {
    const node = graphData.nodes.get(id)
    if (!node) {
      return
    }
    selectedNodeId = id
    selectedEdgeId = ''
    selectedSummaryId = ''
    pathRequested = false
    pushBreadcrumb(node)
    renderer.requestFocus(id)
  }

  function pushBreadcrumb(node: Concept) {
    const previous = breadcrumb[breadcrumb.length - 1]
    if (previous?.id === node.id) {
      return
    }
    breadcrumb = [
      ...breadcrumb.filter((item) => item.id !== node.id),
      { id: node.id, label: node.name }
    ].slice(-8)
  }

  function goToBreadcrumb(index: number) {
    const item = breadcrumb[index]
    if (!item) {
      return
    }
    selectedNodeId = item.id
    selectedEdgeId = ''
    selectedSummaryId = ''
    pathRequested = false
    breadcrumb = breadcrumb.slice(0, index + 1)
    renderer.requestFocus(item.id)
  }

  function showAtlas() {
    selectedNodeId = ''
    selectedEdgeId = ''
    selectedSummaryId = ''
    pathRequested = false
    breadcrumb = []
  }

  function handleCanvasClick() {
    if (selectedEdgeId || selectedSummaryId) {
      selectedEdgeId = ''
      selectedSummaryId = ''
      return
    }
    if (breadcrumb.length > 1) {
      const next = breadcrumb[breadcrumb.length - 2]
      breadcrumb = breadcrumb.slice(0, -1)
      selectedNodeId = next?.id || ''
      if (selectedNodeId) {
        renderer.requestFocus(selectedNodeId)
      }
    }
  }

  function selectRelatedItem(item: RelatedMemoryItem) {
    if (item.nodeId && graphData.nodes.has(item.nodeId)) {
      setAnchorNode(item.nodeId)
    }
    if (item.edgeId && graphData.links.has(item.edgeId)) {
      selectedEdgeId = item.edgeId
      selectedSummaryId = ''
    }
  }

  function selectRelatedGroup(group: RelatedMemoryGroup) {
    selectedSummaryId = group.id
    selectedEdgeId = ''
  }

  function relatedGroupTitle(group: RelatedMemoryGroup): string {
    if (group.id === 'atlas-hubs') {
      return getMessage('brainFrequentHubs')
    }
    if (group.id === 'atlas-recent') {
      return getMessage('brainRecentMemories')
    }
    if (group.id === 'path:pinned') {
      return getMessage('brainPinnedMemories')
    }
    if (group.kind === 'summary') {
      return getMessage('brainMoreType', group.title.replace(/^More\s+/, ''))
    }
    return group.title
  }

  function focusTypeCluster(type: string) {
    const group = graphDataset.relatedGroups.find((item) => item.id === `atlas-type:${type}`)
    if (group) {
      selectRelatedGroup(group)
    }
  }

  function togglePin(id: string) {
    if (!graphData.nodes.has(id)) {
      return
    }
    if (pinnedNodeIds.includes(id)) {
      pinnedNodeIds = pinnedNodeIds.filter((item) => item !== id)
      if (pinnedNodeIds.length < 2) {
        pathRequested = false
      }
      return
    }
    pinnedNodeIds = [...pinnedNodeIds, id].slice(-8)
  }

  function isPinned(id: string): boolean {
    return pinnedNodeIds.includes(id)
  }

  function showPinnedPath() {
    if (pinnedNodeIds.length < 2) {
      return
    }
    selectedEdgeId = ''
    selectedSummaryId = ''
    pathRequested = true
  }

  function clearPinned() {
    pinnedNodeIds = []
    pathRequested = false
  }

  async function focusElement(id = selectedNodeId) {
    if (!id) {
      return
    }
    await tick()
    renderer.focus(id)
  }

  async function fitView() {
    await tick()
    renderer.fitView()
  }

  function zoomBy(scale: number) {
    renderer.zoomBy(scale)
  }

  function formatJson(value: unknown): string {
    if (!value || (typeof value === 'object' && Object.keys(value).length === 0)) {
      return '{}'
    }
    return JSON.stringify(value, null, 2)
  }

  function highlightJson(value: unknown): string {
    return highlightJsonString(formatJson(value))
  }

  function highlightJsonString(value: string): string {
    try {
      const jsonGrammar = Prism.languages.json
      return jsonGrammar ? Prism.highlight(value, jsonGrammar, 'json') : escapeHtml(value)
    } catch {
      return escapeHtml(value)
    }
  }
</script>

<svelte:head>
  <title>{getMessage('brainPageTitle') || 'Anda Brain Graph'}</title>
</svelte:head>

<div class={embedded ? 'brain-shell brain-shell-embedded' : 'brain-shell'}>
  {#if !embedded}
    <header class="brain-topbar">
      <div class="flex min-w-0 items-center gap-3">
        <div
          class="flex size-9 shrink-0 items-center justify-center rounded-md border border-border bg-card"
        >
          <BrainCircuit class="size-4" />
        </div>
        <div class="min-w-0">
          <h1 class="truncate text-sm font-semibold">Anda Brain</h1>
          <p class="truncate text-xs text-muted-foreground">
            {settings.baseUrl}/v1/{settings.spaceId}
          </p>
        </div>
      </div>

      <div class="flex items-center gap-2">
        <span class={badgeClass(loading ? 'secondary' : errorMessage ? 'destructive' : 'outline')}>
          {#if loading}
            <LoaderCircle class="size-3 animate-spin" />
          {/if}
          {statusText}
        </span>
        <button
          class={buttonClass('ghost', 'icon-sm')}
          title={getMessage('settings')}
          onclick={() => (settingsOpen = !settingsOpen)}
        >
          <Settings class="size-4" />
        </button>
        <button class={buttonClass('outline', 'sm')} onclick={loadGraph} disabled={loading}>
          <RefreshCw class={cn('size-4', loading && 'animate-spin')} />
          {getMessage('refresh')}
        </button>
      </div>
    </header>
  {/if}

  {#if errorMessage && !embedded}
    <div
      class="border-b border-destructive/20 bg-destructive/10 px-4 py-2 text-xs text-destructive"
    >
      {errorMessage}
    </div>
  {/if}

  <main class="brain-workspace">
    <aside class="brain-sidebar">
      <button
        class={buttonClass('outline', 'sm')}
        aria-pressed={inboxOpen}
        onclick={() => (inboxOpen = !inboxOpen)}
      >
        {getMessage(inboxOpen ? 'brainBackToGraph' : 'brainInbox')}
      </button>
      {#if settingsOpen}
        <section class="brain-panel">
          <div class="brain-panel-heading">
            <Settings class="size-4" />
            {getMessage('brainConnection')}
          </div>
          <label class="brain-field">
            <span>{getMessage('gatewayUrl')}</span>
            <input class={inputClass('h-8 text-xs')} bind:value={settings.baseUrl} />
          </label>
          <label class="brain-field">
            <span>{getMessage('brainFieldSpace')}</span>
            <input class={inputClass('h-8 text-xs')} bind:value={settings.spaceId} />
          </label>
          <label class="brain-field">
            <span>{getMessage('bearerToken')}</span>
            <input class={inputClass('h-8 text-xs')} bind:value={settings.token} type="password" />
          </label>
          <label class="brain-field">
            <span>{getMessage('appearanceTheme')}</span>
            <select class={nativeSelectClass('h-8 text-xs')} bind:value={settings.appearanceTheme}>
              <option value="system">{getMessage('appearanceSystem')}</option>
              <option value="light">{getMessage('appearanceLight')}</option>
              <option value="dark">{getMessage('appearanceDark')}</option>
            </select>
          </label>
          <button
            class={buttonClass('default', 'sm', 'w-full')}
            onclick={saveAndReload}
            disabled={saving || loading}
          >
            {#if saving}
              <LoaderCircle class="size-4 animate-spin" />
            {/if}
            {getMessage('save')}
          </button>
        </section>
      {/if}

      <section class="brain-panel">
        <div class="brain-panel-heading justify-between">
          <span class="flex items-center gap-2">
            <Search class="size-4" />
            {getMessage('search')}
          </span>
          {#if searchResults.length}
            <span class={badgeClass('outline')}>{searchResults.length}</span>
          {/if}
        </div>
        <div class="flex gap-2">
          <input
            class={inputClass('h-8 text-xs')}
            bind:value={searchQuery}
            placeholder={getMessage('brainSearchPlaceholder')}
            onkeydown={(event) => {
              if (event.key === 'Enter') runSearch()
              if (event.key === 'Escape') {
                searchQuery = ''
                searchResults = []
                searchIndex = 0
              }
            }}
          />
          <button
            class={buttonClass('outline', 'icon-sm')}
            onclick={runSearch}
            title={getMessage('search')}
          >
            <Search class="size-4" />
          </button>
        </div>
        {#if searchResultNodes.length}
          <div class="brain-list mt-3">
            {#each searchResultNodes as result, index}
              <div
                class={cn(
                  'brain-list-row',
                  result.id === selectedNodeId && 'brain-list-row-active'
                )}
              >
                <button
                  class="brain-list-main"
                  onclick={() => selectSearchResult(result.id, index)}
                >
                  <span class="min-w-0">
                    <span class="block truncate font-medium">{result.name}</span>
                    <span class="block truncate text-[0.68rem] text-muted-foreground"
                      >{result.type}</span
                    >
                  </span>
                  <span class="text-[0.68rem] text-muted-foreground">{index + 1}</span>
                </button>
                <button
                  class={buttonClass('ghost', 'icon-xs')}
                  title={isPinned(result.id) ? getMessage('brainUnpin') : getMessage('brainPin')}
                  onclick={() => togglePin(result.id)}
                >
                  {#if isPinned(result.id)}
                    <PinOff class="size-3" />
                  {:else}
                    <Pin class="size-3" />
                  {/if}
                </button>
              </div>
            {/each}
          </div>
          <div class="mt-2 flex items-center justify-between text-xs text-muted-foreground">
            <span>{searchIndex + 1}/{searchResults.length}</span>
            <div class="flex gap-1">
              <button class={buttonClass('ghost', 'xs')} onclick={() => stepSearch(-1)}
                >{getMessage('brainPrev')}</button
              >
              <button class={buttonClass('ghost', 'xs')} onclick={() => stepSearch(1)}
                >{getMessage('brainNext')}</button
              >
            </div>
          </div>
        {/if}
      </section>

      <section class="brain-panel">
        <div class="brain-panel-heading justify-between">
          <span class="flex items-center gap-2">
            <BrainCircuit class="size-4" />
            {getMessage('brainAtlas')}
          </span>
          <button class={buttonClass('ghost', 'xs')} onclick={showAtlas}>
            {getMessage('brainSceneAtlas')}
          </button>
        </div>
        <div class="brain-compact-stats">
          <span>{getMessage('brainVisibleNodes')}: {summary.visibleNodeCount}</span>
          <span>{getMessage('brainVisibleLinks')}: {summary.visibleLinkCount}</span>
          <span>{getMessage('brainLoadedNodes')}: {summary.nodeCount}</span>
          <span>{getMessage('brainLoadedLinks')}: {summary.linkCount}</span>
        </div>
        {#if snapshot.status}
          <p class="mt-2 text-[0.68rem] text-muted-foreground">
            {getMessage('brainTotalConcepts', String(snapshot.status.concepts))}
            ·
            {getMessage('brainTotalProps', String(snapshot.status.propositions))}
          </p>
        {/if}
      </section>

      <section class="brain-panel">
        <div class="brain-panel-heading">
          <GitFork class="size-4" />
          {getMessage('brainAtlasClusters')}
        </div>
        <div class="brain-list">
          {#each summary.atlasClusters.slice(0, 10) as cluster}
            <button class="brain-list-row" onclick={() => focusTypeCluster(cluster.type)}>
              <span class="min-w-0 truncate" style={`color: ${cluster.color}`}>{cluster.type}</span>
              <span class="text-muted-foreground">{cluster.count}</span>
            </button>
          {/each}
        </div>
      </section>

      <section class="brain-panel">
        <div class="brain-panel-heading">
          <Route class="size-4" />
          {getMessage('brainFrequentHubs')}
        </div>
        <div class="brain-list">
          {#each summary.hubs.slice(0, 8) as hub}
            <button class="brain-list-row" onclick={() => selectNode(hub.id)}>
              <span class="min-w-0">
                <span class="block truncate font-medium">{hub.name}</span>
                <span class="block truncate text-[0.68rem] text-muted-foreground">{hub.type}</span>
              </span>
              <span class="text-muted-foreground">{hub.degree}</span>
            </button>
          {/each}
        </div>
      </section>

      <section class="brain-panel">
        <div class="brain-panel-heading">
          <Search class="size-4" />
          {getMessage('brainRecentMemories')}
        </div>
        <div class="brain-list">
          {#each summary.recentNodes as node}
            <button class="brain-list-row" onclick={() => selectNode(node.id)}>
              <span class="min-w-0 truncate">{node.name}</span>
              <span class="text-muted-foreground">{node.type}</span>
            </button>
          {/each}
        </div>
      </section>
    </aside>

    <section class="brain-stage">
      <div class="absolute inset-0" bind:this={container}></div>

      {#if loading && graphDataset.nodes.length === 0}
        <div
          class="absolute inset-0 z-10 flex items-center justify-center bg-background/70 backdrop-blur-sm"
        >
          <div
            class="flex items-center gap-2 rounded-md border border-border bg-card px-3 py-2 text-sm shadow-sm"
          >
            <LoaderCircle class="size-4 animate-spin" />
            {getMessage('loading')}
          </div>
        </div>
      {/if}

      <div class="brain-floating-toolbar">
        <button
          class={buttonClass('outline', 'icon-sm')}
          onclick={() => zoomBy(1.25)}
          title={getMessage('brainZoomIn')}
        >
          <ZoomIn class="size-4" />
        </button>
        <button
          class={buttonClass('outline', 'icon-sm')}
          onclick={() => zoomBy(0.8)}
          title={getMessage('brainZoomOut')}
        >
          <ZoomOut class="size-4" />
        </button>
        <button
          class={buttonClass('outline', 'icon-sm')}
          onclick={fitView}
          title={getMessage('brainFitView')}
        >
          <Maximize2 class="size-4" />
        </button>
        <button
          class={buttonClass('outline', 'icon-sm')}
          onclick={() => focusElement()}
          disabled={!selectedNodeId}
          title={getMessage('brainFocusSelected')}
        >
          <LocateFixed class="size-4" />
        </button>
      </div>

      <div class="brain-scene-chip">
        <span class={badgeClass('secondary')}>
          {#if graphDataset.scene === 'atlas'}
            {getMessage('brainSceneAtlas')}
          {:else if graphDataset.scene === 'path'}
            {getMessage('brainScenePath')}
          {:else}
            {getMessage('brainSceneFocus')}
          {/if}
        </span>
        <span
          >{getMessage('brainVisibleSummary', [
            String(summary.visibleNodeCount),
            String(summary.visibleLinkCount)
          ])}</span
        >
      </div>

      {#if breadcrumb.length}
        <nav class="brain-breadcrumb" aria-label={getMessage('brainBreadcrumb')}>
          <button class={buttonClass('ghost', 'xs')} onclick={showAtlas}>
            {getMessage('brainSceneAtlas')}
          </button>
          {#each breadcrumb as item, index}
            <span class="text-muted-foreground">/</span>
            <button class={buttonClass('ghost', 'xs')} onclick={() => goToBreadcrumb(index)}>
              <span class="max-w-28 truncate">{item.label}</span>
            </button>
          {/each}
        </nav>
      {/if}

      {#if summary.hiddenNodeCount || summary.hiddenEdgeCount}
        <div class="brain-budget-note">
          {getMessage('brainBudgetHidden', [
            String(summary.hiddenNodeCount),
            String(summary.hiddenEdgeCount)
          ])}
        </div>
      {/if}
    </section>

    <aside class="brain-inspector">
      <section class="brain-panel">
        <div class="brain-panel-heading justify-between">
          <span class="flex items-center gap-2">
            <FileBracesCorner class="size-4" />
            {getMessage('brainInspector')}
          </span>
          {#if selectedNode || selectedEdge || selectedSummaryGroup}
            <div class="flex items-center gap-1">
              {#if selectedNode || selectedEdge}
                <button
                  class={buttonClass('ghost', 'icon-xs')}
                  onclick={() => copyInspectorJson(selectedEdge || selectedNode)}
                  title={inspectorCopyState === 'copied'
                    ? getMessage('brainCopiedJson')
                    : getMessage('brainCopyJson')}
                  aria-label={inspectorCopyState === 'copied'
                    ? getMessage('brainCopiedJson')
                    : getMessage('brainCopyJson')}
                >
                  {#if inspectorCopyState === 'copied'}
                    <Check class="size-3" />
                  {:else}
                    <Copy class="size-3" />
                  {/if}
                </button>
              {/if}
              <button
                class={buttonClass('ghost', 'icon-xs')}
                onclick={() => {
                  if (selectedEdgeId) {
                    selectedEdgeId = ''
                  } else if (selectedSummaryId) {
                    selectedSummaryId = ''
                  } else {
                    showAtlas()
                  }
                  inspectorCopyState = 'idle'
                }}
                title={getMessage('close')}
                aria-label={getMessage('brainCloseInspector')}
              >
                <X class="size-3" />
              </button>
            </div>
          {/if}
        </div>

        {#if selectedEdge}
          <div class="space-y-3">
            <div>
              <span class={badgeClass(selectedEdge._virtual ? 'outline' : 'secondary')}>
                {selectedEdge._virtual
                  ? getMessage('brainVirtual')
                  : getMessage('brainProposition')}
              </span>
              <h2 class="mt-2 break-words text-sm font-semibold">{selectedEdge.predicate}</h2>
              <p class="mt-1 break-all text-xs text-muted-foreground">{selectedEdge.id}</p>
            </div>
            <div class="space-y-2 text-xs">
              <button
                class={buttonClass('ghost', 'xs', 'w-full justify-start')}
                onclick={() => selectNode(selectedEdge.subject)}
              >
                {getMessage(
                  'brainSubject',
                  graphData.nodes.get(selectedEdge.subject)?.name || selectedEdge.subject
                )}
              </button>
              <button
                class={buttonClass('ghost', 'xs', 'w-full justify-start')}
                onclick={() => selectNode(selectedEdge.object)}
              >
                {getMessage(
                  'brainObject',
                  graphData.nodes.get(selectedEdge.object)?.name || selectedEdge.object
                )}
              </button>
            </div>
            <div class={separatorClass()}></div>
            {@render DetailBlock(getMessage('brainAttributes'), selectedEdge.attributes)}
            {@render DetailBlock(getMessage('brainMetadata'), selectedEdge.metadata || {})}
          </div>
        {:else if selectedNode}
          <div class="space-y-3">
            <div>
              <span class={badgeClass('secondary')}>{selectedNode.type}</span>
              <h2 class="mt-2 break-words text-sm font-semibold">{selectedNode.name}</h2>
              <p class="mt-1 break-all text-xs text-muted-foreground">{selectedNode.id}</p>
            </div>
            <div class="flex flex-wrap gap-2">
              <button
                class={buttonClass('outline', 'xs')}
                onclick={() => expandNode(selectedNode.id)}
                disabled={expandingNodeId === selectedNode.id}
              >
                {#if expandingNodeId === selectedNode.id}
                  <LoaderCircle class="size-3 animate-spin" />
                {/if}
                {#if remainingRelatedCount > 0}
                  {getMessage('brainShowMoreRelatedWithCount', String(remainingRelatedCount))}
                {:else}
                  {getMessage('brainShowMoreRelated')}
                {/if}
              </button>
              <button class={buttonClass('ghost', 'xs')} onclick={() => togglePin(selectedNode.id)}>
                {#if isPinned(selectedNode.id)}
                  <PinOff class="size-3" />
                  {getMessage('brainUnpin')}
                {:else}
                  <Pin class="size-3" />
                  {getMessage('brainPin')}
                {/if}
              </button>
            </div>
            {#if graphDataset.relatedGroups.length}
              <div class={separatorClass()}></div>
              <div>
                <div class="mb-2 text-xs font-medium text-muted-foreground">
                  {getMessage('brainRelatedMemories')}
                </div>
                {@render RelatedGroups(graphDataset.relatedGroups)}
              </div>
            {/if}
            <div class={separatorClass()}></div>
            {@render DetailBlock(getMessage('brainAttributes'), selectedNode.attributes)}
            {@render DetailBlock(getMessage('brainMetadata'), selectedNode.metadata || {})}
          </div>
        {:else if selectedSummaryGroup}
          <div class="space-y-3">
            <div>
              <span class={badgeClass('outline')}>{getMessage('brainSummaryNode')}</span>
              <h2 class="mt-2 break-words text-sm font-semibold">
                {relatedGroupTitle(selectedSummaryGroup)}
              </h2>
              <p class="mt-1 text-xs text-muted-foreground">
                {getMessage('brainRelatedCount', String(selectedSummaryGroup.total))}
              </p>
            </div>
            {@render RelatedGroups([selectedSummaryGroup])}
          </div>
        {:else}
          <div class="space-y-3 text-xs text-muted-foreground">
            <p>{getMessage('brainAtlasSummary')}</p>
            {@render RelatedGroups(graphDataset.relatedGroups.slice(0, 3))}
          </div>
        {/if}
      </section>

      <section class="brain-panel">
        <div class="brain-panel-heading justify-between">
          <span class="flex items-center gap-2">
            <Route class="size-4" />
            {getMessage('brainPath')}
          </span>
          {#if pinnedNodes.length}
            <button class={buttonClass('ghost', 'xs')} onclick={clearPinned}>
              {getMessage('brainClearPins')}
            </button>
          {/if}
        </div>
        {#if pinnedNodes.length}
          <div class="brain-list">
            {#each pinnedNodes as node}
              <div
                class={cn('brain-list-row', node.id === selectedNodeId && 'brain-list-row-active')}
              >
                <button class="brain-list-main" onclick={() => selectNode(node.id)}>
                  <span class="min-w-0 truncate">{node.name}</span>
                </button>
                <button
                  class={buttonClass('ghost', 'icon-xs')}
                  title={getMessage('brainUnpin')}
                  onclick={() => togglePin(node.id)}
                >
                  <PinOff class="size-3" />
                </button>
              </div>
            {/each}
          </div>
          {#if pinnedNodes.length >= 2}
            <button class={buttonClass('outline', 'sm', 'mt-3 w-full')} onclick={showPinnedPath}>
              <Route class="size-4" />
              {getMessage('brainViewPinnedPath')}
            </button>
          {/if}
        {:else}
          <p class="text-xs text-muted-foreground">{getMessage('brainNoPins')}</p>
        {/if}
      </section>

      <section class="brain-panel">
        <button
          class={buttonClass('ghost', 'sm', 'w-full justify-between')}
          onclick={() => (queryOpen = !queryOpen)}
        >
          <span class="flex items-center gap-2">
            <Braces class="size-4" />
            {getMessage('brainAdvanced')}
          </span>
          <span>{queryOpen ? getMessage('brainHide') : getMessage('brainShow')}</span>
        </button>
        {#if queryOpen}
          <textarea class={textareaClass('mt-3 min-h-36 font-mono text-xs')} bind:value={kipCommand}
          ></textarea>
          <button
            class={buttonClass('outline', 'sm', 'mt-2 w-full')}
            onclick={runKipQuery}
            disabled={loading}
          >
            {getMessage('brainRunReadonly')}
          </button>
          {#if queryOutput}
            <pre class="brain-json language-json mt-3 max-h-64"><code class="language-json"
                >{@html highlightJsonString(queryOutput)}</code
              ></pre>
          {/if}
        {/if}
      </section>
    </aside>
    {#if inboxOpen}
      <div class="brain-inbox-overlay">
        <button class={buttonClass('ghost', 'sm')} onclick={() => (inboxOpen = false)}
          >{getMessage('brainBackToGraph')}</button
        >
        {#key `${settings.baseUrl}|${settings.spaceId}|${settings.token}`}
          <Inbox {settings} />
        {/key}
      </div>
    {/if}
  </main>
</div>

{#snippet DetailBlock(title: string, value: unknown)}
  <div>
    <div class="mb-1 text-xs font-medium text-muted-foreground">{title}</div>
    <pre class="brain-json language-json"><code class="language-json"
        >{@html highlightJson(value)}</code
      ></pre>
  </div>
{/snippet}

{#snippet RelatedGroups(groups: RelatedMemoryGroup[])}
  <div class="space-y-3">
    {#each groups as group}
      <div class="brain-related-group">
        <button class="brain-related-heading" onclick={() => selectRelatedGroup(group)}>
          <span class="min-w-0 truncate">{relatedGroupTitle(group)}</span>
          <span class="text-muted-foreground">{group.total}</span>
        </button>
        <div class="brain-list mt-1">
          {#each group.items as item}
            <button class="brain-list-row" onclick={() => selectRelatedItem(item)}>
              <span class="min-w-0">
                <span class="block truncate font-medium">{item.label}</span>
                <span class="block truncate text-[0.68rem] text-muted-foreground">
                  {item.detail}
                </span>
              </span>
              {#if item.type}
                <span class="shrink-0 text-[0.68rem]" style={`color: ${typeColor(item.type)}`}>
                  {item.type}
                </span>
              {/if}
            </button>
          {/each}
        </div>
        {#if group.hidden > 0}
          <button
            class={buttonClass('ghost', 'xs', 'mt-1 w-full justify-start')}
            onclick={() => selectRelatedGroup(group)}
          >
            {getMessage('brainMoreInGroup', String(group.hidden))}
          </button>
        {/if}
      </div>
    {/each}
  </div>
{/snippet}

<style>
  .brain-inbox-overlay {
    position: absolute;
    inset: 0;
    z-index: 20;
    display: flex;
    flex-direction: column;
    background: var(--background);
  }
  .brain-inbox-overlay > :global(button) {
    align-self: flex-start;
    margin: 0.5rem 1rem 0;
  }

  .brain-shell {
    display: grid;
    grid-template-rows: auto auto minmax(0, 1fr);
    width: 100vw;
    height: 100vh;
    overflow: hidden;
    background:
      radial-gradient(circle at 10% 0%, rgba(20, 184, 166, 0.08), transparent 24rem),
      radial-gradient(circle at 90% 20%, rgba(245, 158, 11, 0.07), transparent 22rem),
      var(--background);
    color: var(--foreground);
  }

  .brain-shell-embedded {
    width: 100%;
    height: 100%;
    grid-template-rows: minmax(0, 1fr);
    background: var(--background);
  }

  .brain-shell-embedded .brain-workspace {
    height: 100%;
  }

  .brain-topbar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 1rem;
    border-bottom: 1px solid var(--border);
    background: color-mix(in oklch, var(--background) 86%, transparent);
    padding: 0.75rem 1rem;
    backdrop-filter: blur(16px);
  }

  .brain-workspace {
    position: relative;
    display: grid;
    grid-template-columns: minmax(17rem, 20rem) minmax(0, 1fr) minmax(18rem, 22rem);
    min-height: 0;
    overflow: hidden;
  }

  .brain-sidebar,
  .brain-inspector {
    display: flex;
    min-height: 0;
    flex-direction: column;
    gap: 0.75rem;
    overflow-y: auto;
    border-color: var(--border);
    background: color-mix(in oklch, var(--background) 90%, transparent);
    padding: 0.75rem;
  }

  .brain-sidebar {
    border-right-width: 1px;
  }

  .brain-inspector {
    border-left-width: 1px;
  }

  .brain-panel {
    border-bottom: 1px solid var(--border);
    padding-bottom: 0.75rem;
  }

  .brain-panel:last-child {
    border-bottom: 0;
  }

  .brain-panel-heading {
    margin-bottom: 0.65rem;
    display: flex;
    align-items: center;
    gap: 0.5rem;
    font-size: 0.78rem;
    font-weight: 600;
  }

  .brain-field {
    margin-bottom: 0.55rem;
    display: grid;
    gap: 0.28rem;
    font-size: 0.72rem;
    color: var(--muted-foreground);
  }

  .brain-list {
    display: grid;
    gap: 0.25rem;
  }

  .brain-list-row {
    display: flex;
    min-width: 0;
    width: 100%;
    align-items: center;
    justify-content: space-between;
    gap: 0.5rem;
    border-radius: 0.375rem;
    padding: 0.38rem 0.45rem;
    color: var(--foreground);
    font-size: 0.72rem;
    text-align: left;
    transition:
      background-color 180ms ease,
      color 180ms ease,
      transform 180ms ease;
  }

  button.brain-list-row,
  .brain-list-main,
  .brain-related-heading {
    cursor: pointer;
  }

  .brain-list-main {
    display: flex;
    min-width: 0;
    flex: 1;
    align-items: center;
    justify-content: space-between;
    gap: 0.5rem;
    text-align: left;
  }

  .brain-list-row:hover,
  .brain-related-heading:hover {
    background: color-mix(in oklch, var(--muted) 56%, transparent);
  }

  .brain-list-row:active,
  .brain-related-heading:active {
    transform: translateY(1px);
  }

  .brain-list-row-active {
    background: color-mix(in oklch, var(--primary) 16%, transparent);
    color: var(--primary);
  }

  .brain-compact-stats {
    display: grid;
    grid-template-columns: repeat(2, minmax(0, 1fr));
    gap: 0.35rem;
    font-size: 0.68rem;
    color: var(--muted-foreground);
    font-variant-numeric: tabular-nums;
  }

  .brain-stage {
    position: relative;
    min-width: 0;
    min-height: 0;
    overflow: hidden;
    background:
      linear-gradient(color-mix(in oklch, var(--border) 42%, transparent) 1px, transparent 1px),
      linear-gradient(
        90deg,
        color-mix(in oklch, var(--border) 42%, transparent) 1px,
        transparent 1px
      ),
      color-mix(in oklch, var(--background) 92%, var(--card));
    background-size:
      40px 40px,
      40px 40px,
      auto;
  }

  .brain-stage :global(canvas) {
    display: block;
    width: 100%;
    height: 100%;
  }

  .brain-floating-toolbar {
    position: absolute;
    top: 0.75rem;
    right: 0.75rem;
    display: flex;
    gap: 0.35rem;
    border: 1px solid var(--border);
    border-radius: 0.5rem;
    background: color-mix(in oklch, var(--background) 88%, transparent);
    padding: 0.35rem;
    backdrop-filter: blur(12px);
  }

  .brain-scene-chip {
    position: absolute;
    left: 0.75rem;
    top: 0.75rem;
    display: flex;
    max-width: calc(100% - 7rem);
    align-items: center;
    gap: 0.35rem;
    border: 1px solid var(--border);
    border-radius: 0.5rem;
    background: color-mix(in oklch, var(--background) 88%, transparent);
    padding: 0.35rem;
    font-size: 0.72rem;
    color: var(--muted-foreground);
    backdrop-filter: blur(12px);
  }

  .brain-breadcrumb {
    position: absolute;
    left: 0.75rem;
    top: 3.55rem;
    display: flex;
    max-width: min(34rem, calc(100% - 1.5rem));
    align-items: center;
    gap: 0.15rem;
    overflow: hidden;
    border: 1px solid var(--border);
    border-radius: 0.5rem;
    background: color-mix(in oklch, var(--background) 88%, transparent);
    padding: 0.28rem;
    backdrop-filter: blur(12px);
  }

  .brain-budget-note {
    position: absolute;
    left: 0.75rem;
    bottom: 0.75rem;
    max-width: min(26rem, calc(100% - 12rem));
    border: 1px solid var(--border);
    border-radius: 0.5rem;
    background: color-mix(in oklch, var(--background) 88%, transparent);
    padding: 0.45rem 0.6rem;
    font-size: 0.7rem;
    color: var(--muted-foreground);
    backdrop-filter: blur(12px);
  }

  .brain-related-group {
    display: grid;
    gap: 0.25rem;
  }

  .brain-related-heading {
    display: flex;
    width: 100%;
    align-items: center;
    justify-content: space-between;
    gap: 0.5rem;
    border-radius: 0.375rem;
    padding: 0.28rem 0.42rem;
    font-size: 0.72rem;
    font-weight: 600;
    text-align: left;
    transition:
      background-color 180ms ease,
      transform 180ms ease;
  }

  .brain-json {
    overflow: auto;
    border-radius: 0.375rem;
    border: 1px solid var(--border);
    background: color-mix(in oklch, var(--muted) 65%, transparent);
    padding: 0.55rem;
    font-family:
      'SF Mono', Monaco, 'Cascadia Code', 'Roboto Mono', Consolas, 'Courier New', monospace;
    font-size: 0.68rem;
    line-height: 1.45;
    white-space: pre-wrap;
    word-break: break-word;
  }

  .brain-json code {
    display: block;
    color: color-mix(in oklch, var(--foreground) 92%, transparent);
  }

  .brain-json :global(.token.property) {
    color: #2563eb;
  }

  .brain-json :global(.token.string) {
    color: #0f766e;
  }

  .brain-json :global(.token.number) {
    color: #c2410c;
  }

  .brain-json :global(.token.boolean),
  .brain-json :global(.token.null) {
    color: #7c3aed;
  }

  .brain-json :global(.token.punctuation),
  .brain-json :global(.token.operator) {
    color: color-mix(in oklch, var(--muted-foreground) 88%, transparent);
  }

  :global(.brain-tooltip) {
    display: grid;
    gap: 0.15rem;
    padding: 0.55rem 0.75rem;
  }

  :global(.brain-tooltip strong) {
    max-width: 16rem;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 0.75rem;
    color: rgba(255, 255, 255, 0.92);
  }

  :global(.brain-tooltip span) {
    font-size: 0.65rem;
    color: rgba(255, 255, 255, 0.58);
  }

  :global(.g6-minimap) {
    overflow: hidden !important;
    border: 1px solid color-mix(in oklch, var(--border) 80%, transparent) !important;
    border-radius: 0.5rem !important;
    background: color-mix(in oklch, var(--background) 90%, transparent) !important;
  }

  @media (max-width: 980px) {
    .brain-topbar {
      flex-wrap: wrap;
    }

    .brain-workspace {
      width: 100%;
      min-width: 0;
      grid-template-columns: minmax(0, 1fr);
      grid-template-rows: auto minmax(28rem, 1fr) auto;
      overflow-x: hidden;
      overflow-y: auto;
    }

    .brain-stage,
    .brain-sidebar,
    .brain-inspector {
      width: 100%;
      min-width: 0;
      max-height: none;
      border: 0;
      border-bottom: 1px solid var(--border);
    }

    .brain-floating-toolbar,
    .brain-scene-chip,
    .brain-breadcrumb,
    .brain-budget-note {
      max-width: calc(100% - 1rem);
      flex-wrap: wrap;
    }

    .brain-floating-toolbar {
      right: 0.5rem;
    }

    .brain-scene-chip,
    .brain-breadcrumb,
    .brain-budget-note {
      left: 0.5rem;
    }

    .brain-breadcrumb {
      top: 3.5rem;
    }
  }
</style>
