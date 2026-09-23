import { assertKipSucceeded } from './api'
import type { BrainApi, BrainStatus, Json, KipOperation } from './api'
import { SvelteMap } from 'svelte/reactivity'

export interface Concept {
  _type?: 'ConceptNode'
  id: string
  type: string
  name: string
  attributes: Record<string, Json>
  metadata?: Record<string, Json>
  _raw?: Record<string, Json>
  _expanded?: boolean
  _isExpanding?: boolean
}

export interface Proposition {
  _type?: 'PropositionLink' | 'VirtualLink'
  id: string
  subject: string
  object: string
  predicate: string
  attributes: Record<string, Json>
  metadata?: Record<string, Json>
  _raw?: Record<string, Json>
  _expanded?: boolean
  _virtual?: boolean
}

export interface GraphSnapshot {
  nodes: Concept[]
  links: Proposition[]
  status: BrainStatus | null
  loadedAt: number
  partial: boolean
}

export interface GraphSummary {
  nodeCount: number
  linkCount: number
  visibleNodeCount: number
  visibleLinkCount: number
  typeCounts: Array<[string, number]>
  predicateCounts: Array<[string, number]>
  hubs: Array<{ id: string; name: string; type: string; degree: number }>
}

type IngestStats = {
  concepts: number
  propositions: number
}

const GRAPH_ROW_LIMIT = 6000
const GRAPH_CONCEPT_LIMIT = 6000
const SCHEMA_ROW_LIMIT = 500
const SEARCH_LIMIT = 32
const OVERVIEW_TYPE_LIMIT = 12
const OVERVIEW_TYPE_COUNT = 12
const EXPAND_LINK_LIMIT = 180
const INTERNAL_TYPES = new Set(['$ConceptType', '$PropositionType'])
const OVERVIEW_TYPE_PRIORITY = [
  'Event',
  'Insight',
  'Experience',
  'Skill',
  'Preference',
  'Person',
  'Release',
  'Project',
  'Task',
  'Website',
  'Organization',
  'Conversation',
  'Task'
]

export class BrainGraphData {
  readonly nodes = new SvelteMap<string, Concept>()
  readonly links = new SvelteMap<string, Proposition>()

  status = $state<BrainStatus | null>(null)
  loadedAt = $state(0)
  partial = $state(false)

  #typeIds: Map<string, string>
  #api: BrainApi
  #globalRef: BrainGraphData | null
  #initPromise: Promise<IngestStats> | null = null
  #overviewPromise: Promise<IngestStats> | null = null
  #expandConceptPromises = new Map<string, Promise<IngestStats>>()
  #linksVersion = 0
  #degreeCache: { version: number; value: Map<string, number> } | null = null
  #adjacencyCache: { version: number; value: Map<string, Proposition[]> } | null = null

  constructor(api: BrainApi, globalRef: BrainGraphData | null = null) {
    this.#api = api
    this.#globalRef = globalRef
    this.#typeIds = globalRef ? globalRef.#typeIds : new Map()
  }

  setApi(api: BrainApi): void {
    this.#api = api
  }

