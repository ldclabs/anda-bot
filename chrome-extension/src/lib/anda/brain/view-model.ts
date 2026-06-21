import type { ComboData, EdgeData, NodeData } from '@antv/g6'
import type {
  BrainGraphData,
  Concept,
  GraphSnapshot,
  GraphSummary,
  Proposition
} from './graph.svelte'

export type BrainGraphScene = 'atlas' | 'focus' | 'path'
export type RelatedMemoryGroupKind =
  | 'atlas-type'
  | 'hub'
  | 'recent'
  | 'predicate'
  | 'path'
  | 'summary'

export interface BrainGraphUiState {
  anchorNodeId: string
  selectedEdgeId: string
  searchResultIds: string[]
  expandedNodeIds: Set<string>
  pinnedNodeIds: Set<string>
  pathRequested: boolean
  breadcrumb: Array<{ id: string; label: string }>
}

export interface RelatedMemoryItem {
  id: string
  label: string
  detail: string
  nodeId?: string
  edgeId?: string
  type?: string
  predicate?: string
}

export interface RelatedMemoryGroup {
  id: string
  title: string
  kind: RelatedMemoryGroupKind
  total: number
  hidden: number
  items: RelatedMemoryItem[]
}

export interface BrainSummaryNode {
  id: string
  label: string
  groupId: string
  count: number
  kind: RelatedMemoryGroupKind
}

export interface BrainGraphViewSummary extends GraphSummary {
  scene: BrainGraphScene
  hiddenNodeCount: number
  hiddenEdgeCount: number
  budget: GraphBudget
  atlasClusters: Array<{ type: string; count: number; color: string }>
  recentNodes: Array<{ id: string; name: string; type: string }>
}

export interface BrainGraphView {
  scene: BrainGraphScene
  nodes: NodeData[]
  edges: EdgeData[]
  combos: ComboData[]
  relatedGroups: RelatedMemoryGroup[]
  summaryNodes: BrainSummaryNode[]
  summary: BrainGraphViewSummary
}

interface GraphBudget {
  nodes: number
  edges: number
  labels: number
}

interface BuildContext {
  graphData: BrainGraphData
  snapshot: GraphSnapshot
  nodesById: Map<string, Concept>
  degree: Map<string, number>
  ui: BrainGraphUiState
  searchSet: Set<string>
  pinnedSet: Set<string>
}

interface NodeStyleOptions {
  degree: number
  selected: boolean
  searched: boolean
  pinned: boolean
  anchor: boolean
  summary?: boolean
  cluster?: boolean
  combo?: string
}

interface EdgeStyleOptions {
  selected: boolean
  focused: boolean
  path: boolean
}

export const GRAPH_BUDGETS: Record<'atlas' | 'focus' | 'focusExpanded' | 'path', GraphBudget> = {
  atlas: { nodes: 120, edges: 240, labels: 80 },
  focus: { nodes: 180, edges: 500, labels: 80 },
  focusExpanded: { nodes: 320, edges: 900, labels: 110 },
  path: { nodes: 80, edges: 200, labels: 80 }
}

const SUMMARY_PREFIX = 'brain-summary:'
const MAX_RELATED_ITEMS = 8
const MAX_SUMMARY_GROUPS = 4
const INTERNAL_TYPES = new Set(['$ConceptType', '$PropositionType'])

export function buildBrainGraphView(
  graphData: BrainGraphData,
  snapshot: GraphSnapshot,
  ui: BrainGraphUiState
): BrainGraphView {
  const nodesById = new Map(snapshot.nodes.map((node) => [node.id, node]))
  const context: BuildContext = {
    graphData,
    snapshot,
    nodesById,
    degree: graphData.degreeByNode(),
    ui,
    searchSet: new Set(ui.searchResultIds),
    pinnedSet: new Set(ui.pinnedNodeIds)
  }
  const scene = resolveScene(context)
  if (scene === 'path') {
    return buildPathView(context)
  }
  if (scene === 'focus') {
    return buildFocusView(context)
  }
  return buildAtlasView(context)
}

