import type { EntityClaim } from './api'

const BELIEF_STATUSES = ['accepted', 'contested', 'uncertain', 'rejected', 'insufficient']

/** Whether a claim has not been replaced, rejected or retired. A claim the
 * policy merely does not count (imported memory) still describes the entity. */
export function isCurrent(claim: EntityClaim): boolean {
  return (
    claim.record.state === 'active' &&
    claim.belief?.excluded_reason !== 'outside_valid_time' &&
    claim.belief?.status !== 'rejected'
  )
}

/** The message key describing a claim's belief, or null when there is none. */
export function beliefKey(claim: EntityClaim): string | null {
  const belief = claim.belief
  if (!belief) return null
  if (belief.excluded_reason === 'outside_valid_time') return 'memoryBelief_ended'
  if (belief.excluded_reason) return 'memoryBelief_excluded'
  return BELIEF_STATUSES.includes(belief.status) ? `memoryBelief_${belief.status}` : null
}

export interface ClaimGroup {
  key: string
  direction: EntityClaim['direction']
  predicate: string
  current: EntityClaim[]
  earlier: EntityClaim[]
}

/** Claims by relation, groups with a current claim first, then newest first. */
export function groupClaims(claims: EntityClaim[]): ClaimGroup[] {
  const groups = new Map<string, ClaimGroup>()
  for (const claim of claims) {
    const key = `${claim.direction}:${claim.record.predicate_label}`
    let group = groups.get(key)
    if (!group) {
      group = {
        key,
        direction: claim.direction,
        predicate: claim.record.predicate_label,
        current: [],
        earlier: []
      }
      groups.set(key, group)
    }
    ;(isCurrent(claim) ? group.current : group.earlier).push(claim)
  }
  const ordered = [...groups.values()]
  return [
    ...ordered.filter((group) => group.current.length),
    ...ordered.filter((group) => !group.current.length)
  ]
}

export const GRAPH_WIDTH = 160
/** Column centres: relations into the entity on the left, out of it on the right. */
const COLUMN = { incoming: 22, outgoing: 138 }
/** Lines run between the centre label and the column labels. */
const LINE = { from: 20, to: 34 }

export interface GraphNode {
  id: string
  label: string
  x: number
  y: number
  /** Index of the relation the node is drawn under. */
  group: number
  current: boolean
}

export interface GraphEdge {
  id: string
  x1: number
  y1: number
  x2: number
  y2: number
  /** Arrowhead polygon, mid-line, pointing at the relation's object. */
  arrow: string
  group: number
  current: boolean
}

export interface GraphRelation {
  direction: EntityClaim['direction']
  predicate: string
  group: number
  nodes: number
}

export interface EntityGraphLayout {
  nodes: GraphNode[]
  edges: GraphEdge[]
  relations: GraphRelation[]
  /** Related entities on this page that were not drawn. */
  hidden: number
  /** Drawing height in the units of `GRAPH_WIDTH`. */
  height: number
}

/**
 * A fixed two-column layout of the entities the given claims connect to: the
 * entity in the centre, subjects of relations into it on the left, objects of
 * relations out of it on the right, one row per entity, `row` units apart,
 * and at most `maxPerColumn` rows a side. Relations stay contiguous, largest
 * first.
 * Literal values and the entity itself are not drawn.
 */
export function layoutEntityGraph(
  entityId: string,
  claims: EntityClaim[],
  maxPerColumn = 12,
  row = 8
): EntityGraphLayout {
  const relations = new Map<
    string,
    Pick<GraphRelation, 'direction' | 'predicate'> & {
      members: Map<string, { label: string; current: boolean }>
    }
  >()
  const assigned = new Map<string, string>()
  for (const claim of claims) {
    const id = claim.other.id
    if (!id || id === entityId) continue
    const key = `${claim.direction}:${claim.record.predicate_label}`
    let relation = relations.get(key)
    if (!relation) {
      relation = {
        direction: claim.direction,
        predicate: claim.record.predicate_label,
        members: new Map()
      }
      relations.set(key, relation)
    }
    const member = relation.members.get(id)
    if (member) member.current ||= isCurrent(claim)
    else relation.members.set(id, { label: claim.other.label, current: isCurrent(claim) })
  }
  const ordered = [...relations.entries()].sort(
    ([leftKey, left], [rightKey, right]) =>
      right.members.size - left.members.size || leftKey.localeCompare(rightKey)
  )
  // An entity joined by several relations is drawn once, under the largest.
  for (const [key, relation] of ordered) {
    for (const id of relation.members.keys()) if (!assigned.has(id)) assigned.set(id, key)
  }
  const columns = {
    incoming: [] as Omit<GraphNode, 'x' | 'y'>[],
    outgoing: [] as Omit<GraphNode, 'x' | 'y'>[]
  }
  const shown: GraphRelation[] = []
  for (const [key, relation] of ordered) {
    const column = columns[relation.direction]
    const group = shown.length
    let nodes = 0
    for (const [id, member] of relation.members) {
      if (assigned.get(id) !== key || column.length === maxPerColumn) continue
      column.push({ id, ...member, group })
      nodes++
    }
    if (nodes)
      shown.push({ direction: relation.direction, predicate: relation.predicate, group, nodes })
  }
  const rows = Math.max(columns.incoming.length, columns.outgoing.length)
  const height = Math.max(6 * row, (rows + 2) * row)
  const center = { x: GRAPH_WIDTH / 2, y: height / 2 }
  const nodes: GraphNode[] = []
  const edges: GraphEdge[] = []
  for (const direction of ['incoming', 'outgoing'] as const) {
    const column = columns[direction]
    column.forEach((node, index) => {
      const x = COLUMN[direction]
      const y = round(center.y + (index - (column.length - 1) / 2) * row)
      nodes.push({ ...node, x, y })
      const span = Math.abs(x - center.x)
      const at = (distance: number) => point(center.x, center.y, x, y, distance / span)
      const from = at(LINE.from)
      const to = at(LINE.to)
      const middle = (LINE.from + LINE.to) / 2
      edges.push({
        id: node.id,
        x1: from.x,
        y1: from.y,
        x2: to.x,
        y2: to.y,
        // Mid-line, clear of both labels, pointing at the relation's object.
        arrow:
          direction === 'outgoing'
            ? arrowhead(at(middle - 3), at(middle + 3))
            : arrowhead(at(middle + 3), at(middle - 3)),
        group: node.group,
        current: node.current
      })
    })
  }
  return { nodes, edges, relations: shown, hidden: assigned.size - nodes.length, height }
}

function point(x1: number, y1: number, x2: number, y2: number, t: number) {
  return { x: round(x1 + (x2 - x1) * t), y: round(y1 + (y2 - y1) * t) }
}

function arrowhead(from: { x: number; y: number }, tip: { x: number; y: number }): string {
  const length = Math.hypot(tip.x - from.x, tip.y - from.y) || 1
  const ux = (tip.x - from.x) / length
  const uy = (tip.y - from.y) / length
  const size = 2.4
  const base = { x: tip.x - ux * size, y: tip.y - uy * size }
  const half = size * 0.55
  return [
    [tip.x, tip.y],
    [base.x - uy * half, base.y + ux * half],
    [base.x + uy * half, base.y - ux * half]
  ]
    .map(([x, y]) => `${round(x)},${round(y)}`)
    .join(' ')
}

function round(value: number): number {
  return Math.round(value * 100) / 100
}
