/**
 * The workbench canvas: the chat and the open panels tile the space beside
 * the sidebar as a tree of splits, and any panel can open in a window of its
 * own instead. Edges are in reading order: `start` is the left in a
 * left-to-right interface and the right in a right-to-left one.
 */
export type PanelTab = 'resources' | 'changes' | 'terminal' | 'browser'
export type PaneId = 'chat' | PanelTab
export type PanelEdge = 'start' | 'end' | 'top' | 'bottom'
export const PANEL_TABS: readonly PanelTab[] = ['resources', 'changes', 'terminal', 'browser']
export const PANEL_EDGES: readonly PanelEdge[] = ['start', 'end', 'top', 'bottom']

export interface PaneLeaf {
  pane: PaneId
}
/** Children side by side (`row`) or stacked (`column`), with sizes that sum to 1. */
export interface PaneSplit {
  axis: 'row' | 'column'
  children: LayoutNode[]
  sizes: number[]
}
export type LayoutNode = PaneLeaf | PaneSplit

export const defaultWorkbench: LayoutNode = { pane: 'chat' }

export function isPanelTab(value: unknown): value is PanelTab {
  return PANEL_TABS.includes(value as PanelTab)
}
export function isPanelEdge(value: unknown): value is PanelEdge {
  return PANEL_EDGES.includes(value as PanelEdge)
}
export function isSplit(node: LayoutNode): node is PaneSplit {
  return 'axis' in node
}

function record(value: unknown): Record<string, unknown> | null {
  return value && typeof value === 'object' && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : null
}
/**
 * Exactly a canvas: the chat once, each panel at most once, splits of two to
 * eight children with positive sizes summing to 1, at most eight deep.
 */
export function validWorkbench(value: unknown): value is LayoutNode {
  const seen = new Set<string>()
  const valid = (node: unknown, depth: number): boolean => {
    const item = record(node)
    if (!item || depth > 8) return false
    const keys = Object.keys(item)
    if ('pane' in item) {
      if (keys.length !== 1 || !(item.pane === 'chat' || isPanelTab(item.pane))) return false
      if (seen.has(item.pane as string)) return false
      seen.add(item.pane as string)
      return true
    }
    const { axis, children, sizes } = item
    return (
      keys.length === 3 &&
      (axis === 'row' || axis === 'column') &&
      Array.isArray(children) &&
      Array.isArray(sizes) &&
      children.length >= 2 &&
      children.length <= 8 &&
      sizes.length === children.length &&
      sizes.every((size) => typeof size === 'number' && size > 0 && Number.isFinite(size)) &&
      Math.abs(sizes.reduce((sum: number, size: number) => sum + size, 0) - 1) < 0.01 &&
      children.every((child) => valid(child, depth + 1))
    )
  }
  return valid(value, 0) && seen.has('chat')
}
export function validDetached(value: unknown): value is PanelTab[] {
  return Array.isArray(value) && value.every(isPanelTab) && new Set(value).size === value.length
}

export function panes(node: LayoutNode): PaneId[] {
  return isSplit(node) ? node.children.flatMap(panes) : [node.pane]
}
export function hasPane(node: LayoutNode, id: PaneId): boolean {
  return panes(node).includes(id)
}
/** Merges a split into its parent when they share an axis, and a split of one into its child. */
function normalize(split: PaneSplit): LayoutNode {
  const children: LayoutNode[] = []
  const sizes: number[] = []
  split.children.forEach((child, i) => {
    if (isSplit(child) && child.axis === split.axis)
      child.children.forEach((grandchild, j) => {
        children.push(grandchild)
        sizes.push(split.sizes[i]! * child.sizes[j]!)
      })
    else {
      children.push(child)
      sizes.push(split.sizes[i]!)
    }
  })
  if (children.length === 1) return children[0]!
  const total = sizes.reduce((sum, size) => sum + size, 0)
  // Rounded, so a saved layout reads 0.58 rather than 0.5800000000000001.
  return {
    axis: split.axis,
    children,
    sizes: sizes.map((size) => Math.round((size / total) * 1e6) / 1e6)
  }
}
export function removePane(node: LayoutNode, id: PaneId): LayoutNode {
  if (!isSplit(node)) return node
  const children: LayoutNode[] = []
  const sizes: number[] = []
  node.children.forEach((child, i) => {
    if (!isSplit(child) && child.pane === id) return
    children.push(removePane(child, id))
    sizes.push(node.sizes[i]!)
  })
  return normalize({ axis: node.axis, children, sizes })
}
const axisOf = (edge: PanelEdge) => (edge === 'start' || edge === 'end' ? 'row' : 'column')
const leadsWith = (edge: PanelEdge) => edge === 'start' || edge === 'top'
/** Puts `id` on `side` of `target`, in half of the target's room. */
export function splitPane(
  node: LayoutNode,
  target: PaneId,
  id: PaneId,
  side: PanelEdge
): LayoutNode {
  const base = removePane(node, id)
  if (target === id || !hasPane(base, target)) return node
  const axis = axisOf(side)
  const before = leadsWith(side)
  const place = (current: LayoutNode): LayoutNode => {
    if (!isSplit(current)) {
      if (current.pane !== target) return current
      const pair = before ? [{ pane: id }, current] : [current, { pane: id }]
      return { axis, children: pair, sizes: [0.5, 0.5] }
    }
    const index = current.children.findIndex((child) => !isSplit(child) && child.pane === target)
    if (index >= 0 && current.axis === axis) {
      const children = [...current.children]
      const sizes = [...current.sizes]
      const half = sizes[index]! / 2
      sizes[index] = half
      const at = before ? index : index + 1
      children.splice(at, 0, { pane: id })
      sizes.splice(at, 0, half)
      return { axis, children, sizes }
    }
    return normalize({ ...current, children: current.children.map(place) })
  }
  return place(base)
}
/** Puts `id` along a whole edge of the canvas. */
export function edgePane(node: LayoutNode, id: PaneId, edge: PanelEdge, share = 0.35): LayoutNode {
  const base = removePane(node, id)
  const pane = { pane: id }
  return normalize(
    leadsWith(edge)
      ? { axis: axisOf(edge), children: [pane, base], sizes: [share, 1 - share] }
      : { axis: axisOf(edge), children: [base, pane], sizes: [1 - share, share] }
  )
}
/**
 * Where an opened panel goes: beside the chat at the end, then stacked under
 * the last panel, taking an even share of that column.
 */