  ready(): Promise<IngestStats> {
    if (!this.#initPromise) {
      this.#initPromise = this.#initSchema().catch((error) => {
        this.#initPromise = null
        throw error
      })
    }
    return this.#initPromise
  }

  snapshot(): GraphSnapshot {
    return {
      nodes: Array.from(this.nodes.values()),
      links: Array.from(this.links.values()),
      status: this.status,
      loadedAt: this.loadedAt,
      partial: this.partial
    }
  }

  clear(): void {
    this.nodes.clear()
    this.links.clear()
    if (!this.#globalRef) {
      this.#typeIds.clear()
    }
    this.status = null
    this.loadedAt = 0
    this.partial = false
    this.#initPromise = null
    this.#overviewPromise = null
    this.#expandConceptPromises.clear()
    this.#linksVersion += 1
  }

  async loadOverview(): Promise<IngestStats> {
    if (!this.#overviewPromise) {
      this.#overviewPromise = this.#loadOverview().catch((error) => {
        this.#overviewPromise = null
        throw error
      })
    }
    return this.#overviewPromise
  }

  async #loadOverview(): Promise<IngestStats> {
    const stats: IngestStats = { concepts: 0, propositions: 0 }
    this.status = await this.#api.status().catch(() => null)

    await this.ready().then((result) => {
      stats.concepts += result.concepts
      stats.propositions += result.propositions
    })

    await this.loadOverviewConcepts().then((result) => {
      stats.concepts += result.concepts
      stats.propositions += result.propositions
    })

    const storedConceptCount = [...this.nodes.values()].filter(
      (node) => node._raw?.kind === 'concept'
    ).length
    if (this.status && storedConceptCount < this.status.concepts) {
      this.partial = true
    }

    this.loadedAt = Date.now()
    return stats
  }

  async #initSchema(): Promise<IngestStats> {
    if (this.conceptTypeNames().length > 0 && this.propositionTypeNames().length > 0) {
      return { concepts: 0, propositions: 0 }
    }

    if (this.#globalRef) {
      await this.#globalRef.ready()
      const stats: IngestStats = { concepts: 0, propositions: 0 }
      for (const concept of this.#globalRef.nodes.values()) {
        if (concept.type === '$ConceptType' || concept.type === '$PropositionType') {
          const before = this.nodes.has(concept.id)
          this.addConcept(cloneConcept(concept), false)
          if (!before) {
            stats.concepts += 1
          }
        }
      }
      return stats
    }

    return this.loadSchema()
  }

  async loadSchema(): Promise<IngestStats> {
    const response = await this.#api.executeKipReadonly<unknown>({
      execution: { mode: 'independent' },
      operations: [
        { command: 'LIST TYPES LIMIT :limit', parameters: { limit: SCHEMA_ROW_LIMIT } },
        { command: 'LIST PREDICATES LIMIT :limit', parameters: { limit: SCHEMA_ROW_LIMIT } }
      ]
    })
    assertKipSucceeded(response, 2)
    this.partial ||= hasNextCursor(response)
    let concepts = 0
    response.results.forEach((operation, index) => {
      if (!Array.isArray(operation.result)) throw new Error('KIP schema list must be an array')
      for (const value of operation.result) {
        const row = recordOf(value)
        if (!row || typeof row.ref !== 'string' || typeof row.local_name !== 'string') {
          throw new Error('KIP schema list is missing a symbol reference or local name')
        }
        // Schema symbols are client-side display nodes, never persisted Concepts.
        const id = `schema:${index}:${row.ref}`
        if (!this.nodes.has(id)) concepts += 1
        this.addConcept({
          id,
          type: index === 0 ? '$ConceptType' : '$PropositionType',
          name: row.local_name,
          attributes: {},
          metadata: { schema_ref: row.ref, display_only: true },
          _raw: row
        })
      }
    })
    return { concepts, propositions: 0 }
  }

  async loadLinksByPredicate(predicateNames = this.propositionTypeNames()): Promise<IngestStats> {
    const commands = Array.from(new Set(predicateNames))
      .filter(Boolean)
      .sort((left, right) => left.localeCompare(right))
      .map(
        (predicate): KipOperation => ({
          command: `FIND(?link)
WHERE {
  ?link (?s, :predicate, ?o)
}
LIMIT :limit`,
          parameters: { predicate, limit: GRAPH_ROW_LIMIT }
        })
      )

    if (!commands.length) {
      return { concepts: 0, propositions: 0 }
    }

    const response = await this.#api.executeKipReadonly<unknown>({
      execution: { mode: 'independent' },
      operations: commands
    })
    assertKipSucceeded(response)
    this.partial ||= hasNextCursor(response)
    return this.ingest(response)
  }

  async loadOverviewConcepts(): Promise<IngestStats> {
    return this.loadConceptsByType(this.overviewConceptTypeNames(), OVERVIEW_TYPE_LIMIT)
  }

  async loadConceptsByType(
    typeNames = this.conceptTypeNames(),
    limit = GRAPH_CONCEPT_LIMIT
  ): Promise<IngestStats> {
    const commands = Array.from(new Set(typeNames))
      .filter(Boolean)
      .sort((left, right) => left.localeCompare(right))
      .map(
        (type): KipOperation => ({
          command: `FIND(?node)
WHERE {
  ?node CONCEPT {type: :type}
}
LIMIT :limit`,
          parameters: { type, limit }
        })
      )

    if (!commands.length) {
      return { concepts: 0, propositions: 0 }
    }

    const response = await this.#api.executeKipReadonly<unknown>({
      execution: { mode: 'independent' },
      operations: commands
    })
    assertKipSucceeded(response)
    this.partial ||= hasNextCursor(response)
    return this.ingest(response)
  }

  conceptTypeNames(): string[] {
    return Array.from(this.nodes.values())
      .filter((node) => node.type === '$ConceptType')
      .map((node) =>
        typeof node.metadata?.schema_ref === 'string' ? node.metadata.schema_ref : node.name
      )
  }

  propositionTypeNames(): string[] {
    return Array.from(this.nodes.values())
      .filter((node) => node.type === '$PropositionType')
      .map((node) =>
        typeof node.metadata?.schema_ref === 'string' ? node.metadata.schema_ref : node.name
      )
  }

  overviewConceptTypeNames(): string[] {
    return this.conceptTypeNames()
      .filter((name) => !INTERNAL_TYPES.has(name))
      .sort(
        (left, right) =>
          overviewTypePriority(left) - overviewTypePriority(right) || left.localeCompare(right)
      )
      .slice(0, OVERVIEW_TYPE_COUNT)
  }

  async searchConcepts(term: string): Promise<Concept[]> {
    const query = term.trim()
    if (!query) {
      return []
    }
    const response = await this.#api.executeKipReadonly<unknown>({
      execution: { mode: 'independent' },
      operations: [
        {
          command: 'SEARCH CONCEPT :term LIMIT :limit',
          parameters: { term: query, limit: SEARCH_LIMIT }
        }
      ]
    })
    assertKipSucceeded(response, 1)
    this.partial ||= hasNextCursor(response)
    this.ingest(response)
    return collectConcepts(response)
      .map((concept) => this.nodes.get(concept.id))
      .filter((concept): concept is Concept => Boolean(concept))
  }

  async expandConcept(id: string): Promise<IngestStats> {
    const concept = this.nodes.get(id)
    if (concept?.type === '$ConceptType') {
      return this.loadConceptsByType(
        [String(concept.metadata?.schema_ref || concept.name)],
        EXPAND_LINK_LIMIT
      )
    }
    if (concept?.type === '$PropositionType') {
      return this.loadLinksByPredicate([String(concept.metadata?.schema_ref || concept.name)])
    }
    // Literal/foreign/reference display nodes have no local Concept to expand.
    if (concept?.metadata?.display_only === true && !/^C-\d+$/.test(id)) {
      return { concepts: 0, propositions: 0 }
    }
    const cached = this.#expandConceptPromises.get(id)
    if (cached) return cached
    if (concept) {
      this.nodes.set(id, { ...concept, _expanded: true, _isExpanding: true })
    }

    const promise = this.#expandConcept(id)
    this.#expandConceptPromises.set(id, promise)
    return promise
  }

  async #expandConcept(id: string): Promise<IngestStats> {
    try {
      const response = await this.#api.executeKipReadonly<unknown>({
        execution: { mode: 'independent' },
        operations: [
          {
            command: `FIND(?link, ?o)
WHERE {
  ?node CONCEPT {id: :id}
  ?link (?node, ?predicate, ?o)
}
LIMIT :limit`,
            parameters: { id, limit: EXPAND_LINK_LIMIT }
          },
          {
            command: `FIND(?s, ?link)
WHERE {
  ?node CONCEPT {id: :id}
  ?link (?s, ?predicate, ?node)
}
LIMIT :limit`,
            parameters: { id, limit: EXPAND_LINK_LIMIT }
          }
        ]
      })

      assertKipSucceeded(response, 2)
      this.partial ||= hasNextCursor(response)
      const stats = this.ingest(response)
      const next = this.nodes.get(id)
      if (next) {
        this.nodes.set(id, {
          ...next,
          _expanded: true,
          _isExpanding: false
        })
      }
      return stats
    } catch (error) {
      const next = this.nodes.get(id)
      if (next) {
        this.nodes.set(id, {
          ...next,
          _isExpanding: false
        })
      }
      this.#expandConceptPromises.delete(id)
      throw error
    }
  }

  async executeQuery(command: string): Promise<unknown> {
    const response = await this.#api.executeKipReadonly<unknown>({
      execution: { mode: 'independent' },
      operations: [{ command }]
    })
    assertKipSucceeded(response, 1)
    this.partial ||= hasNextCursor(response)
    this.ingest(response)
    return response
  }

  ingest(value: unknown): IngestStats {
    const stats: IngestStats = { concepts: 0, propositions: 0 }
    walkKipValue(value, (item) => {
      const concept = conceptFromValue(item)
      if (concept) {
        if (!this.nodes.has(concept.id)) stats.concepts += 1
        this.addConcept(concept)
        return
      }
      const row = recordOf(item)
      if (
        row?.kind !== 'proposition' ||
        typeof row.id !== 'string' ||
        typeof row.predicate_ref !== 'string'
      )
        return
      if (!('subject' in row) || !('object' in row)) return
      const subject = endpointNode(row.subject, row.id, 'subject')
      const object = endpointNode(row.object, row.id, 'object')
      for (const endpoint of [subject, object]) {
        if (!this.hasConcept(endpoint.id)) {
          this.addConcept(endpoint)
          stats.concepts += 1
        } else if (!this.nodes.has(endpoint.id)) {
          this.addConcept(cloneConcept(this.loadConcept(endpoint.id)!), false)
        }
      }
      if (!this.links.has(row.id)) stats.propositions += 1
      this.addProposition({
        id: row.id,
        subject: subject.id,
        object: object.id,
        predicate: localSymbol(row.predicate_ref),
        attributes: {},
        metadata: { predicate_ref: row.predicate_ref },
        _raw: row
      })
    })
    return stats
  }

  hasConcept(id: string): boolean {
    return this.nodes.has(id) || this.#globalRef?.hasConcept(id) || false
  }

  loadConcept(id: string): Concept | null {
    return this.nodes.get(id) || this.#globalRef?.loadConcept(id) || null
  }

  getNeighborLinks(id: string): Proposition[] {
    return [...(this.#adjacencyIndex().get(id) || [])]
  }

  addConcept(input: Concept, cache = true): void {
    if (cache && this.#globalRef) {
      this.#globalRef.addConcept(input)
    }

    const previous = this.nodes.get(input.id)
    const concept: Concept = {
      ...previous,
      ...input,
      attributes: input.attributes || previous?.attributes || {},
      metadata: input.metadata || previous?.metadata || {}
    }
    this.nodes.set(concept.id, concept)

    if (concept.type === '$ConceptType' || concept.type === '$PropositionType') {
      this.#typeIds.set(String(concept.metadata?.schema_ref || concept.name), concept.id)
    }

    const typeId = this.#typeIds.get(String(concept.metadata?.schema_ref || concept.type))
    if (typeId && typeId !== concept.id) {
      this.addProposition(
        {
          _type: 'VirtualLink',
          _virtual: true,
          _expanded: true,
          id: `virtual:instance_of:${concept.id}:${typeId}`,
          subject: concept.id,
          object: typeId,
          predicate: 'instance_of',
          attributes: {},
          metadata: {}
        },
        false
      )
    }
  }

  addProposition(input: Proposition, cache = true): void {
    if (cache && this.#globalRef) {
      this.#globalRef.addProposition(input)
    }

    const previous = this.links.get(input.id)
    const proposition: Proposition = {
      ...previous,
      ...input,
      attributes: input.attributes || previous?.attributes || {},
      metadata: input.metadata || previous?.metadata || {}
    }
    this.links.set(proposition.id, proposition)
    this.#linksVersion += 1
  }

  degreeByNode(): Map<string, number> {
    if (this.#degreeCache?.version === this.#linksVersion) {
      return this.#degreeCache.value
    }
    const degree = new Map<string, number>()
    for (const link of this.links.values()) {
      if (link._virtual) {
        continue
      }
      degree.set(link.subject, (degree.get(link.subject) || 0) + 1)
      degree.set(link.object, (degree.get(link.object) || 0) + 1)
    }
    this.#degreeCache = { version: this.#linksVersion, value: degree }
    return degree
  }

  #adjacencyIndex(): Map<string, Proposition[]> {
    if (this.#adjacencyCache?.version === this.#linksVersion) {
      return this.#adjacencyCache.value
    }
    const adjacency = new Map<string, Proposition[]>()
    const push = (id: string, link: Proposition) => {
      const list = adjacency.get(id)
      if (list) {
        list.push(link)
      } else {
        adjacency.set(id, [link])
      }
    }
    for (const link of this.links.values()) {
      push(link.subject, link)
      if (link.object !== link.subject) {
        push(link.object, link)
      }
    }
    this.#adjacencyCache = { version: this.#linksVersion, value: adjacency }
    return adjacency
  }

  neighborIds(id: string, radius = 1): Set<string> {
    const adjacency = this.#adjacencyIndex()
    const keep = new Set<string>([id])
    let frontier: string[] = [id]
    for (let i = 0; i < radius && frontier.length > 0; i += 1) {
      const next: string[] = []
      for (const nodeId of frontier) {
        for (const link of adjacency.get(nodeId) || []) {
          if (!keep.has(link.subject) && this.nodes.has(link.subject)) {
            keep.add(link.subject)
            next.push(link.subject)
          }
          if (!keep.has(link.object) && this.nodes.has(link.object)) {
            keep.add(link.object)
            next.push(link.object)
          }
        }
      }
      frontier = next
    }
    return keep
  }

  summary(visibleNodeCount = this.nodes.size, visibleLinkCount = this.links.size): GraphSummary {
    const degree = this.degreeByNode()
    const nodes = Array.from(this.nodes.values())
    const realLinks: Proposition[] = []
    for (const link of this.links.values()) {
      if (!link._virtual) {
        realLinks.push(link)
      }
    }
    return {
      nodeCount: this.nodes.size,
      linkCount: realLinks.length,
      visibleNodeCount,
      visibleLinkCount,
      typeCounts: topCounts(nodes, (node) => node.type, 12),
      predicateCounts: topCounts(realLinks, (link) => link.predicate, 12),
      hubs: nodes
        .map((node) => ({
          id: node.id,
          name: node.name,
          type: node.type,
          degree: degree.get(node.id) || 0
        }))
        .sort((left, right) => right.degree - left.degree || left.name.localeCompare(right.name))
        .slice(0, 12)
    }
  }
}