export function isBrainSummaryNodeId(id: string): boolean {
  return id.startsWith(SUMMARY_PREFIX)
}

export function typeColor(type: string): string {
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
  return fixed[type] || paletteHash(type, typePalette)
}

export function predicateColor(predicate: string): string {
  return paletteHash(predicate, predicatePalette)
}

function resolveScene(context: BuildContext): BrainGraphScene {
  if (context.ui.pathRequested && context.pinnedSet.size >= 2) {
    return 'path'
  }
  if (context.ui.anchorNodeId && context.nodesById.has(context.ui.anchorNodeId)) {
    return 'focus'
  }
  return 'atlas'
}

function buildAtlasView(context: BuildContext): BrainGraphView {
  const budget = GRAPH_BUDGETS.atlas
  const typeCounts = topTypeCounts(context.snapshot.nodes, 14)
  const hubs = topHubs(context, 36)
  const recent = recentNodes(context.snapshot.nodes, 24)
  const clusterNodes = typeCounts
    .slice(0, 18)
    .map(([type, count]) => makeClusterConcept(type, count))

  const candidateNodes = uniqueConcepts([
    ...clusterNodes,
    ...hubs.map((hub) => context.nodesById.get(hub.id)).filter(isDefined),
    ...recent
  ])
  const visibleConcepts = candidateNodes.slice(0, budget.nodes)
  const visibleIds = new Set(visibleConcepts.map((node) => node.id))
  const clusterIds = new Set(clusterNodes.map((node) => node.id))
  const labelIds = new Set(visibleConcepts.slice(0, budget.labels).map((node) => node.id))

  const realVisibleLinks = context.snapshot.links
    .filter(
      (link) =>
        !link._virtual &&
        visibleIds.has(link.subject) &&
        visibleIds.has(link.object) &&
        !clusterIds.has(link.subject) &&
        !clusterIds.has(link.object)
    )
    .sort((left, right) => linkPriority(right, context, true) - linkPriority(left, context, true))
    .slice(0, budget.edges)
  const atlasLinks = makeAtlasClusterLinks(visibleConcepts, clusterIds, budget.edges)
  const visibleLinks = [...realVisibleLinks, ...atlasLinks]
    .sort((left, right) => linkPriority(right, context, true) - linkPriority(left, context, true))
    .slice(0, budget.edges)

  const groups: RelatedMemoryGroup[] = [
    ...typeCounts.slice(0, 8).map(([type, count]) =>
      makeNodeGroup(
        `atlas-type:${type}`,
        type,
        'atlas-type',
        context.snapshot.nodes.filter((node) => node.type === type),
        count
      )
    ),
    makeNodeGroup(
      'atlas-hubs',
      'High-connection memories',
      'hub',
      hubs.map((hub) => context.nodesById.get(hub.id)).filter(isDefined),
      hubs.length
    ),
    makeNodeGroup('atlas-recent', 'Recently loaded', 'recent', recent, recent.length)
  ].filter((group) => group.total > 0)

  return finalizeView({
    context,
    scene: 'atlas',
    budget,
    visibleConcepts,
    visibleLinks,
    relatedGroups: groups,
    summaryNodes: [],
    labelIds,
    clusterIds,
    pathEdgeIds: new Set(),
    hiddenNodeCount: Math.max(0, candidateNodes.length - visibleConcepts.length),
    hiddenEdgeCount: Math.max(0, context.snapshot.links.length - realVisibleLinks.length)
  })
}

