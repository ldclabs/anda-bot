import {
  isSplit,
  type LayoutNode,
  type PaneId,
  type PanelEdge,
  type PaneSplit
} from '../shared/panels'

/**
 * Where the canvas tree puts each pane: rectangles in fractions of the area
 * beside the sidebar, x in reading order. The shell lays them out as one CSS
 * grid, so moving a pane never moves its element and nothing remounts.
 */
export interface Rect {
  x0: number
  x1: number
  y0: number
  y1: number
}
/** The edge between children `index` and `index + 1` of the split at `path`. */
export interface Divider {
  key: string
  path: number[]
  index: number
  axis: PaneSplit['axis']
  /** The split's rectangle. */
  rect: Rect
  /** The edge's offset along the split's axis. */
  at: number
}
export interface Geometry {
  panes: Map<PaneId, Rect>
  dividers: Divider[]
}

const FULL: Rect = { x0: 0, x1: 1, y0: 0, y1: 1 }
// One rounding for every computed edge, so neighbours share exact grid lines.
const round = (value: number) => Math.round(value * 1e6) / 1e6

export function measure(node: LayoutNode): Geometry {
  const geometry: Geometry = { panes: new Map(), dividers: [] }
  const walk = (current: LayoutNode, rect: Rect, path: number[]) => {
    if (!isSplit(current)) {
      geometry.panes.set(current.pane, rect)
      return
    }
    const row = current.axis === 'row'
    const from = row ? rect.x0 : rect.y0
    const to = row ? rect.x1 : rect.y1
    let offset = from
    current.children.forEach((child, index) => {
      const last = index === current.children.length - 1
      const end = last ? to : round(offset + (to - from) * current.sizes[index]!)
      walk(child, row ? { ...rect, x0: offset, x1: end } : { ...rect, y0: offset, y1: end }, [
        ...path,
        index
      ])
      if (!last)
        geometry.dividers.push({
          key: `${path.join('.')}:${index}`,
          path,
          index,
          axis: current.axis,
          rect,
          at: end
        })
      offset = end
    })
  }
  walk(node, FULL, [])
  return geometry
}

/**
 * The shell grid for a geometry: tracks after the sidebar's column, and a
 * `grid-area` for each pane and divider. Track sizes are flexible, so the
 * canvas keeps its proportions as the window resizes.
 */
export function gridOf(geometry: Geometry) {
  const lines = (pick: (rect: Rect) => number[]) =>
    [...new Set([...geometry.panes.values()].flatMap(pick))].sort((a, b) => a - b)
  const xs = lines((rect) => [rect.x0, rect.x1])
  const ys = lines((rect) => [rect.y0, rect.y1])
  const tracks = (edges: number[]) =>
    edges
      .slice(1)
      .map((edge, i) => `minmax(0, ${((edge - edges[i]!) * 1000).toFixed(3)}fr)`)
      .join(' ')
  // Column 1 is the sidebar's.
  const column = (x: number) => xs.indexOf(x) + 2
  const row = (y: number) => ys.indexOf(y) + 1
  return {
    columns: tracks(xs),
    rows: tracks(ys),
    pane(id: PaneId): string | undefined {
      const rect = geometry.panes.get(id)
      return rect && `${row(rect.y0)} / ${column(rect.x0)} / ${row(rect.y1)} / ${column(rect.x1)}`
    },
    divider({ axis, rect, at }: Divider): string {
      return axis === 'row'
        ? `${row(rect.y0)} / ${column(at)} / ${row(rect.y1)} / ${column(at) + 1}`
        : `${row(at)} / ${column(rect.x0)} / ${row(at) + 1} / ${column(rect.x1)}`
    }
  }
}

/** The edge of the canvas a pane runs along its whole length, if any. */
export function edgeOf(rect: Rect | undefined): PanelEdge | null {
  if (!rect) return null
  if (rect.y0 === 0 && rect.y1 === 1 && rect.x0 !== rect.x1) {
    if (rect.x1 === 1 && rect.x0 > 0) return 'end'
    if (rect.x0 === 0 && rect.x1 < 1) return 'start'
  }
  if (rect.x0 === 0 && rect.x1 === 1) {
    if (rect.y0 === 0 && rect.y1 < 1) return 'top'
    if (rect.y1 === 1 && rect.y0 > 0) return 'bottom'
  }
  return null
}