export function isConcept(value: unknown): value is Concept {
  if (!value || typeof value !== 'object') {
    return false
  }
  const item = value as Partial<Concept>
  return (
    typeof item.id === 'string' &&
    typeof item.type === 'string' &&
    typeof item.name === 'string' &&
    typeof (item as Partial<Proposition>).predicate !== 'string'
  )
}

export function isProposition(value: unknown): value is Proposition {
  if (!value || typeof value !== 'object') {
    return false
  }
  const item = value as Partial<Proposition>
  return (
    typeof item.id === 'string' &&
    typeof item.subject === 'string' &&
    typeof item.object === 'string' &&
    typeof item.predicate === 'string'
  )
}

function cloneConcept(concept: Concept): Concept {
  return {
    ...concept,
    attributes: cloneJsonRecord(concept.attributes),
    metadata: concept.metadata ? cloneJsonRecord(concept.metadata) : undefined
  }
}

function cloneJsonRecord(value: Record<string, Json>): Record<string, Json> {
  return JSON.parse(JSON.stringify(value)) as Record<string, Json>
}

function recordOf(value: unknown): Record<string, Json> | null {
  return value && typeof value === 'object' && !Array.isArray(value)
    ? (value as Record<string, Json>)
    : null
}

function localSymbol(reference: string): string {
  return reference.slice(reference.lastIndexOf('/') + 1)
}