function buildFocusView(context: BuildContext): BrainGraphView {
  const anchorId = context.ui.anchorNodeId
  const expanded = context.ui.expandedNodeIds.has(anchorId)
  const budget = expanded ? GRAPH_BUDGETS.focusExpanded : GRAPH_BUDGETS.focus
  const anchor = context.nodesById.get(anchorId)
  if (!anchor) {
    return buildAtlasView(context)
  }

  const neighborLinks = context.graphData
    .getNeighborLinks(anchorId)
    .filter((link) => context.nodesById.has(link.subject) && context.nodesById.has(link.object))
    .sort((left, right) => linkPriority(right, context, false) - linkPriority(left, context, false))
  const neighborIds = new Set<string>([anchorId])
  for (const link of neighborLinks) {
    neighborIds.add(link.subject)
    neighborIds.add(link.object)
  }
  for (const id of context.searchSet) {
    if (context.nodesById.has(id)) {
      neighborIds.add(id)
    }
  }
  for (const id of context.pinnedSet) {
    if (context.nodesById.has(id)) {
      neighborIds.add(id)
    }
  }

  const rankedIds = Array.from(neighborIds).sort(
    (left, right) => nodePriority(right, context, anchorId) - nodePriority(left, context, anchorId)
  )
  const pinnedOrSearchOutsideFocus = rankedIds.filter(
    (id) => context.pinnedSet.has(id) || context.searchSet.has(id) || id === anchorId
  )
  const visibleRealLimit = budget.nodes - 1
  const visibleIds = new Set<string>(rankedIds.slice(0, visibleRealLimit))
  for (const id of pinnedOrSearchOutsideFocus) {
    visibleIds.add(id)
  }
  const cappedIds = capIds(visibleIds, visibleRealLimit, context, anchorId)
  const hiddenIds = rankedIds.filter((id) => !cappedIds.has(id))
  const summaryNodes = makeSummaryNodes(context, hiddenIds, anchorId, budget.nodes - cappedIds.size)
  const summaryConcepts = summaryNodes.map(summaryNodeToConcept)
  const visibleConcepts = [
    ...Array.from(cappedIds)
      .map((id) => context.nodesById.get(id))
      .filter(isDefined),
    ...summaryConcepts
  ]
  const visibleIdSet = new Set(visibleConcepts.map((node) => node.id))
  const realVisibleIds = new Set(Array.from(cappedIds))
  const summaryLinks = summaryNodes.map((summaryNode) => makeSummaryLink(anchorId, summaryNode))
  const visibleLinks = [
    ...neighborLinks
      .filter((link) => realVisibleIds.has(link.subject) && realVisibleIds.has(link.object))
      .slice(0, budget.edges - summaryLinks.length),
    ...summaryLinks
  ].slice(0, budget.edges)
  const relatedGroups = buildFocusRelatedGroups(context, anchorId, neighborLinks, hiddenIds)
  const labelIds = importantLabelIds(context, visibleIdSet, anchorId, budget.labels)

  return finalizeView({
    context,
    scene: 'focus',
    budget,
    visibleConcepts,
    visibleLinks,
    relatedGroups,
    summaryNodes,
    labelIds,
    clusterIds: new Set(),
    pathEdgeIds: new Set(),
    hiddenNodeCount: hiddenIds.length,
    hiddenEdgeCount: Math.max(0, neighborLinks.length - visibleLinks.length)
  })
}

function buildPathView(context: BuildContext): BrainGraphView {
  const budget = GRAPH_BUDGETS.path
  const pinned = Array.from(context.pinnedSet).filter((id) => context.nodesById.has(id))
  if (pinned.length < 2) {
    return buildFocusView(context)
  }

  const pathNodeIds = new Set<string>(pinned)
  const pathEdgeIds = new Set<string>()
  for (let index = 0; index < pinned.length - 1; index += 1) {
    const path = shortestPath(context, pinned[index]!, pinned[index + 1]!)
    for (const id of path.nodeIds) {
      pathNodeIds.add(id)
    }
    for (const id of path.edgeIds) {
      pathEdgeIds.add(id)
    }
  }

  for (const neighbor of commonNeighbors(context, pinned)) {
    pathNodeIds.add(neighbor)
    if (pathNodeIds.size >= budget.nodes) {
      break
    }
  }

  const visibleIds = capIds(pathNodeIds, budget.nodes, context, context.ui.anchorNodeId)
  const visibleConcepts = Array.from(visibleIds)
    .map((id) => context.nodesById.get(id))
    .filter(isDefined)
  const visibleIdSet = new Set(visibleConcepts.map((node) => node.id))
  const visibleLinks = context.snapshot.links
    .filter((link) => visibleIdSet.has(link.subject) && visibleIdSet.has(link.object))
    .sort(
      (left, right) =>
        (pathEdgeIds.has(right.id) ? 100000 : 0) - (pathEdgeIds.has(left.id) ? 100000 : 0) ||
        linkPriority(right, context, false) - linkPriority(left, context, false)
    )
    .slice(0, budget.edges)
  const relatedGroups = buildPathRelatedGroups(context, pinned, visibleLinks)
  const labelIds = importantLabelIds(context, visibleIdSet, context.ui.anchorNodeId, budget.labels)

  return finalizeView({
    context,
    scene: 'path',
    budget,
    visibleConcepts,
    visibleLinks,
    relatedGroups,
    summaryNodes: [],
    labelIds,
    clusterIds: new Set(),
    pathEdgeIds,
    hiddenNodeCount: Math.max(0, pathNodeIds.size - visibleIds.size),
    hiddenEdgeCount: Math.max(0, context.snapshot.links.length - visibleLinks.length)
  })
}