/**
 * The panes under the window's own controls: macOS draws its buttons over the
 * top-left corner, Windows its caption buttons over the top-right. `sidebar`
 * is the side the sidebar takes, when shown; in a right-to-left interface the
 * canvas's start is on the right.
 */
export function cornerPanes(
  panes: Map<PaneId, Rect>,
  sidebar: 'left' | 'right' | null,
  rtl: boolean
): { left: PaneId | null; right: PaneId | null } {
  const at = (atStart: boolean) =>
    [...panes].find(
      ([, rect]) => rect.y0 === 0 && (atStart ? rect.x0 === 0 : rect.x1 === 1)
    )?.[0] ?? null
  return {
    left: sidebar === 'left' ? null : at(!rtl),
    right: sidebar === 'right' ? null : at(rtl)
  }
}

/** Panes never shrink below these when an edge between them is dragged. */
export const PANE_MIN = { chat: { width: 340, height: 260 }, panel: { width: 220, height: 140 } }
export function minExtent(node: LayoutNode, axis: PaneSplit['axis']): number {
  if (!isSplit(node)) {
    const min = node.pane === 'chat' ? PANE_MIN.chat : PANE_MIN.panel
    return axis === 'row' ? min.width : min.height
  }
  const extents = node.children.map((child) => minExtent(child, axis))
  return node.axis === axis
    ? extents.reduce((sum, extent) => sum + extent, 0)
    : Math.max(...extents)
}

/**
 * The canvas at `width` × `height` pixels with every pane at its minimum size
 * or more where the room allows: a split gives children below their minimum
 * room from the others, in proportion to what those have to spare. The saved
 * layout keeps its shape; this is how it is shown.
 */
export function fitWorkbench(node: LayoutNode, width: number, height: number): LayoutNode {
  if (!isSplit(node) || width <= 0 || height <= 0) return node
  const row = node.axis === 'row'
  const length = row ? width : height
  const mins = node.children.map((child) => minExtent(child, node.axis) / length)
  let sizes = node.sizes
  const deficit = sizes.reduce((sum, size, i) => sum + Math.max(0, mins[i]! - size), 0)
  if (deficit > 0 && mins.reduce((sum, min) => sum + min, 0) < 1) {
    const spare = sizes.map((size, i) => Math.max(0, size - mins[i]!))
    const room = spare.reduce((sum, value) => sum + value, 0)
    sizes = sizes.map((size, i) =>
      size < mins[i]! ? mins[i]! : size - (spare[i]! / room) * deficit
    )
  }
  return {
    ...node,
    sizes,
    children: node.children.map((child, i) =>
      fitWorkbench(child, row ? width * sizes[i]! : width, row ? height : height * sizes[i]!)
    )
  }
}

/**
 * Where a dragged panel lands on a pane: along the nearest edge when the
 * pointer is in its outer third, else in its place (`center`, a swap). `x`
 * and `y` are fractions of the pane's box on screen.
 */
export function dropZone(x: number, y: number, rtl: boolean): PanelEdge | 'center' {
  const distances: [PanelEdge, number][] = [
    [rtl ? 'end' : 'start', x],
    [rtl ? 'start' : 'end', 1 - x],
    ['top', y],
    ['bottom', 1 - y]
  ]
  const [edge, distance] = distances.reduce((near, next) => (next[1] < near[1] ? next : near))
  return distance < 1 / 3 ? edge : 'center'
}
/** The part of a pane's box a drop zone fills, to preview the drop. */
export function zoneBox(
  box: { left: number; top: number; width: number; height: number },
  zone: PanelEdge | 'center',
  rtl: boolean
) {
  const left = zone === (rtl ? 'end' : 'start')
  const right = zone === (rtl ? 'start' : 'end')
  return {
    left: right ? box.left + box.width / 2 : box.left,
    top: zone === 'bottom' ? box.top + box.height / 2 : box.top,
    width: left || right ? box.width / 2 : box.width,
    height: zone === 'top' || zone === 'bottom' ? box.height / 2 : box.height
  }
}
