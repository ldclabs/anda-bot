import type { BrainApi, BrainStatus, Json, KipCommandItem, KipError, KipResponse } from './api'
import { SvelteMap } from 'svelte/reactivity'

export interface Concept {
  _type?: 'ConceptNode'
  id: string
  type: string
  name: string
  attributes: Record<string, Json>
  metadata?: Record<string, Json>
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
  'Domain',
  'Preference',
  'Person',
  'Release',
  'Project',
  'SleepTask',
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

    if (this.status && this.nodes.size < this.status.concepts) {
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
    const response = await this.#api.executeKipReadonly<Array<KipResponse<unknown>>>({
      commands: [
        {
          command: `FIND(?ct)
WHERE {
  ?ct {type: "$ConceptType"}
}
LIMIT :limit`,
          parameters: { limit: SCHEMA_ROW_LIMIT }
        },
        {
          command: `FIND(?pt)
WHERE {
  ?pt {type: "$PropositionType"}
}
LIMIT :limit`,
          parameters: { limit: SCHEMA_ROW_LIMIT }
        }
      ]
    })
    assertNoKipErrors(response.result)
    this.partial ||= hasNextCursor(response.result)
    return this.ingest(response.result)
  }

  async loadLinksByPredicate(predicateNames = this.propositionTypeNames()): Promise<IngestStats> {
    const commands = Array.from(new Set(predicateNames))
      .filter(Boolean)
      .sort((left, right) => left.localeCompare(right))
      .map(
        (predicate): KipCommandItem => ({
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

    const response = await this.#api.executeKipReadonly<Array<KipResponse<unknown>>>({
      commands
    })
    assertNoKipErrors(response.result)
    this.partial ||= hasNextCursor(response.result)
    return this.ingest(response.result)
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
        (type): KipCommandItem => ({
          command: `FIND(?node)
WHERE {
  ?node {type: :type}
}
LIMIT :limit`,
          parameters: { type, limit }
        })
      )

    if (!commands.length) {
      return { concepts: 0, propositions: 0 }
    }

    const response = await this.#api.executeKipReadonly<Array<KipResponse<unknown>>>({
      commands
    })
    assertNoKipErrors(response.result)
    this.partial ||= hasNextCursor(response.result)
    return this.ingest(response.result)
  }

  conceptTypeNames(): string[] {
    return Array.from(this.nodes.values())
      .filter((node) => node.type === '$ConceptType')
      .map((node) => node.name)
  }

  propositionTypeNames(): string[] {
    return Array.from(this.nodes.values())
      .filter((node) => node.type === '$PropositionType')
      .map((node) => node.name)
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
      command: 'SEARCH CONCEPT :term LIMIT :limit',
      parameters: {
        term: query,
        limit: SEARCH_LIMIT
      }
    })
    this.ingest(response.result)
    return collectConcepts(response.result)
      .map((concept) => this.nodes.get(concept.id))
      .filter((concept): concept is Concept => Boolean(concept))
  }

  async expandConcept(id: string): Promise<IngestStats> {
    const concept = this.nodes.get(id)
    if (concept) {
      this.nodes.set(id, {
        ...concept,
        _expanded: true,
        _isExpanding: true
      })
    }

    const cached = this.#expandConceptPromises.get(id)
    if (cached) {
      return cached
    }

    const promise = this.#expandConcept(id)
    this.#expandConceptPromises.set(id, promise)
    return promise
  }

  async #expandConcept(id: string): Promise<IngestStats> {
    try {
      const response = await this.#api.executeKipReadonly<Array<KipResponse<unknown>>>({
        commands: [
          {
            command: `FIND(?link, ?o)
WHERE {
  ?link ({id: :id}, ?predicate, ?o)
}
LIMIT :limit`,
            parameters: { id, limit: EXPAND_LINK_LIMIT }
          },
          {
            command: `FIND(?s, ?link)
WHERE {
  ?link (?s, ?predicate, {id: :id})
}
LIMIT :limit`,
            parameters: { id, limit: EXPAND_LINK_LIMIT }
          }
        ]
      })

      assertNoKipErrors(response.result)
      this.partial ||= hasNextCursor(response.result)
      const stats = this.ingest(response.result)
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
      command
    })
    this.ingest(response.result)
    return response.result
  }

  ingest(value: unknown): IngestStats {
    const stats: IngestStats = { concepts: 0, propositions: 0 }
    walkKipValue(value, (item) => {
      if (isConcept(item)) {
        if (!this.nodes.has(item.id)) {
          stats.concepts += 1
        }
        this.addConcept(item)
      } else if (isProposition(item)) {
        if (!this.links.has(item.id)) {
          stats.propositions += 1
        }
        this.addProposition(item)
      }
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
    const links: Proposition[] = []
    for (const link of this.links.values()) {
      if (link.subject === id || link.object === id) {
        links.push(link)
      }
    }
    return links
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
      this.#typeIds.set(concept.name, concept.id)
    }

    const typeId = this.#typeIds.get(concept.type)
    if (typeId && typeId !== concept.id) {
      this.addProposition({
        _type: 'VirtualLink',
        _virtual: true,
        _expanded: true,
        id: `virtual:instance_of:${concept.id}:${typeId}`,
        subject: concept.id,
        object: typeId,
        predicate: 'instance_of',
        attributes: {},
        metadata: {}
      }, false)
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
  }

  degreeByNode(): Map<string, number> {
    const degree = new Map<string, number>()
    for (const link of this.links.values()) {
      if (link._virtual) {
        continue
      }
      degree.set(link.subject, (degree.get(link.subject) || 0) + 1)
      degree.set(link.object, (degree.get(link.object) || 0) + 1)
    }
    return degree
  }

  neighborIds(id: string, radius = 1): Set<string> {
    const keep = new Set<string>([id])
    let frontier = new Set<string>([id])
    for (let i = 0; i < radius; i += 1) {
      const next = new Set<string>()
      for (const link of this.links.values()) {
        const touchesSubject = frontier.has(link.subject)
        const touchesObject = frontier.has(link.object)
        if (!touchesSubject && !touchesObject) {
          continue
        }
        if (this.nodes.has(link.subject)) {
          next.add(link.subject)
        }
        if (this.nodes.has(link.object)) {
          next.add(link.object)
        }
      }
      for (const nodeId of next) {
        keep.add(nodeId)
      }
      frontier = next
    }
    return keep
  }

  summary(visibleNodeCount = this.nodes.size, visibleLinkCount = this.links.size): GraphSummary {
    const degree = this.degreeByNode()
    return {
      nodeCount: this.nodes.size,
      linkCount: Array.from(this.links.values()).filter((link) => !link._virtual).length,
      visibleNodeCount,
      visibleLinkCount,
      typeCounts: topCounts(Array.from(this.nodes.values()), (node) => node.type, 12),
      predicateCounts: topCounts(
        Array.from(this.links.values()).filter((link) => !link._virtual),
        (link) => link.predicate,
        12
      ),
      hubs: Array.from(this.nodes.values())
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

function collectConcepts(value: unknown): Concept[] {
  const concepts: Concept[] = []
  walkKipValue(value, (item) => {
    if (isConcept(item)) {
      concepts.push(item)
    }
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
  if (record.result !== undefined || record.error !== undefined) {
    walkKipValue(record.result, visit)
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
  return typeof record.next_cursor === 'string' || hasNextCursor(record.result)
}

function assertNoKipErrors(value: unknown): void {
  const error = findKipError(value)
  if (error) {
    throw new Error(formatKipError(error))
  }
}

function findKipError(value: unknown): KipError | null {
  if (Array.isArray(value)) {
    for (const item of value) {
      const error = findKipError(item)
      if (error) {
        return error
      }
    }
    return null
  }
  if (!value || typeof value !== 'object') {
    return null
  }

  const record = value as Record<string, unknown>
  if (isKipError(record.error)) {
    return record.error
  }
  return findKipError(record.result)
}

function isKipError(value: unknown): value is KipError {
  return Boolean(value && typeof value === 'object' && typeof (value as KipError).message === 'string')
}

function formatKipError(error: KipError): string {
  const prefix = error.code ? `${error.code}: ` : ''
  const hint = error.hint ? ` ${error.hint}` : ''
  return `${prefix}${error.message}${hint}`
}

function overviewTypePriority(type: string): number {
  const index = OVERVIEW_TYPE_PRIORITY.indexOf(type)
  return index === -1 ? OVERVIEW_TYPE_PRIORITY.length : index
}

function topCounts<T>(items: T[], key: (item: T) => string, limit: number): Array<[string, number]> {
  const counts = new Map<string, number>()
  for (const item of items) {
    const k = key(item)
    counts.set(k, (counts.get(k) || 0) + 1)
  }
  return Array.from(counts.entries())
    .sort((left, right) => right[1] - left[1] || left[0].localeCompare(right[0]))
    .slice(0, limit)
}