function finalizeView(input: {
  context: BuildContext
  scene: BrainGraphScene
  budget: GraphBudget
  visibleConcepts: Concept[]
  visibleLinks: Proposition[]
  relatedGroups: RelatedMemoryGroup[]
  summaryNodes: BrainSummaryNode[]
  labelIds: Set<string>
  clusterIds: Set<string>
  pathEdgeIds: Set<string>
  hiddenNodeCount: number
  hiddenEdgeCount: number
}): BrainGraphView {
  const {
    context,
    scene,
    budget,
    visibleConcepts,
    visibleLinks,
    relatedGroups,
    summaryNodes,
    labelIds,
    clusterIds,
    pathEdgeIds,
    hiddenNodeCount,
    hiddenEdgeCount
  } = input
  const typeCounts = new Map<string, number>()
  for (const node of visibleConcepts) {
    if (!isBrainSummaryNodeId(node.id) && !clusterIds.has(node.id)) {
      typeCounts.set(node.type, (typeCounts.get(node.type) || 0) + 1)
    }
  }
  const combos = Array.from(typeCounts.entries())
    .filter(([, count]) => count >= 2)
    .slice(0, 28)
    .map(([type, count]) => makeCombo(type, count))
  const comboIds = new Set(combos.map((combo) => combo.id))

  const nodes = visibleConcepts.map((node) =>
    makeNode(node, {
      degree: context.degree.get(node.id) || 0,
      selected: node.id === context.ui.anchorNodeId,
      searched: context.searchSet.has(node.id),
      pinned: context.pinnedSet.has(node.id),
      anchor: node.id === context.ui.anchorNodeId,
      summary: isBrainSummaryNodeId(node.id),
      cluster: clusterIds.has(node.id),
      combo:
        !clusterIds.has(node.id) && comboIds.has(comboId(node.type))
          ? comboId(node.type)
          : undefined
    })
  )
  const edges = visibleLinks.map((link) =>
    makeEdge(link, {
      selected: link.id === context.ui.selectedEdgeId,
      focused:
        Boolean(context.ui.anchorNodeId) &&
        (link.subject === context.ui.anchorNodeId || link.object === context.ui.anchorNodeId),
      path: pathEdgeIds.has(link.id)
    })
  )
  const graphSummary = context.graphData.summary(nodes.length, edges.length)
  const typeCountValues = topTypeCounts(context.snapshot.nodes, 12)
  const recent = recentNodes(context.snapshot.nodes, 8)

  return {
    scene,
    nodes,
    edges,
    combos,
    relatedGroups,
    summaryNodes,
    summary: {
      ...graphSummary,
      scene,
      hiddenNodeCount,
      hiddenEdgeCount,
      budget,
      typeCounts: typeCountValues,
      atlasClusters: typeCountValues.map(([type, count]) => ({
        type,
        count,
        color: typeColor(type)
      })),
      recentNodes: recent.map((node) => ({ id: node.id, name: node.name, type: node.type }))
    }
  }
}

