import { CanvasEvent, EdgeEvent, Graph, NodeEvent } from '@antv/g6'
import { RadialLayout } from '@antv/layout'
import { getMessage } from '$lib/i18n'
import { escapeHtml } from '$lib/utils/format'
import type { Concept, Proposition } from './graph.svelte'
import type { BrainGraphView } from './view-model'

/** Which elements the view wants emphasised on the next sync. */
export interface BrainGraphHighlights {
  selectedNodeId: string
  selectedEdgeId: string
  selectedSummaryId: string
  expandingNodeId: string
  searchResults: string[]
  pinnedNodeIds: string[]
}

/** What the canvas reports back to the view. */
export interface BrainGraphHandlers {
  onSelectNode(id: string): void
  onExpandNode(id: string): void
  onSelectEdge(id: string): void
  onCanvasClick(): void
}

const RENDER_WATCHDOG_MS = 8000
const FOCUS_REQUEST_TTL_MS = 2000
const RESIZE_DEBOUNCE_MS = 120

/**
 * Draws the Brain graph on a G6 canvas and keeps it in step with the view model.
 *
 * Three rules shape everything here, all forced by G6 inside an extension page:
 *
 * - **Never await a camera or animation promise.** `focusElement`, `fitView`,
 *   `zoomTo`, and `setElementState` drive `requestAnimationFrame`, which Chrome
 *   throttles or suspends outright for a hidden page — their promises can stay
 *   pending forever. They are called with animation off and their rejections
 *   swallowed. `render()` and `draw()` must be awaited, so they run under a
 *   watchdog instead.
 * - **Re-layout only when the topology changes.** A dataset whose node/combo set
 *   is unchanged is redrawn in place, preserving positions and the viewport;
 *   anything else would make the graph jump on every selection.
 * - **Element state is diffed, not reapplied.** Only ids whose state actually
 *   changed go into the batched `setElementState` call.
 *
 * `sync()` is safe to call concurrently: an overlapping call coalesces into one
 * more pass rather than interleaving.
 */
export class BrainGraphRenderer {
  #handlers: BrainGraphHandlers
  #graph: Graph | null = null
  #container: HTMLElement | null = null
  #resizeObserver: ResizeObserver | null = null
  #resizeTimer: ReturnType<typeof setTimeout> | null = null

  #dataset: BrainGraphView | null = null
  #highlights: BrainGraphHighlights | null = null

  #syncing = false
  #pendingSync = false
  #rendered = false
  #lastTopologyKey = '__initial__'
  #nodeIds = new Set<string>()
  #edgeIds = new Set<string>()
  #appliedStates = new Map<string, string>()
  #pendingFocus: { id: string; at: number } | null = null
  #hoveredLabel: { kind: 'node' | 'edge'; id: string } | null = null

  constructor(handlers: BrainGraphHandlers) {
    this.#handlers = handlers
  }

  /** True once the first render finished; until then `sync()` is a no-op. */
  get rendered(): boolean {
    return this.#rendered
  }

