import { describe, expect, it, vi } from 'vitest'
import type { BrainApi } from './api'
import { BrainGraphData } from './graph.svelte'
import { GRAPH_BUDGETS, buildBrainGraphView, type BrainGraphUiState } from './view-model'

function createGraph(): BrainGraphData {
  return new BrainGraphData({
    status: vi.fn(),
    executeKipReadonly: vi.fn()
  } as unknown as BrainApi)
}

function ui(overrides: Partial<BrainGraphUiState> = {}): BrainGraphUiState {
  return {
    anchorNodeId: '',
    selectedEdgeId: '',
    searchResultIds: [],
    expandedNodeIds: new Set(),
    pinnedNodeIds: new Set(),
    pathRequested: false,
    breadcrumb: [],
    ...overrides
  }
}

function addNode(graph: BrainGraphData, id: string, type = 'Memory'): void {
  graph.addConcept({
    id,
    type,
    name: `${type} ${id}`,
    attributes: {},
    metadata: {
      created_at: `2026-06-${String((Number(id.replace(/\D/g, '')) % 20) + 1).padStart(2, '0')}`
    }
  })
}

function addLink(
  graph: BrainGraphData,
  id: string,
  subject: string,
  object: string,
  predicate = 'related_to',
  virtual = false
): void {
  graph.addProposition({
    _type: virtual ? 'VirtualLink' : 'PropositionLink',
    _virtual: virtual,
    id,
    subject,
    object,
    predicate,
    attributes: {},
    metadata: {}
  })
}

describe('buildBrainGraphView', () => {
  it('builds a bounded atlas with cluster groups', () => {
    const graph = createGraph()
    for (let index = 0; index < 220; index += 1) {
      addNode(graph, `node-${index}`, index % 2 === 0 ? 'Project' : 'Insight')
    }

    const view = buildBrainGraphView(graph, graph.snapshot(), ui())

    expect(view.scene).toBe('atlas')
    expect(view.nodes.length).toBeLessThanOrEqual(GRAPH_BUDGETS.atlas.nodes)
    expect(view.edges.length).toBeLessThanOrEqual(GRAPH_BUDGETS.atlas.edges)
    expect(view.edges.length).toBeGreaterThan(0)
    expect(view.nodes.some((node) => String(node.id).startsWith('brain-cluster:'))).toBe(true)
    expect(view.relatedGroups.some((group) => group.kind === 'atlas-type')).toBe(true)
  })

  it('keeps anchor, search results, pinned nodes, and summary nodes in focus budget', () => {
    const graph = createGraph()
    addNode(graph, 'anchor', 'Project')
    for (let index = 0; index < 260; index += 1) {
      addNode(graph, `neighbor-${index}`, index % 3 === 0 ? 'Person' : 'Insight')
      addLink(graph, `link-${index}`, 'anchor', `neighbor-${index}`, 'mentions')
    }

    const view = buildBrainGraphView(
      graph,
      graph.snapshot(),
      ui({
        anchorNodeId: 'anchor',
        searchResultIds: ['neighbor-259'],
        pinnedNodeIds: new Set(['neighbor-258'])
      })
    )
    const nodeIds = new Set(view.nodes.map((node) => String(node.id)))

    expect(view.scene).toBe('focus')
    expect(view.nodes.length).toBeLessThanOrEqual(GRAPH_BUDGETS.focus.nodes)
    expect(view.edges.length).toBeLessThanOrEqual(GRAPH_BUDGETS.focus.edges)
    expect(nodeIds.has('anchor')).toBe(true)
    expect(nodeIds.has('neighbor-259')).toBe(true)
    expect(nodeIds.has('neighbor-258')).toBe(true)
    expect(view.summaryNodes.length).toBeGreaterThan(0)
    expect(view.relatedGroups.some((group) => group.kind === 'summary')).toBe(true)
  })

  it('uses the expanded focus budget only after the anchor is expanded locally', () => {
    const graph = createGraph()
    addNode(graph, 'anchor', 'Project')
    for (let index = 0; index < 280; index += 1) {
      addNode(graph, `neighbor-${index}`, 'Insight')
      addLink(graph, `link-${index}`, 'anchor', `neighbor-${index}`, 'relates_to')
    }

    const view = buildBrainGraphView(
      graph,
      graph.snapshot(),
      ui({
        anchorNodeId: 'anchor',
        expandedNodeIds: new Set(['anchor'])
      })
    )

    expect(view.nodes.length).toBeGreaterThan(GRAPH_BUDGETS.focus.nodes)
    expect(view.nodes.length).toBeLessThanOrEqual(GRAPH_BUDGETS.focusExpanded.nodes)
    expect(view.edges.length).toBeLessThanOrEqual(GRAPH_BUDGETS.focusExpanded.edges)
  })

  it('limits path view to pinned-memory explanation data', () => {
    const graph = createGraph()
    for (const id of ['a', 'bridge', 'b']) {
      addNode(graph, id, 'Project')
    }
    addLink(graph, 'a-bridge', 'a', 'bridge', 'depends_on')
    addLink(graph, 'bridge-b', 'bridge', 'b', 'depends_on')
    for (let index = 0; index < 140; index += 1) {
      addNode(graph, `common-${index}`, 'Insight')
      addLink(graph, `a-common-${index}`, 'a', `common-${index}`, 'mentions')
      addLink(graph, `b-common-${index}`, 'b', `common-${index}`, 'mentions')
    }

    const view = buildBrainGraphView(
      graph,
      graph.snapshot(),
      ui({
        pinnedNodeIds: new Set(['a', 'b']),
        pathRequested: true
      })
    )
    const nodeIds = new Set(view.nodes.map((node) => String(node.id)))

    expect(view.scene).toBe('path')
    expect(view.nodes.length).toBeLessThanOrEqual(GRAPH_BUDGETS.path.nodes)
    expect(view.edges.length).toBeLessThanOrEqual(GRAPH_BUDGETS.path.edges)
    expect(nodeIds.has('a')).toBe(true)
    expect(nodeIds.has('b')).toBe(true)
    expect(nodeIds.has('bridge')).toBe(true)
  })

  it('deprioritizes virtual edges before applying the focus edge budget', () => {
    const graph = createGraph()
    addNode(graph, 'anchor', 'Project')
    for (let index = 0; index < 620; index += 1) {
      addNode(graph, `real-${index}`, 'Insight')
      addLink(graph, `real-link-${index}`, 'anchor', `real-${index}`, 'mentions')
    }
    for (let index = 0; index < 20; index += 1) {
      addNode(graph, `virtual-${index}`, 'Insight')
      addLink(graph, `virtual-link-${index}`, 'anchor', `virtual-${index}`, 'instance_of', true)
    }

    const view = buildBrainGraphView(
      graph,
      graph.snapshot(),
      ui({
        anchorNodeId: 'anchor',
        expandedNodeIds: new Set(['anchor'])
      })
    )

    expect(view.edges.length).toBeLessThanOrEqual(GRAPH_BUDGETS.focusExpanded.edges)
    expect(view.edges.some((edge) => String(edge.id).startsWith('virtual-link-'))).toBe(false)
  })
})