export function addPane(node: LayoutNode, id: PanelTab): LayoutNode {
  if (hasPane(node, id)) return node
  const last = panes(node)
    .filter((pane) => pane !== 'chat')
    .at(-1)
  if (!last) return edgePane(node, id, 'end', 0.42)
  const stack = (current: LayoutNode): LayoutNode => {
    if (!isSplit(current)) return current
    const index = current.children.findIndex((child) => !isSplit(child) && child.pane === last)
    if (index < 0 || current.axis !== 'column')
      return { ...current, children: current.children.map(stack) }
    const share = 1 / (current.children.length + 1)
    const children = [...current.children]
    children.splice(index + 1, 0, { pane: id })
    const sizes = current.sizes.map((size) => size * (1 - share))
    sizes.splice(index + 1, 0, share)
    return normalize({ axis: 'column', children, sizes })
  }
  const stacked = stack(node)
  return hasPane(stacked, id) ? stacked : splitPane(node, last, id, 'bottom')
}
export function swapPanes(node: LayoutNode, a: PaneId, b: PaneId): LayoutNode {
  if (!isSplit(node)) return { pane: node.pane === a ? b : node.pane === b ? a : node.pane }
  return { ...node, children: node.children.map((child) => swapPanes(child, a, b)) }
}
/** The split at `path` (child indices from the root). */
export function splitAt(node: LayoutNode, path: readonly number[]): PaneSplit | undefined {
  let current: LayoutNode | undefined = node
  for (const index of path)
    current = current && isSplit(current) ? current.children[index] : undefined
  return current && isSplit(current) ? current : undefined
}
export function resizeSplit(
  node: LayoutNode,
  path: readonly number[],
  sizes: number[]
): LayoutNode {
  if (!isSplit(node)) return node
  if (!path.length) return { ...node, sizes }
  const [index, ...rest] = path
  return {
    ...node,
    children: node.children.map((child, i) =>
      i === index ? resizeSplit(child, rest, sizes) : child
    )
  }
}
/** A stored canvas, without the panels that open in their own windows. */
export function readWorkbench(value: unknown, detached: readonly PanelTab[]): LayoutNode {
  const tree = validWorkbench(value) ? value : defaultWorkbench
  return detached.reduce(removePane, tree)
}

/** A transcript link to a workspace file, shown in Resources. */
export interface FileRequest {
  id: number
  path: string
  line?: number
}
/**
 * What a detached panel shows of the chat open in the main window. Main only
 * relays it, so the attachments' type is the renderer's.
 */
export interface PanelContext<Attachment = unknown> {
  source: string
  workspace?: string
  branch: string
  resources: Attachment[]
  sessionFiles: string[]
  fileRequest: FileRequest | null
  changeFocus: { id: number; path: string } | null
}
export type PanelRequest =
  /** Main window: open the panel's window, or bring it forward; `at` is a screen point to open at. */
  | { action: 'open'; tab: PanelTab; focus: boolean; at?: { x: number; y: number } }
  | { action: 'close'; tab: PanelTab }
  /** A panel window: close and go back into the canvas, along `edge` if given. */
  | { action: 'dock'; tab: PanelTab; edge?: PanelEdge }
  /** Main window: the chat the panel windows follow. */
  | { action: 'context'; context: PanelContext }
  /** A panel window: what it shows at startup. */
  | { action: 'state' }
export interface PanelWindowState {
  tab: PanelTab
  context: PanelContext | null
  preferences: import('./contract').Preferences
  platform: string
  fullScreen: boolean
}
/** Main window: a panel window opened or closed, or asked to go back into the canvas. */
export interface PanelWindowEvent {
  tab: PanelTab
  open: boolean
  dock?: PanelEdge | 'canvas'
}