  /**
   * Creates the canvas in `container` and performs the first render. Resolves
   * when the graph is on screen; rejects if that first render fails.
   */
  async mount(
    container: HTMLElement,
    dataset: BrainGraphView,
    highlights: BrainGraphHighlights
  ): Promise<void> {
    if (this.#graph) {
      return
    }
    this.#container = container
    this.#dataset = dataset
    this.#highlights = highlights

    const stage = this.#stageSize()
    this.#graph = new Graph({
      container,
      width: stage.width,
      height: stage.height,
      padding: 24,
      // Animations run on requestAnimationFrame, which is throttled or fully
      // suspended for hidden extension pages; awaiting them can stall the
      // sync loop, and on large graphs they hurt interaction latency.
      animation: false,
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
      layout: { ...layoutOptions(dataset, stage) },
      behaviors: [
        { type: 'drag-canvas', key: 'drag-canvas' },
        { type: 'zoom-canvas', key: 'zoom-canvas', sensitivity: 1.12 },
        { type: 'drag-element', key: 'drag-element' },
        { type: 'hover-activate', key: 'hover-activate' },
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
          getContent: (_event: unknown, items: Array<{ data?: unknown }>) =>
            elementTooltip(items?.[0]?.data),
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

    this.#bindEvents(this.#graph)

    if (import.meta.env.DEV) {
      ;(window as unknown as { __brainGraph?: Graph }).__brainGraph = this.#graph
    }

    this.#resizeObserver = new ResizeObserver(() => this.scheduleResize())
    this.#resizeObserver.observe(container)

    await this.#sync()
    this.#rendered = true
  }

  /** Reconciles the canvas with a new dataset and highlight set. */
  async sync(dataset: BrainGraphView, highlights: BrainGraphHighlights): Promise<void> {
    this.#dataset = dataset
    this.#highlights = highlights
    if (!this.#graph || !this.#rendered) {
      return
    }
    await this.#sync()
  }

  /**
   * Applies highlights without redrawing, for instant selection feedback ahead
   * of the debounced dataset sync.
   */
  applyHighlights(highlights: BrainGraphHighlights): void {
    this.#highlights = highlights
    if (this.#graph && this.#rendered) {
      this.#applyElementStates()
    }
  }

  /** Asks the next sync to centre `id`, if it is still on screen by then. */
  requestFocus(id: string): void {
    this.#pendingFocus = { id, at: Date.now() }
  }

  focus(id: string): void {
    this.#graph?.focusElement(id, false).catch(() => undefined)
  }

  fitView(): void {
    this.#graph?.fitView(undefined, false).catch(() => undefined)
  }

  zoomBy(scale: number): void {
    const graph = this.#graph
    if (!graph) {
      return
    }
    graph.zoomTo(Math.max(0.08, Math.min(graph.getZoom() * scale, 5)), false)
  }

  /** Refits the canvas to its container. Debounced; never re-runs the layout. */
  scheduleResize(): void {
    if (this.#resizeTimer) {
      clearTimeout(this.#resizeTimer)
    }
    this.#resizeTimer = setTimeout(() => {
      this.#resizeTimer = null
      const stage = this.#stageSize()
      this.#graph?.resize(stage.width, stage.height)
      if (this.#rendered && this.#nodeIds.size > 0) {
        this.fitView()
      }
    }, RESIZE_DEBOUNCE_MS)
  }