function makeNode(node: Concept, options: NodeStyleOptions): NodeData {
  const typeDef = isTypeDefinition(node)
  const color = options.summary ? '#94a3b8' : typeColor(node.type)
  const size = options.summary
    ? [74, 42]
    : options.cluster
      ? [84, 52]
      : typeDef
        ? [66, 42]
        : Math.max(18, Math.min(44, 18 + Math.sqrt(options.degree) * 6))
  return {
    id: node.id,
    type: options.summary || options.cluster || typeDef ? 'ellipse' : 'circle',
    combo: options.combo,
    data: node,
    style: {
      size,
      fill: options.summary ? mixHex(color, '#ffffff', 0.72) : mixHex(color, '#ffffff', 0.56),
      stroke: options.summary ? '#64748b' : color,
      lineWidth: options.selected ? 3.4 : options.pinned ? 3 : options.searched ? 2.8 : 1.8,
      lineDash: options.summary ? [4, 4] : undefined,
      cursor: node._isExpanding ? 'wait' : 'pointer',
      labelText: '',
      labelFill: 'rgba(245, 245, 244, 0.9)',
      labelBackgroundFill: 'rgba(28, 25, 23, 0.78)',
      haloStroke: hexToRgba(color, 0.16)
    }
  } as unknown as NodeData
}

function makeEdge(link: Proposition, options: EdgeStyleOptions): EdgeData {
  const summary = isBrainSummaryNodeId(link.object) || isBrainSummaryNodeId(link.subject)
  const atlas = link.metadata?.atlas === true
  const color = summary
    ? '#94a3b8'
    : atlas
      ? typeColor(String(link.attributes?.type || ''))
      : link._virtual
        ? '#a78bfa'
        : predicateColor(link.predicate)
  return {
    id: link.id,
    source: link.subject,
    target: link.object,
    data: link,
    style: {
      stroke: color,
      strokeOpacity: summary
        ? 0.38
        : atlas
          ? 0.58
          : link._virtual
            ? 0.25
            : options.focused || options.path
              ? 0.84
              : 0.48,
      lineWidth: options.selected
        ? 2.4
        : options.path
          ? 1.9
          : options.focused || atlas
            ? 1.35
            : 0.9,
      lineDash: summary || link._virtual ? [4, 4] : undefined,
      endArrow: !link._virtual && !atlas,
      labelText: '',
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
      labelText: '',
      fill: hexToRgba(color, 0.06),
      stroke: hexToRgba(color, 0.52),
      labelFill: hexToRgba(color, 0.92),
      cursor: 'default'
    }
  } as ComboData
}

function makeClusterConcept(type: string, count: number): Concept {
  return {
    id: `brain-cluster:${type}`,
    type,
    name: `${type} ${count}`,
    attributes: { count, cluster: type },
    metadata: {}
  }
}

function makeAtlasClusterLinks(
  visibleConcepts: Concept[],
  clusterIds: Set<string>,
  limit: number
): Proposition[] {
  const links: Proposition[] = []
  const seenByType = new Map<string, number>()
  for (const node of visibleConcepts) {
    if (clusterIds.has(node.id) || isBrainSummaryNodeId(node.id)) {
      continue
    }
    const clusterId = `brain-cluster:${node.type}`
    if (!clusterIds.has(clusterId)) {
      continue
    }
    const seen = seenByType.get(node.type) || 0
    if (seen >= 12) {
      continue
    }
    seenByType.set(node.type, seen + 1)
    links.push({
      _type: 'VirtualLink',
      _virtual: true,
      id: `atlas:${node.type}:${node.id}`,
      subject: clusterId,
      object: node.id,
      predicate: 'atlas_member',
      attributes: { type: node.type },
      metadata: { atlas: true }
    })
    if (links.length >= limit) {
      break
    }
  }
  return links
}

