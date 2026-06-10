<script lang="ts">
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
  import {
    CanvasEvent,
    EdgeEvent,
    Graph,
    NodeEvent,
    type ComboData,
    type EdgeData,
    type NodeData
  } from '@antv/g6'
  import { RadialLayout } from '@antv/layout'
  import {
    Braces,
    Check,
    Copy,
    Database,
    Eye,
    EyeOff,
    GitFork,
    LoaderCircle,
    LocateFixed,
    Maximize2,
    BrainCircuit,
    RefreshCw,
    Search,
    Settings,
    FileBracesCorner,
    SlidersHorizontal,
    X,
    ZoomIn,
    ZoomOut
  } from '@lucide/svelte'
  import { onMount, tick } from 'svelte'

  type ViewMode = 'overview' | 'focus' | 'full'
  type EdgeMode = 'focus' | 'all' | 'none'
  type LabelMode = 'smart' | 'all' | 'none'

  interface RenderedDataset {
    nodes: NodeData[]
    edges: EdgeData[]
    combos: ComboData[]
  }

  const initialSettings: BrainGraphSettings = {
    ...defaultSettings,
    spaceId: ANDA_BOT_SPACE_ID
  }

  let settings = $state<BrainGraphSettings>(initialSettings)
  let graphData = new BrainGraphData(new BrainApi(initialSettings))
  let graph: Graph | null = null
  let container: HTMLDivElement | null = $state(null)
  let resizeObserver: ResizeObserver | null = null
  let resizeTimer: ReturnType<typeof setTimeout> | null = null
  let graphRendered = $state(false)

  let loading = $state(false)
  let saving = $state(false)
  let errorMessage = $state('')
  let statusText = $state('Idle')
  let settingsOpen = $state(false)
  let queryOpen = $state(false)
  let inspectorCopyState = $state<'idle' | 'copied'>('idle')

  let selectedNodeId = $state('')
  let selectedEdgeId = $state('')
  let expandingNodeId = $state('')
  let searchQuery = $state('')
  let searchResults = $state<string[]>([])
  let searchIndex = $state(0)
  let kipCommand = $state(`FIND(?link)
WHERE {
  ?link (?s, "belongs_to_domain", ?o)
}
LIMIT 6000`)
  let queryOutput = $state('')

  let viewMode = $state<ViewMode>('overview')
  let edgeMode = $state<EdgeMode>('focus')
  let labelMode = $state<LabelMode>('smart')
  let maxVisibleNodes = $state(260)
  let focusRadius = $state(1)
  let typeFilter = $state('')
  let predicateFilter = $state('')
  let inspectorCopyTimer: ReturnType<typeof setTimeout> | null = null

  const snapshot = $derived.by<GraphSnapshot>(() => {
    return graphData.snapshot()
  })
  const graphDataset = $derived.by<RenderedDataset>(() => {
    return buildGraphDataset(snapshot)
  })
  const selectedNode = $derived.by<Concept | null>(() =>
    selectedNodeId ? graphData.nodes.get(selectedNodeId) || null : null
  )
  const selectedEdge = $derived.by<Proposition | null>(() =>
    selectedEdgeId ? graphData.links.get(selectedEdgeId) || null : null
  )
  const summary = $derived.by(() =>
    graphData.summary(graphDataset.nodes.length, graphDataset.edges.length)
  )

  $effect(() => applyAppearanceTheme(settings.appearanceTheme))

  let syncTimer: ReturnType<typeof setTimeout> | null = null
  $effect(() => {
    graphDataset
    if (!graph || !graphRendered) {
      return
    }
    if (syncTimer) {
      clearTimeout(syncTimer)
    }
    syncTimer = setTimeout(() => {
      syncTimer = null
      syncGraph().catch((error) => {
        errorMessage = errorToMessage(error)
      })
    }, 120)
  })

  onMount(() => {
    initGraph()
    loadBrainGraphSettings()
      .then(async (saved) => {
        settings = saved
        graphData.setApi(new BrainApi(settings))
        await loadGraph()
      })
      .catch((error) => {
        errorMessage = errorToMessage(error)
        statusText = 'Settings unavailable'
      })

    return () => {
      if (syncTimer) {
        clearTimeout(syncTimer)
      }
      if (resizeTimer) {
        clearTimeout(resizeTimer)
      }
      if (inspectorCopyTimer) {
        clearTimeout(inspectorCopyTimer)
      }
      resizeObserver?.disconnect()
      resizeObserver = null
      graph?.destroy()
      graph = null
    }
  })

  function initGraph() {
    if (!container || graph) {
      return
    }

    const stage = graphStageSize()
    graph = new Graph({
      container,
      width: stage.width,
      height: stage.height,
      autoFit: 'view',
      padding: 24,
      animation: true,
      theme: 'dark',
      node: {
        style: {
          zIndex: 80,
          labelFontSize: 10,
          labelPlacement: 'bottom',
          labelOffsetY: 4,
          labelBackground: true,
          labelBackgroundRadius: 3,
          labelBackgroundLineWidth: 0,
          halo: true,
          haloLineWidth: 5
        },
        state: {
          selected: {
            lineWidth: 3,
            halo: true,
            haloLineWidth: 12,
            haloStroke: 'rgba(20, 184, 166, 0.24)'
          },
          highlight: {
            lineWidth: 3,
            halo: true,
            haloLineWidth: 10,
            haloStroke: 'rgba(245, 158, 11, 0.24)'
          },
          dim: {
            opacity: 0.22,
            labelOpacity: 0.18
          },
          loading: {
            lineWidth: 4,
            lineDash: [4, 4],
            stroke: '#f59e0b'
          }
        }
      },
      edge: {
        type: 'line',
        style: {
          zIndex: 10,
          endArrow: true,
          endArrowSize: 3,
          strokeOpacity: 0.55
        },
        state: {
          selected: {
            lineWidth: 2.5,
            stroke: '#14b8a6',
            strokeOpacity: 1
          },
          highlight: {
            lineWidth: 2,
            stroke: '#f59e0b',
            strokeOpacity: 1
          },
          dim: {
            opacity: 0.18
          }
        }
      },
      combo: {
        type: 'circle',
        style: {
          lineDash: [5, 5],
          lineWidth: 1,
          labelFill: 'rgba(245, 245, 244, 0.68)',
          labelFontSize: 11,
          labelPlacement: 'top'
        }
      },
      layout: {
        ...buildLayoutOptions(stage)
      },
      behaviors: [
        { type: 'drag-canvas', key: 'drag-canvas' },
        { type: 'zoom-canvas', key: 'zoom-canvas', sensitivity: 1.12 },
        { type: 'drag-element', key: 'drag-element' },
        { type: 'hover-activate', key: 'hover-activate' },
        {
          type: 'click-select',
          key: 'click-select',
          degree: 0,
          multiple: false,
          state: 'selected'
        },
        { type: 'optimize-viewport-transform', key: 'optimize-viewport', debounce: 240 },
        { type: 'auto-adapt-label', key: 'auto-adapt-label', throttle: 260, padding: 2 }
      ],
      plugins: [
        {
          type: 'minimap',
          key: 'minimap',
          size: [168, 112],
          position: 'left-bottom'
        },
        {
          type: 'tooltip',
          key: 'tooltip',
          trigger: 'hover',
          getContent: (_event: unknown, items: Array<{ data?: unknown }>) => {
            const item = items?.[0]?.data
            return buildTooltip(item)
          },
          style: {
            '.tooltip': {
              background: 'rgba(28, 25, 23, 0.96)',
              border: '1px solid rgba(255, 255, 255, 0.12)',
              'border-radius': '8px',
              padding: '0',
              'box-shadow': '0 12px 36px rgba(0, 0, 0, 0.34)',
              'backdrop-filter': 'blur(10px)',
              'pointer-events': 'none'
            }
          }
        }
      ]
    })

    graph.on(NodeEvent.CLICK, (event: any) => {
      const id = event.target?.id
      if (id) {
        selectNode(id)
      }
    })
    graph.on(NodeEvent.DBLCLICK, (event: any) => {
      const id = event.target?.id
      if (id) {
        expandNode(id)
      }
    })
    graph.on(EdgeEvent.CLICK, (event: any) => {
      const id = event.target?.id
      if (id) {
        selectedEdgeId = id
        selectedNodeId = ''
      }
    })
    graph.on(CanvasEvent.CLICK, () => {
      selectedEdgeId = ''
    })

    resizeObserver = new ResizeObserver(() => scheduleGraphResize())
    resizeObserver.observe(container)

    syncGraph()
      .then(() => {
        graphRendered = true
      })
      .catch((error) => {
        errorMessage = errorToMessage(error)
      })
  }

  async function syncGraph() {
    if (!graph) {
      return
    }
    resizeGraphCanvas()
    graph.setOptions({ layout: buildLayoutOptions() as any })
    graph.setData(graphDataset)
    if (graphDataset.nodes.length > 300) {
      graph.setOptions({ animation: false })
    }
    await graph.render()
    syncElementStates()
  }

  function scheduleGraphResize() {
    if (resizeTimer) {
      clearTimeout(resizeTimer)
    }
    resizeTimer = setTimeout(() => {
      resizeTimer = null
      if (!graph || !graphRendered) {
        resizeGraphCanvas()
        return
      }
      syncGraph()
        .then(() => fitView())
        .catch((error) => {
          errorMessage = errorToMessage(error)
        })
    }, 120)
  }

  function resizeGraphCanvas(): { width: number; height: number } {
    const stage = graphStageSize()
    graph?.resize(stage.width, stage.height)
    return stage
  }

  function graphStageSize(): { width: number; height: number } {
    const rect = container?.getBoundingClientRect()
    return {
      width: Math.max(320, Math.floor(rect?.width || window.innerWidth || 1024)),
      height: Math.max(320, Math.floor(rect?.height || window.innerHeight || 720))
    }
  }

  function buildLayoutOptions(stage = graphStageSize()) {
    const comboCount = Math.max(1, graphDataset.combos.length)
    const comboScale = Math.sqrt(comboCount)
    const innerWidth = Math.max(
      360,
      Math.min(stage.width * 0.86, (stage.width * 1.18) / comboScale)
    )
    const innerHeight = Math.max(
      320,
      Math.min(stage.height * 0.86, (stage.height * 1.18) / comboScale)
    )
    const spacing = Math.max(28, Math.min(84, Math.min(stage.width, stage.height) / 14))
    const comboPadding = Math.max(20, Math.min(52, Math.min(stage.width, stage.height) / 18))

    return {
      type: 'combo-combined',
      comboPadding,
      spacing,
      innerLayout: new RadialLayout({
        width: innerWidth,
        height: innerHeight,
        linkDistance: Math.max(80, Math.min(160, Math.min(innerWidth, innerHeight) / 4)),
        preventOverlap: true,
        strictRadial: false,
        nodeSize: 52,
        nodeSpacing: 18
      })
    }
  }

  function syncElementStates() {
    if (!graph) {
      return
    }
    const searchSet = new Set(searchResults)
    for (const node of graphDataset.nodes) {
      const states: string[] = []
      if (node.id === selectedNodeId) {
        states.push('selected')
      } else if (searchSet.has(node.id)) {
        states.push('highlight')
      } else if (searchSet.size > 0) {
        states.push('dim')
      }
      if (node.id === expandingNodeId) {
        states.push('loading')
      }
      graph.setElementState(String(node.id), states)
    }
    if (selectedEdgeId) {
      graph.setElementState(selectedEdgeId, ['selected'])
    }
  }

  async function loadGraph() {
    loading = true
    errorMessage = ''
    statusText = 'Loading graph'
    try {
      graphData.clear()
      graphData.setApi(new BrainApi(settings))
      const stats = await graphData.loadOverview()
      statusText = graphData.status
        ? `Overview ${graphData.nodes.size}/${graphData.status.concepts} nodes`
        : `Overview ${stats.concepts} nodes`
      await tick()
      await fitView()
    } catch (error) {
      errorMessage = errorToMessage(error)
      statusText = 'Load failed'
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
      syncElementStates()
      return
    }

    loading = true
    errorMessage = ''
    statusText = 'Searching'
    try {
      const results = await graphData.searchConcepts(query)
      searchResults = results.map((result) => result.id)
      searchIndex = 0
      if (results[0]) {
        selectedNodeId = results[0].id
        selectedEdgeId = ''
        viewMode = 'focus'
        await graphData.expandConcept(results[0].id).catch(() => undefined)
      }
      statusText = `${results.length} search results`
      await tick()
      if (results[0]) {
        await focusElement(results[0].id)
      }
    } catch (error) {
      errorMessage = errorToMessage(error)
      statusText = 'Search failed'
    } finally {
      loading = false
    }
  }

  function stepSearch(delta: number) {
    if (!searchResults.length) {
      return
    }
    searchIndex = (searchIndex + delta + searchResults.length) % searchResults.length
    selectedNodeId = searchResults[searchIndex] || ''
    selectedEdgeId = ''
    viewMode = 'focus'
    if (selectedNodeId) {
      focusElement(selectedNodeId)
    }
  }

  async function runKipQuery() {
    const command = kipCommand.trim()
    if (!command) {
      return
    }
    loading = true
    errorMessage = ''
    statusText = 'Running KIP'
    try {
      const result = await graphData.executeQuery(command)
      queryOutput = JSON.stringify(result, null, 2)
      statusText = 'KIP complete'
    } catch (error) {
      errorMessage = errorToMessage(error)
      statusText = 'KIP failed'
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
        throw new Error('Clipboard is unavailable')
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
      viewMode = 'focus'
      selectedNodeId = id
      selectedEdgeId = ''
      await tick()
      await focusElement(id)
    } catch (error) {
      errorMessage = errorToMessage(error)
    } finally {
      expandingNodeId = ''
    }
  }

  function selectNode(id: string) {
    selectedNodeId = id
    selectedEdgeId = ''
    if (viewMode === 'overview') {
      edgeMode = 'focus'
    }
  }

  async function focusElement(id = selectedNodeId) {
    if (!graph || !id) {
      return
    }
    await tick()
    graph.focusElement(id, { duration: 300 })
  }

  async function fitView() {
    if (!graph) {
      return
    }
    await tick()
    graph.fitView(undefined, { duration: 260 })
  }

  function zoomBy(scale: number) {
    if (!graph) {
      return
    }
    const zoom = graph.getZoom()
    graph.zoomTo(Math.max(0.08, Math.min(zoom * scale, 5)), { duration: 180 })
  }

  function resetFilters() {
    typeFilter = ''
    predicateFilter = ''
    maxVisibleNodes = 260
    focusRadius = 1
    edgeMode = 'focus'
    labelMode = 'smart'
  }

  function buildGraphDataset(source: GraphSnapshot): RenderedDataset {
    const degree = graphData.degreeByNode()
    const searchSet = new Set(searchResults)
    const typeFilteredNodes = source.nodes.filter((node) => !typeFilter || node.type === typeFilter)
    const nodeIdsByFilter = new Set(typeFilteredNodes.map((node) => node.id))
    const rawLinks = source.links.filter((link) => {
      if (predicateFilter && link.predicate !== predicateFilter) {
        return false
      }
      return nodeIdsByFilter.has(link.subject) && nodeIdsByFilter.has(link.object)
    })

    let keepIds: Set<string>
    if (viewMode === 'focus' && selectedNodeId) {
      keepIds = graphData.neighborIds(selectedNodeId, focusRadius)
    } else if (searchSet.size > 0) {
      keepIds = new Set<string>()
      for (const id of searchSet) {
        for (const neighbor of graphData.neighborIds(id, focusRadius)) {
          keepIds.add(neighbor)
        }
      }
    } else if (viewMode === 'full') {
      keepIds = new Set(typeFilteredNodes.map((node) => node.id))
    } else {
      keepIds = new Set(
        typeFilteredNodes
          .slice()
          .sort(
            (left, right) =>
              (degree.get(right.id) || 0) - (degree.get(left.id) || 0) ||
              left.name.localeCompare(right.name)
          )
          .slice(0, maxVisibleNodes)
          .map((node) => node.id)
      )
    }

    if (selectedNodeId && nodeIdsByFilter.has(selectedNodeId)) {
      keepIds.add(selectedNodeId)
    }
    for (const id of searchSet) {
      if (nodeIdsByFilter.has(id)) {
        keepIds.add(id)
      }
    }

    keepIds = capNodeSet(keepIds, maxVisibleNodes, degree, searchSet)

    const visibleNodes = typeFilteredNodes.filter((node) => keepIds.has(node.id))
    const visibleIdSet = new Set(visibleNodes.map((node) => node.id))
    const focusIds =
      selectedNodeId && visibleIdSet.has(selectedNodeId)
        ? graphData.neighborIds(selectedNodeId, focusRadius)
        : new Set<string>()

    let visibleLinks = rawLinks.filter(
      (link) => visibleIdSet.has(link.subject) && visibleIdSet.has(link.object)
    )
    if (edgeMode === 'none') {
      visibleLinks = []
    } else if (edgeMode === 'focus' && selectedNodeId) {
      visibleLinks = visibleLinks.filter(
        (link) => focusIds.has(link.subject) || focusIds.has(link.object) || link._virtual
      )
    }
    visibleLinks = visibleLinks
      .slice()
      .sort((left, right) => linkPriority(right, degree) - linkPriority(left, degree))
      .slice(0, viewMode === 'full' ? 3200 : 1400)

    const typeCounts = new Map<string, number>()
    for (const node of visibleNodes) {
      typeCounts.set(node.type, (typeCounts.get(node.type) || 0) + 1)
    }
    const combos = Array.from(typeCounts.entries())
      .filter(([, count]) => count >= 2)
      .slice(0, 28)
      .map(([type, count]) => makeCombo(type, count))
    const comboIds = new Set(combos.map((combo) => combo.id))

    return {
      nodes: visibleNodes.map((node) =>
        makeNode(node, {
          degree: degree.get(node.id) || 0,
          selected: node.id === selectedNodeId,
          searched: searchSet.has(node.id),
          combo: comboIds.has(comboId(node.type)) ? comboId(node.type) : undefined,
          visibleCount: visibleNodes.length
        })
      ),
      edges: visibleLinks.map((link) =>
        makeEdge(link, {
          selected: link.id === selectedEdgeId,
          focused:
            selectedNodeId && (link.subject === selectedNodeId || link.object === selectedNodeId),
          visibleCount: visibleNodes.length
        })
      ),
      combos
    }
  }

  function capNodeSet(
    ids: Set<string>,
    limit: number,
    degree: Map<string, number>,
    searchSet: Set<string>
  ): Set<string> {
    if (ids.size <= limit) {
      return ids
    }
    return new Set(
      Array.from(ids)
        .sort((left, right) => {
          const leftPriority =
            (left === selectedNodeId ? 100000 : 0) + (searchSet.has(left) ? 50000 : 0)
          const rightPriority =
            (right === selectedNodeId ? 100000 : 0) + (searchSet.has(right) ? 50000 : 0)
          return (
            rightPriority - leftPriority ||
            (degree.get(right) || 0) - (degree.get(left) || 0) ||
            left.localeCompare(right)
          )
        })
        .slice(0, limit)
    )
  }

  function makeNode(
    node: Concept,
    options: {
      degree: number
      selected: boolean
      searched: boolean
      combo?: string
      visibleCount: number
    }
  ): NodeData {
    const typeDef = node.type === '$ConceptType' || node.type === '$PropositionType'
    const color = typeColor(node.type)
    const showLabel =
      labelMode === 'all' ||
      (labelMode === 'smart' &&
        (options.selected ||
          options.searched ||
          typeDef ||
          options.degree >= 4 ||
          options.visibleCount < 90))
    const size = typeDef ? [66, 42] : Math.max(18, Math.min(44, 18 + Math.sqrt(options.degree) * 6))
    return {
      id: node.id,
      type: typeDef ? 'ellipse' : 'circle',
      combo: options.combo,
      data: node,
      style: {
        size,
        fill: hexToRgba(color, typeDef ? 0.2 : 0.12),
        stroke: color,
        lineWidth: options.selected ? 3 : options.searched ? 2.5 : 1.2,
        cursor: node._isExpanding ? 'wait' : 'pointer',
        labelText: showLabel ? node.name : '',
        labelFill: 'rgba(245, 245, 244, 0.9)',
        labelBackgroundFill: 'rgba(28, 25, 23, 0.78)',
        haloStroke: hexToRgba(color, 0.16)
      }
    } as unknown as NodeData
  }

  function makeEdge(
    link: Proposition,
    options: { selected: boolean; focused: boolean | ''; visibleCount: number }
  ): EdgeData {
    const color = link._virtual ? '#a78bfa' : predicateColor(link.predicate)
    const showLabel =
      labelMode !== 'none' && (options.selected || options.focused || options.visibleCount < 90)
    return {
      id: link.id,
      source: link.subject,
      target: link.object,
      data: link,
      style: {
        stroke: color,
        strokeOpacity: link._virtual ? 0.25 : options.focused ? 0.84 : 0.48,
        lineWidth: options.selected ? 2.4 : options.focused ? 1.6 : 0.9,
        lineDash: link._virtual ? [4, 4] : undefined,
        endArrow: !link._virtual,
        labelText: showLabel ? link.predicate : '',
        labelFontSize: 9,
        labelFill: 'rgba(245, 245, 244, 0.64)',
        labelBackground: true,
        labelBackgroundFill: 'rgba(28, 25, 23, 0.62)',
        labelBackgroundRadius: 3,
        cursor: 'pointer'
      }
    } as unknown as EdgeData
  }

  function makeCombo(type: string, count: number): ComboData {
    const color = typeColor(type)
    return {
      id: comboId(type),
      type: 'circle',
      style: {
        labelText: `${type} ${count}`,
        fill: hexToRgba(color, 0.06),
        stroke: hexToRgba(color, 0.52),
        labelFill: hexToRgba(color, 0.92),
        cursor: 'default'
      }
    } as ComboData
  }

  function linkPriority(link: Proposition, degree: Map<string, number>): number {
    return (
      (link._virtual ? -1000 : 0) +
      (link.subject === selectedNodeId || link.object === selectedNodeId ? 10000 : 0) +
      (degree.get(link.subject) || 0) +
      (degree.get(link.object) || 0)
    )
  }

  function buildTooltip(item: unknown): string {
    if (!item || typeof item !== 'object') {
      return ''
    }
    const record = item as Partial<Concept & Proposition>
    if (record.predicate) {
      return `<div class="brain-tooltip"><strong>${escapeHtml(record.predicate)}</strong><span>Proposition</span></div>`
    }
    if (record.name) {
      return `<div class="brain-tooltip"><strong>${escapeHtml(record.name)}</strong><span>${escapeHtml(record.type || '')}</span></div>`
    }
    return ''
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

  function typeColor(type: string): string {
    const fixed: Record<string, string> = {
      $ConceptType: '#7c3aed',
      $PropositionType: '#4f46e5',
      Domain: '#65a30d',
      Person: '#0f766e',
      Project: '#2563eb',
      Preference: '#c2410c',
      Conversation: '#be185d',
      Event: '#ca8a04',
      Insight: '#0891b2'
    }
    if (fixed[type]) {
      return fixed[type]
    }
    return paletteHash(type, typePalette)
  }

  function predicateColor(predicate: string): string {
    return paletteHash(predicate, predicatePalette)
  }

  function paletteHash(value: string, palette: string[]): string {
    let hash = 0
    for (let i = 0; i < value.length; i += 1) {
      hash = (hash * 31 + value.charCodeAt(i)) >>> 0
    }
    return palette[hash % palette.length] || palette[0]!
  }

  function hexToRgba(hex: string, alpha: number): string {
    const r = parseInt(hex.slice(1, 3), 16)
    const g = parseInt(hex.slice(3, 5), 16)
    const b = parseInt(hex.slice(5, 7), 16)
    return `rgba(${r}, ${g}, ${b}, ${alpha})`
  }

  function escapeHtml(value: string): string {
    return value
      .replace(/&/g, '&amp;')
      .replace(/</g, '&lt;')
      .replace(/>/g, '&gt;')
      .replace(/"/g, '&quot;')
  }

  function comboId(type: string): string {
    return `type:${type}`
  }

  const typePalette = [
    '#0f766e',
    '#2563eb',
    '#c2410c',
    '#be185d',
    '#65a30d',
    '#7c3aed',
    '#0891b2',
    '#b45309',
    '#4338ca',
    '#047857',
    '#b91c1c',
    '#6d28d9'
  ]
  const predicatePalette = [
    '#38bdf8',
    '#34d399',
    '#f59e0b',
    '#fb7185',
    '#a78bfa',
    '#22c55e',
    '#f97316',
    '#06b6d4'
  ]
</script>

<svelte:head>
  <title>Anda Brain Graph</title>
</svelte:head>

<div class="brain-shell">
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
        title="Settings"
        onclick={() => (settingsOpen = !settingsOpen)}
      >
        <Settings class="size-4" />
      </button>
      <button class={buttonClass('outline', 'sm')} onclick={loadGraph} disabled={loading}>
        <RefreshCw class={cn('size-4', loading && 'animate-spin')} />
        Refresh
      </button>
    </div>
  </header>

  {#if errorMessage}
    <div
      class="border-b border-destructive/20 bg-destructive/10 px-4 py-2 text-xs text-destructive"
    >
      {errorMessage}
    </div>
  {/if}

  <main class="brain-workspace">
    <aside class="brain-sidebar">
      {#if settingsOpen}
        <section class="brain-panel">
          <div class="brain-panel-heading">
            <Settings class="size-4" />
            Connection
          </div>
          <label class="brain-field">
            <span>Gateway</span>
            <input class={inputClass('h-8 text-xs')} bind:value={settings.baseUrl} />
          </label>
          <label class="brain-field">
            <span>Space</span>
            <input class={inputClass('h-8 text-xs')} bind:value={settings.spaceId} />
          </label>
          <label class="brain-field">
            <span>Token</span>
            <input class={inputClass('h-8 text-xs')} bind:value={settings.token} type="password" />
          </label>
          <label class="brain-field">
            <span>Theme</span>
            <select class={nativeSelectClass('h-8 text-xs')} bind:value={settings.appearanceTheme}>
              <option value="system">System</option>
              <option value="light">Light</option>
              <option value="dark">Dark</option>
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
            Save
          </button>
        </section>
      {/if}

      <section class="brain-panel">
        <div class="brain-panel-heading">
          <Search class="size-4" />
          Search
        </div>
        <div class="flex gap-2">
          <input
            class={inputClass('h-8 text-xs')}
            bind:value={searchQuery}
            placeholder="Concept, type, alias"
            onkeydown={(event) => {
              if (event.key === 'Enter') runSearch()
              if (event.key === 'Escape') {
                searchQuery = ''
                searchResults = []
              }
            }}
          />
          <button class={buttonClass('outline', 'icon-sm')} onclick={runSearch} title="Search">
            <Search class="size-4" />
          </button>
        </div>
        {#if searchResults.length}
          <div class="flex items-center justify-between text-xs text-muted-foreground">
            <span>{searchIndex + 1}/{searchResults.length}</span>
            <div class="flex gap-1">
              <button class={buttonClass('ghost', 'xs')} onclick={() => stepSearch(-1)}>Prev</button
              >
              <button class={buttonClass('ghost', 'xs')} onclick={() => stepSearch(1)}>Next</button>
            </div>
          </div>
        {/if}
      </section>

      <section class="brain-panel">
        <div class="brain-panel-heading">
          <SlidersHorizontal class="size-4" />
          View
        </div>
        <label class="brain-field">
          <span>Mode</span>
          <select class={nativeSelectClass('h-8 text-xs')} bind:value={viewMode}>
            <option value="overview">Overview</option>
            <option value="focus">Focus</option>
            <option value="full">Full</option>
          </select>
        </label>
        <label class="brain-field">
          <span>Edges</span>
          <select class={nativeSelectClass('h-8 text-xs')} bind:value={edgeMode}>
            <option value="focus">Focus</option>
            <option value="all">All</option>
            <option value="none">None</option>
          </select>
        </label>
        <label class="brain-field">
          <span>Labels</span>
          <select class={nativeSelectClass('h-8 text-xs')} bind:value={labelMode}>
            <option value="smart">Smart</option>
            <option value="all">All</option>
            <option value="none">None</option>
          </select>
        </label>
        <label class="brain-field">
          <span>Node cap {maxVisibleNodes}</span>
          <input
            class="w-full accent-primary"
            bind:value={maxVisibleNodes}
            min="80"
            max="1200"
            step="20"
            type="range"
          />
        </label>
        <label class="brain-field">
          <span>Radius {focusRadius}</span>
          <input
            class="w-full accent-primary"
            bind:value={focusRadius}
            min="1"
            max="3"
            step="1"
            type="range"
          />
        </label>
        <button class={buttonClass('ghost', 'sm', 'w-full')} onclick={resetFilters}
          >Reset filters</button
        >
      </section>

      <section class="brain-panel">
        <div class="brain-panel-heading">
          <GitFork class="size-4" />
          Filters
        </div>
        <label class="brain-field">
          <span>Type</span>
          <select class={nativeSelectClass('h-8 text-xs')} bind:value={typeFilter}>
            <option value="">All types</option>
            {#each summary.typeCounts as [type, count]}
              <option value={type}>{type} ({count})</option>
            {/each}
          </select>
        </label>
        <label class="brain-field">
          <span>Predicate</span>
          <select class={nativeSelectClass('h-8 text-xs')} bind:value={predicateFilter}>
            <option value="">All predicates</option>
            {#each summary.predicateCounts as [predicate, count]}
              <option value={predicate}>{predicate} ({count})</option>
            {/each}
          </select>
        </label>
      </section>

      <section class="brain-panel">
        <div class="brain-panel-heading">
          <Database class="size-4" />
          Counts
        </div>
        <dl class="grid grid-cols-2 gap-2 text-xs">
          <div>
            <dt class="text-muted-foreground">Loaded nodes</dt>
            <dd class="font-medium">{summary.nodeCount}</dd>
          </div>
          <div>
            <dt class="text-muted-foreground">Loaded links</dt>
            <dd class="font-medium">{summary.linkCount}</dd>
          </div>
          <div>
            <dt class="text-muted-foreground">Visible nodes</dt>
            <dd class="font-medium">{summary.visibleNodeCount}</dd>
          </div>
          <div>
            <dt class="text-muted-foreground">Visible links</dt>
            <dd class="font-medium">{summary.visibleLinkCount}</dd>
          </div>
        </dl>
        {#if snapshot.status}
          <div class="mt-2 grid grid-cols-2 gap-2 text-xs text-muted-foreground">
            <span>Total concepts {snapshot.status.concepts}</span>
            <span>Total props {snapshot.status.propositions}</span>
          </div>
        {/if}
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
            Loading
          </div>
        </div>
      {/if}

      <div class="brain-floating-toolbar">
        <button
          class={buttonClass('outline', 'icon-sm')}
          onclick={() => zoomBy(1.25)}
          title="Zoom in"
        >
          <ZoomIn class="size-4" />
        </button>
        <button
          class={buttonClass('outline', 'icon-sm')}
          onclick={() => zoomBy(0.8)}
          title="Zoom out"
        >
          <ZoomOut class="size-4" />
        </button>
        <button class={buttonClass('outline', 'icon-sm')} onclick={fitView} title="Fit view">
          <Maximize2 class="size-4" />
        </button>
        <button
          class={buttonClass('outline', 'icon-sm')}
          onclick={() => focusElement()}
          disabled={!selectedNodeId}
          title="Focus selected"
        >
          <LocateFixed class="size-4" />
        </button>
        <button
          class={buttonClass('outline', 'icon-sm')}
          onclick={() => (edgeMode = edgeMode === 'none' ? 'focus' : 'none')}
          title="Toggle edges"
        >
          {#if edgeMode === 'none'}
            <EyeOff class="size-4" />
          {:else}
            <Eye class="size-4" />
          {/if}
        </button>
      </div>

      <div class="brain-mode-tabs" data-slot="button-group">
        <button
          class={buttonClass(viewMode === 'overview' ? 'default' : 'outline', 'xs')}
          onclick={() => (viewMode = 'overview')}
        >
          Overview
        </button>
        <button
          class={buttonClass(viewMode === 'focus' ? 'default' : 'outline', 'xs')}
          onclick={() => (viewMode = 'focus')}
        >
          Focus
        </button>
        <button
          class={buttonClass(viewMode === 'full' ? 'default' : 'outline', 'xs')}
          onclick={() => (viewMode = 'full')}
        >
          Full
        </button>
      </div>
    </section>

    <aside class="brain-inspector">
      <section class="brain-panel">
        <div class="brain-panel-heading justify-between">
          <span class="flex items-center gap-2">
            <FileBracesCorner class="size-4" />
            Inspector
          </span>
          {#if selectedNode || selectedEdge}
            <div class="flex items-center gap-1">
              <button
                class={buttonClass('ghost', 'icon-xs')}
                onclick={() => copyInspectorJson()}
                title={inspectorCopyState === 'copied' ? 'Copied JSON' : 'Copy JSON'}
                aria-label={inspectorCopyState === 'copied' ? 'Copied JSON' : 'Copy JSON'}
              >
                {#if inspectorCopyState === 'copied'}
                  <Check class="size-3" />
                {:else}
                  <Copy class="size-3" />
                {/if}
              </button>
              <button
                class={buttonClass('ghost', 'icon-xs')}
                onclick={() => {
                  selectedNodeId = ''
                  selectedEdgeId = ''
                  inspectorCopyState = 'idle'
                }}
                title="Close"
                aria-label="Close inspector"
              >
                <X class="size-3" />
              </button>
            </div>
          {/if}
        </div>

        {#if selectedNode}
          <div class="space-y-3">
            <div>
              <span class={badgeClass('secondary')}>{selectedNode.type}</span>
              <h2 class="mt-2 break-words text-sm font-semibold">{selectedNode.name}</h2>
              <p class="mt-1 break-all text-xs text-muted-foreground">{selectedNode.id}</p>
            </div>
            <div class="flex gap-2">
              <button
                class={buttonClass('outline', 'xs')}
                onclick={() => expandNode(selectedNode.id)}
                disabled={expandingNodeId === selectedNode.id}
              >
                {#if expandingNodeId === selectedNode.id}
                  <LoaderCircle class="size-3 animate-spin" />
                {/if}
                Expand
              </button>
              <button
                class={buttonClass('ghost', 'xs')}
                onclick={() => {
                  viewMode = 'focus'
                  focusElement(selectedNode.id)
                }}
              >
                Focus
              </button>
            </div>
            <div class={separatorClass()}></div>
            {@render DetailBlock('Attributes', selectedNode.attributes)}
            {@render DetailBlock('Metadata', selectedNode.metadata || {})}
          </div>
        {:else if selectedEdge}
          <div class="space-y-3">
            <div>
              <span class={badgeClass(selectedEdge._virtual ? 'outline' : 'secondary')}>
                {selectedEdge._virtual ? 'Virtual' : 'Proposition'}
              </span>
              <h2 class="mt-2 break-words text-sm font-semibold">{selectedEdge.predicate}</h2>
              <p class="mt-1 break-all text-xs text-muted-foreground">{selectedEdge.id}</p>
            </div>
            <div class="space-y-2 text-xs">
              <button
                class={buttonClass('ghost', 'xs', 'w-full justify-start')}
                onclick={() => selectNode(selectedEdge.subject)}
              >
                Subject {graphData.nodes.get(selectedEdge.subject)?.name || selectedEdge.subject}
              </button>
              <button
                class={buttonClass('ghost', 'xs', 'w-full justify-start')}
                onclick={() => selectNode(selectedEdge.object)}
              >
                Object {graphData.nodes.get(selectedEdge.object)?.name || selectedEdge.object}
              </button>
            </div>
            <div class={separatorClass()}></div>
            {@render DetailBlock('Attributes', selectedEdge.attributes)}
            {@render DetailBlock('Metadata', selectedEdge.metadata || {})}
          </div>
        {:else}
          <div class="space-y-2 text-xs text-muted-foreground">
            {#each summary.hubs.slice(0, 8) as hub}
              <button
                class={buttonClass('ghost', 'xs', 'w-full justify-between')}
                onclick={() => {
                  selectNode(hub.id)
                  viewMode = 'focus'
                }}
              >
                <span class="min-w-0 truncate">{hub.name}</span>
                <span class="shrink-0 text-muted-foreground">{hub.degree}</span>
              </button>
            {/each}
          </div>
        {/if}
      </section>

      <section class="brain-panel">
        <button
          class={buttonClass('ghost', 'sm', 'w-full justify-between')}
          onclick={() => (queryOpen = !queryOpen)}
        >
          <span class="flex items-center gap-2">
            <Braces class="size-4" />
            KIP
          </span>
          <span>{queryOpen ? 'Hide' : 'Show'}</span>
        </button>
        {#if queryOpen}
          <textarea class={textareaClass('mt-3 min-h-36 font-mono text-xs')} bind:value={kipCommand}
          ></textarea>
          <button
            class={buttonClass('outline', 'sm', 'mt-2 w-full')}
            onclick={runKipQuery}
            disabled={loading}
          >
            Run readonly
          </button>
          {#if queryOutput}
            <pre class="brain-json language-json mt-3 max-h-64"><code class="language-json"
                >{@html highlightJsonString(queryOutput)}</code
              ></pre>
          {/if}
        {/if}
      </section>

      <section class="brain-panel">
        <div class="brain-panel-heading">
          <GitFork class="size-4" />
          Top Types
        </div>
        <div class="space-y-1">
          {#each summary.typeCounts.slice(0, 8) as [type, count]}
            <button
              class={buttonClass('ghost', 'xs', 'w-full justify-between')}
              onclick={() => (typeFilter = type)}
            >
              <span class="truncate" style={`color: ${typeColor(type)}`}>{type}</span>
              <span class="text-muted-foreground">{count}</span>
            </button>
          {/each}
        </div>
      </section>
    </aside>
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

<style>
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

  .brain-mode-tabs {
    position: absolute;
    left: 0.75rem;
    top: 0.75rem;
    display: flex;
    gap: 0.35rem;
    border: 1px solid var(--border);
    border-radius: 0.5rem;
    background: color-mix(in oklch, var(--background) 88%, transparent);
    padding: 0.35rem;
    backdrop-filter: blur(12px);
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
    .brain-mode-tabs {
      max-width: calc(100% - 1rem);
      flex-wrap: wrap;
    }

    .brain-floating-toolbar {
      right: 0.5rem;
    }

    .brain-mode-tabs {
      left: 0.5rem;
    }
  }
</style>