  destroy(): void {
    if (this.#resizeTimer) {
      clearTimeout(this.#resizeTimer)
      this.#resizeTimer = null
    }
    this.#resizeObserver?.disconnect()
    this.#resizeObserver = null
    this.#graph?.destroy()
    this.#graph = null
    this.#container = null
    this.#rendered = false
  }

  #bindEvents(graph: Graph): void {
    // G6 types the payload as IEvent; only the target id is used here.
    const on = (eventName: string, handle: (id: string) => void): void => {
      graph.on(eventName, (event) => {
        const id = (event as { target?: { id?: string } }).target?.id
        if (id) {
          handle(id)
        }
      })
    }

    on(NodeEvent.CLICK, (id) => this.#handlers.onSelectNode(id))
    on(NodeEvent.DBLCLICK, (id) => this.#handlers.onExpandNode(id))
    on(NodeEvent.POINTER_ENTER, (id) => this.#showHoverLabel('node', id))
    on(NodeEvent.POINTER_LEAVE, (id) => this.#hideHoverLabel('node', id))
    on(EdgeEvent.CLICK, (id) => this.#handlers.onSelectEdge(id))
    on(EdgeEvent.POINTER_ENTER, (id) => this.#showHoverLabel('edge', id))
    on(EdgeEvent.POINTER_LEAVE, (id) => this.#hideHoverLabel('edge', id))
    graph.on(CanvasEvent.CLICK, () => this.#handlers.onCanvasClick())
  }

  async #sync(): Promise<void> {
    const graph = this.#graph
    if (!graph) {
      return
    }
    if (this.#syncing) {
      this.#pendingSync = true
      return
    }
    this.#syncing = true
    try {
      do {
        this.#pendingSync = false
        const dataset = this.#dataset
        if (!dataset) {
          return
        }
        const topologyKey = topologyKeyOf(dataset)
        const topologyChanged = topologyKey !== this.#lastTopologyKey
        this.#hoveredLabel = null
        graph.setData(dataset)
        if (topologyChanged) {
          this.#lastTopologyKey = topologyKey
          graph.setOptions({ layout: layoutOptions(dataset, this.#stageSize()) as never })
          await withRenderWatchdog(graph.render())
        } else {
          // Same node topology: keep layout positions and the viewport,
          // only redraw changed elements (edges, labels, styles).
          await withRenderWatchdog(graph.draw())
        }

        this.#nodeIds = new Set(dataset.nodes.map((node) => String(node.id)))
        this.#edgeIds = new Set(dataset.edges.map((edge) => String(edge.id)))
        for (const id of this.#appliedStates.keys()) {
          if (!this.#nodeIds.has(id) && !this.#edgeIds.has(id)) {
            this.#appliedStates.delete(id)
          }
        }
        this.#applyElementStates()

        const focus = this.#pendingFocus
        this.#pendingFocus = null
        if (dataset.nodes.length === 0) {
          continue
        }
        if (focus && Date.now() - focus.at < FOCUS_REQUEST_TTL_MS && this.#nodeIds.has(focus.id)) {
          this.focus(focus.id)
        } else if (topologyChanged) {
          this.fitView()
        }
      } while (this.#pendingSync)
    } finally {
      this.#syncing = false
    }
  }

  #applyElementStates(): void {
    const graph = this.#graph
    const highlights = this.#highlights
    if (!graph || !highlights) {
      return
    }
    const searchSet = new Set(highlights.searchResults)
    const batch: Record<string, string[]> = {}
    let changed = 0

    const apply = (id: string, states: string[]) => {
      const key = states.join(' ')
      if ((this.#appliedStates.get(id) || '') === key) {
        return
      }
      if (key) {
        this.#appliedStates.set(id, key)
      } else {
        this.#appliedStates.delete(id)
      }
      batch[id] = states
      changed += 1
    }

    for (const id of this.#nodeIds) {
      const states: string[] = []
      if (id === highlights.selectedNodeId || id === highlights.selectedSummaryId) {
        states.push('selected')
      } else if (searchSet.has(id)) {
        states.push('highlight')
      } else if (searchSet.size > 0) {
        states.push('dim')
      }
      if (highlights.pinnedNodeIds.includes(id) && id !== highlights.selectedNodeId) {
        states.push('highlight')
      }
      if (id === highlights.expandingNodeId) {
        states.push('loading')
      }
      apply(id, states)
    }
    for (const id of this.#edgeIds) {
      apply(id, id === highlights.selectedEdgeId ? ['selected'] : [])
    }

    if (changed > 0) {
      // One batched state update; tolerate elements that vanished mid-flight.
      graph.setElementState(batch, false).catch(() => undefined)
    }
  }

  /** Labels are hidden by default and revealed only under the pointer. */
  #showHoverLabel(kind: 'node' | 'edge', id: string): void {
    if (!this.#graph) {
      return
    }
    if (this.#hoveredLabel && (this.#hoveredLabel.kind !== kind || this.#hoveredLabel.id !== id)) {
      this.#setElementLabel(this.#hoveredLabel.kind, this.#hoveredLabel.id, '')
    }
    const label = kind === 'node' ? this.#nodeLabel(id) : this.#edgeLabel(id)
    if (!label) {
      return
    }
    this.#hoveredLabel = { kind, id }
    this.#setElementLabel(kind, id, label)
  }

  #hideHoverLabel(kind: 'node' | 'edge', id: string): void {
    if (!this.#hoveredLabel || this.#hoveredLabel.kind !== kind || this.#hoveredLabel.id !== id) {
      return
    }
    this.#hoveredLabel = null
    this.#setElementLabel(kind, id, '')
  }

  #setElementLabel(kind: 'node' | 'edge', id: string, label: string): void {
    const graph = this.#graph
    if (!graph) {
      return
    }
    if (kind === 'node') {
      graph.updateNodeData([{ id, style: { labelText: label } } as never])
    } else {
      graph.updateEdgeData([{ id, style: { labelText: label } } as never])
    }
    graph.draw().catch(() => undefined)
  }

  #nodeLabel(id: string): string {
    const node = this.#dataset?.nodes.find((item) => String(item.id) === id)
    return (node?.data as Partial<Concept> | undefined)?.name || ''
  }

  #edgeLabel(id: string): string {
    const edge = this.#dataset?.edges.find((item) => String(item.id) === id)
    return (edge?.data as Partial<Proposition> | undefined)?.predicate || ''
  }

  #stageSize(): { width: number; height: number } {
    const rect = this.#container?.getBoundingClientRect()
    return {
      width: Math.max(320, Math.floor(rect?.width || window.innerWidth || 1024)),
      height: Math.max(320, Math.floor(rect?.height || window.innerHeight || 720))
    }
  }
}

/**
 * The node and combo set, order-independent. Two datasets sharing a key can be
 * redrawn in place; anything else needs a fresh layout.
 */
function topologyKeyOf(dataset: BrainGraphView): string {
  return dataset.nodes
    .map((node) => `${node.id}|${node.combo || ''}`)
    .sort()
    .join(',')
}

/** Sizes the radial layout to the canvas and how many combos share it. */
function layoutOptions(dataset: BrainGraphView, stage: { width: number; height: number }) {
  const comboScale = Math.sqrt(Math.max(1, dataset.combos.length))
  const innerWidth = Math.max(360, Math.min(stage.width * 0.86, (stage.width * 1.18) / comboScale))
  const innerHeight = Math.max(
    320,
    Math.min(stage.height * 0.86, (stage.height * 1.18) / comboScale)
  )

  return {
    type: 'combo-combined',
    // Iterative layouts with animation enabled return promises that never
    // settle in this @antv/layout version; the non-animated path computes
    // final positions synchronously and lets graph.render() resolve.
    animation: false,
    comboPadding: Math.max(20, Math.min(52, Math.min(stage.width, stage.height) / 18)),
    spacing: Math.max(28, Math.min(84, Math.min(stage.width, stage.height) / 14)),
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

/**
 * Bounds an awaited G6 render. Element animations can be left dangling when a
 * behavior interrupts them, which would otherwise deadlock the sync loop.
 */
function withRenderWatchdog(work: Promise<void>): Promise<void> {
  return Promise.race([
    work.catch(() => undefined),
    new Promise<void>((resolve) => setTimeout(resolve, RENDER_WATCHDOG_MS))
  ])
}

function elementTooltip(item: unknown): string {
  if (!item || typeof item !== 'object') {
    return ''
  }
  const record = item as Partial<Concept & Proposition>
  if (record.metadata && (record.metadata as Record<string, unknown>).summary) {
    return tooltipHtml(record.name || '', getMessage('brainSummaryNode'))
  }
  if (record.predicate) {
    return tooltipHtml(record.predicate, getMessage('brainProposition'))
  }
  if (record.name) {
    return tooltipHtml(record.name, record.type || '')
  }
  return ''
}

function tooltipHtml(title: string, subtitle: string): string {
  return `<div class="brain-tooltip"><strong>${escapeHtml(title)}</strong><span>${escapeHtml(subtitle)}</span></div>`
}