function makeSummaryNodes(
  context: BuildContext,
  hiddenIds: string[],
  anchorId: string,
  availableSlots: number
): BrainSummaryNode[] {
  if (hiddenIds.length === 0 || availableSlots <= 0) {
    return []
  }
  const byType = new Map<string, string[]>()
  for (const id of hiddenIds) {
    const node = context.nodesById.get(id)
    if (!node) {
      continue
    }
    const list = byType.get(node.type) || []
    list.push(id)
    byType.set(node.type, list)
  }
  return Array.from(byType.entries())
    .sort((left, right) => right[1].length - left[1].length || left[0].localeCompare(right[0]))
    .slice(0, Math.min(MAX_SUMMARY_GROUPS, availableSlots))
    .map(([type, ids]) => ({
      id: `${SUMMARY_PREFIX}${anchorId}:${type}`,
      label: `+${ids.length} ${type}`,
      groupId: `summary:${anchorId}:${type}`,
      count: ids.length,
      kind: 'summary'
    }))
}

function summaryNodeToConcept(summary: BrainSummaryNode): Concept {
  return {
    id: summary.id,
    type: 'Related',
    name: summary.label,
    attributes: {
      count: summary.count,
      group_id: summary.groupId
    },
    metadata: {
      summary: true
    }
  }
}

function makeSummaryLink(anchorId: string, summary: BrainSummaryNode): Proposition {
  return {
    _type: 'VirtualLink',
    _virtual: true,
    id: `${summary.id}:edge`,
    subject: anchorId,
    object: summary.id,
    predicate: 'related',
    attributes: { count: summary.count },
    metadata: { summary: true }
  }
}

function buildFocusRelatedGroups(
  context: BuildContext,
  anchorId: string,
  links: Proposition[],
  hiddenIds: string[]
): RelatedMemoryGroup[] {
  const byPredicate = new Map<string, Proposition[]>()
  for (const link of links) {
    const list = byPredicate.get(link.predicate) || []
    list.push(link)
    byPredicate.set(link.predicate, list)
  }
  const groups = Array.from(byPredicate.entries())
    .sort((left, right) => right[1].length - left[1].length || left[0].localeCompare(right[0]))
    .slice(0, 6)
    .map(([predicate, items]) => makeLinkGroup(context, `predicate:${predicate}`, predicate, items))

  const hiddenByType = new Map<string, Concept[]>()
  for (const id of hiddenIds) {
    const node = context.nodesById.get(id)
    if (!node) {
      continue
    }
    const list = hiddenByType.get(node.type) || []
    list.push(node)
    hiddenByType.set(node.type, list)
  }
  for (const [type, nodes] of hiddenByType.entries()) {
    groups.push(
      makeNodeGroup(`summary:${anchorId}:${type}`, `More ${type}`, 'summary', nodes, nodes.length)
    )
  }
  return groups.filter((group) => group.total > 0)
}

function buildPathRelatedGroups(
  context: BuildContext,
  pinned: string[],
  links: Proposition[]
): RelatedMemoryGroup[] {
  const pinnedNodes = pinned.map((id) => context.nodesById.get(id)).filter(isDefined)
  const predicates = new Map<string, Proposition[]>()
  for (const link of links) {
    const list = predicates.get(link.predicate) || []
    list.push(link)
    predicates.set(link.predicate, list)
  }
  return [
    makeNodeGroup('path:pinned', 'Pinned memories', 'path', pinnedNodes, pinnedNodes.length),
    ...Array.from(predicates.entries())
      .sort((left, right) => right[1].length - left[1].length || left[0].localeCompare(right[0]))
      .slice(0, 4)
      .map(([predicate, items]) => makeLinkGroup(context, `path:${predicate}`, predicate, items))
  ].filter((group) => group.total > 0)
}

function makeNodeGroup(
  id: string,
  title: string,
  kind: RelatedMemoryGroupKind,
  nodes: Concept[],
  total = nodes.length
): RelatedMemoryGroup {
  const items = nodes.slice(0, MAX_RELATED_ITEMS).map((node) => ({
    id: node.id,
    label: node.name,
    detail: node.type,
    nodeId: node.id,
    type: node.type
  }))
  return {
    id,
    title,
    kind,
    total,
    hidden: Math.max(0, total - items.length),
    items
  }
}