function conceptFromValue(value: unknown): Concept | null {
  const row = recordOf(value)
  if (!row || row.kind !== 'concept' || typeof row.id !== 'string' || 'element' in row) return null
  return {
    id: row.id,
    type: typeof row.schema_ref === 'string' ? localSymbol(row.schema_ref) : 'Reference',
    name: typeof row.name === 'string' ? row.name : row.id,
    attributes: recordOf(row.attributes) || {},
    metadata: {
      ...(recordOf(row._system) || {}),
      schema_ref: row.schema_ref || '',
      ...(row.withheld ? { withheld: row.withheld } : {})
    },
    _raw: row
  }
}

function endpointNode(value: Json, proposition: string, side: string): Concept {
  const reference = recordOf(value)
  const localId = reference && typeof reference.id === 'string' ? reference.id : null
  const canonicalId =
    reference && typeof reference.canonical_id === 'string' ? reference.canonical_id : null
  const foreignSpaceId =
    reference && typeof reference.space_id === 'string' ? reference.space_id : null
  const foreignElementId =
    reference && typeof reference.element_id === 'string' ? reference.element_id : null
  const foreign = foreignSpaceId !== null && foreignElementId !== null
  // Core literals are bare string/number/boolean/null; arbitrary objects and
  // arrays are not reference shortcuts. Preserve that distinction on the graph.
  if (Array.isArray(value) || (reference && !localId && canonicalId === null && !foreign)) {
    throw new Error(`Invalid KIP tuple endpoint: ${JSON.stringify(value)}`)
  }
  const literal = !reference
  const id = localId
    ? localId
    : canonicalId !== null
      ? `reference:canonical:${encodeURIComponent(canonicalId)}`
      : foreign
        ? `reference:foreign:${encodeURIComponent(foreignSpaceId)}:${encodeURIComponent(foreignElementId)}`
        : `literal:${proposition}:${side}`
  return {
    id,
    type: literal ? 'Literal' : 'Reference',
    name: localId || (typeof value === 'string' ? value : JSON.stringify(value)),
    attributes: { value },
    metadata: { display_only: true }
  }
}