function makeLinkGroup(
  context: BuildContext,
  id: string,
  title: string,
  links: Proposition[]
): RelatedMemoryGroup {
  const items = links.slice(0, MAX_RELATED_ITEMS).map((link) => {
    const subject = context.nodesById.get(link.subject)
    const object = context.nodesById.get(link.object)
    const other =
      link.subject === context.ui.anchorNodeId
        ? object
        : link.object === context.ui.anchorNodeId
          ? subject
          : object || subject
    return {
      id: link.id,
      label: other?.name || link.predicate,
      detail: link.predicate,
      nodeId: other?.id,
      edgeId: link.id,
      type: other?.type,
      predicate: link.predicate
    }
  })
  return {
    id,
    title,
    kind: 'predicate',
    total: links.length,
    hidden: Math.max(0, links.length - items.length),
    items
  }
}

function shortestPath(
  context: BuildContext,
  start: string,
  end: string
): { nodeIds: string[]; edgeIds: string[] } {
  if (start === end) {
    return { nodeIds: [start], edgeIds: [] }
  }
  const queue = [start]
  const previous = new Map<string, { nodeId: string; edgeId: string }>()
  const visited = new Set<string>([start])
  while (queue.length > 0 && visited.size < 1200) {
    const current = queue.shift()!
    for (const link of context.graphData.getNeighborLinks(current)) {
      const next = link.subject === current ? link.object : link.subject
      if (!context.nodesById.has(next) || visited.has(next)) {
        continue
      }
      visited.add(next)
      previous.set(next, { nodeId: current, edgeId: link.id })
      if (next === end) {
        const nodeIds = [end]
        const edgeIds: string[] = []
        let cursor = end
        while (cursor !== start) {
          const step = previous.get(cursor)
          if (!step) {
            break
          }
          edgeIds.push(step.edgeId)
          nodeIds.push(step.nodeId)
          cursor = step.nodeId
        }
        return { nodeIds: nodeIds.reverse(), edgeIds: edgeIds.reverse() }
      }
      queue.push(next)
    }
  }
  return { nodeIds: [start, end], edgeIds: [] }
}

function commonNeighbors(context: BuildContext, ids: string[]): string[] {
  const counts = new Map<string, number>()
  for (const id of ids) {
    const neighbors = new Set<string>()
    for (const link of context.graphData.getNeighborLinks(id)) {
      const other = link.subject === id ? link.object : link.subject
      if (context.nodesById.has(other) && !context.pinnedSet.has(other)) {
        neighbors.add(other)
      }
    }
    for (const neighbor of neighbors) {
      counts.set(neighbor, (counts.get(neighbor) || 0) + 1)
    }
  }
  return Array.from(counts.entries())
    .filter(([, count]) => count >= 2)
    .sort(
      (left, right) =>
        right[1] - left[1] ||
        (context.degree.get(right[0]) || 0) - (context.degree.get(left[0]) || 0) ||
        left[0].localeCompare(right[0])
    )
    .map(([id]) => id)
}

function capIds(
  ids: Set<string>,
  limit: number,
  context: BuildContext,
  anchorId: string
): Set<string> {
  if (ids.size <= limit) {
    return ids
  }
  return new Set(
    Array.from(ids)
      .sort(
        (left, right) =>
          nodePriority(right, context, anchorId) - nodePriority(left, context, anchorId)
      )
      .slice(0, limit)
  )
}

function importantLabelIds(
  context: BuildContext,
  visibleIds: Set<string>,
  anchorId: string,
  limit: number
): Set<string> {
  const ids = Array.from(visibleIds).sort(
    (left, right) => nodePriority(right, context, anchorId) - nodePriority(left, context, anchorId)
  )
  return new Set(ids.slice(0, limit))
}

function nodePriority(id: string, context: BuildContext, anchorId: string): number {
  const node = context.nodesById.get(id)
  const typePenalty = node && INTERNAL_TYPES.has(node.type) && id !== anchorId ? -2500 : 0
  return (
    (id === anchorId ? 1_000_000 : 0) +
    (context.pinnedSet.has(id) ? 800_000 : 0) +
    (context.searchSet.has(id) ? 650_000 : 0) +
    (context.ui.expandedNodeIds.has(id) ? 120_000 : 0) +
    Math.min(context.degree.get(id) || 0, 24) * 1000 +
    typePenalty
  )
}

function linkPriority(link: Proposition, context: BuildContext, atlas: boolean): number {
  const anchorId = context.ui.anchorNodeId
  return (
    (link.subject === anchorId || link.object === anchorId ? 1_000_000 : 0) +
    (context.pinnedSet.has(link.subject) && context.pinnedSet.has(link.object) ? 600_000 : 0) +
    (context.searchSet.has(link.subject) || context.searchSet.has(link.object) ? 420_000 : 0) +
    (link._virtual ? -100_000 : 0) +
    (atlas
      ? Math.min(
          (context.degree.get(link.subject) || 0) + (context.degree.get(link.object) || 0),
          60
        )
      : 0) +
    predicateWeight(link.predicate)
  )
}

function predicateWeight(predicate: string): number {
  if (predicate === 'instance_of') {
    return -5000
  }
  let score = 0
  for (let index = 0; index < predicate.length; index += 1) {
    score = (score + predicate.charCodeAt(index)) % 997
  }
  return score
}

function topTypeCounts(nodes: Concept[], limit: number): Array<[string, number]> {
  const counts = new Map<string, number>()
  for (const node of nodes) {
    if (isBrainSummaryNodeId(node.id)) {
      continue
    }
    counts.set(node.type, (counts.get(node.type) || 0) + 1)
  }
  return Array.from(counts.entries())
    .sort((left, right) => right[1] - left[1] || left[0].localeCompare(right[0]))
    .slice(0, limit)
}

function topHubs(
  context: BuildContext,
  limit: number
): Array<{ id: string; name: string; type: string; degree: number }> {
  return context.snapshot.nodes
    .map((node) => ({
      id: node.id,
      name: node.name,
      type: node.type,
      degree: context.degree.get(node.id) || 0
    }))
    .filter((node) => node.degree > 0 && !INTERNAL_TYPES.has(node.type))
    .sort((left, right) => right.degree - left.degree || left.name.localeCompare(right.name))
    .slice(0, limit)
}

function recentNodes(nodes: Concept[], limit: number): Concept[] {
  return nodes
    .slice()
    .sort(
      (left, right) =>
        memoryTimestamp(right) - memoryTimestamp(left) || right.id.localeCompare(left.id)
    )
    .slice(0, limit)
}

function memoryTimestamp(node: Concept): number {
  const candidates = [
    node.metadata?.updated_at,
    node.metadata?.updatedAt,
    node.metadata?.created_at,
    node.metadata?.createdAt,
    node.attributes?.updated_at,
    node.attributes?.created_at
  ]
  for (const value of candidates) {
    if (typeof value === 'number') {
      return value
    }
    if (typeof value === 'string') {
      const parsed = Date.parse(value)
      if (Number.isFinite(parsed)) {
        return parsed
      }
    }
  }
  return 0
}

function uniqueConcepts(nodes: Concept[]): Concept[] {
  const seen = new Set<string>()
  const result: Concept[] = []
  for (const node of nodes) {
    if (seen.has(node.id)) {
      continue
    }
    seen.add(node.id)
    result.push(node)
  }
  return result
}

function isTypeDefinition(node: Concept): boolean {
  return node.type === '$ConceptType' || node.type === '$PropositionType'
}

function comboId(type: string): string {
  return `type:${type}`
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

function mixHex(hex: string, target: string, amount: number): string {
  const source = parseHex(hex)
  const destination = parseHex(target)
  const mix = (left: number, right: number) => Math.round(left + (right - left) * amount)
  return `#${toHex(mix(source.r, destination.r))}${toHex(mix(source.g, destination.g))}${toHex(
    mix(source.b, destination.b)
  )}`
}

function parseHex(hex: string): { r: number; g: number; b: number } {
  return {
    r: parseInt(hex.slice(1, 3), 16),
    g: parseInt(hex.slice(3, 5), 16),
    b: parseInt(hex.slice(5, 7), 16)
  }
}

function toHex(value: number): string {
  return value.toString(16).padStart(2, '0')
}

function isDefined<T>(value: T | null | undefined): value is T {
  return value !== null && value !== undefined
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