function collectConcepts(value: unknown): Concept[] {
  const concepts: Concept[] = []
  walkKipValue(value, (item) => {
    const concept = conceptFromValue(item)
    if (concept) concepts.push(concept)
  })
  return concepts
}

function walkKipValue(value: unknown, visit: (value: unknown) => void): void {
  if (Array.isArray(value)) {
    for (const item of value) {
      walkKipValue(item, visit)
    }
    return
  }

  if (!value || typeof value !== 'object') {
    return
  }

  visit(value)

  const record = value as Record<string, unknown>
  // Only protocol containers are traversed; user attributes/payloads are data.
  for (const key of ['results', 'result', 'hits', 'element']) {
    if (record[key] !== undefined) walkKipValue(record[key], visit)
  }
}

function hasNextCursor(value: unknown): boolean {
  if (Array.isArray(value)) {
    return value.some(hasNextCursor)
  }
  if (!value || typeof value !== 'object') {
    return false
  }
  const record = value as Record<string, unknown>
  return (
    typeof record.next_cursor === 'string' ||
    hasNextCursor(record.results) ||
    hasNextCursor(record.result)
  )
}

function overviewTypePriority(type: string): number {
  const index = OVERVIEW_TYPE_PRIORITY.indexOf(localSymbol(type))
  return index === -1 ? OVERVIEW_TYPE_PRIORITY.length : index
}

function topCounts<T>(
  items: T[],
  key: (item: T) => string,
  limit: number
): Array<[string, number]> {
  const counts = new Map<string, number>()
  for (const item of items) {
    const k = key(item)
    counts.set(k, (counts.get(k) || 0) + 1)
  }
  return Array.from(counts.entries())
    .sort((left, right) => right[1] - left[1] || left[0].localeCompare(right[0]))
    .slice(0, limit)
}
