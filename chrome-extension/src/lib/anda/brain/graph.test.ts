import { describe, expect, it, vi } from 'vitest'
import type { BrainApi, BrainStatus, KipOperation, KipRequest } from './api'
import { BrainGraphData } from './graph.svelte'

const status: BrainStatus = {
  id: 'anda_bot',
  concepts: 900,
  propositions: 2808,
  conversations: 0,
  formation_processing: false,
  maintenance_processing: false,
  formation_processed_id: 0,
  maintenance_processed_id: 0
}

// The raw shapes below match Nexus view.rs and META LIST/SEARCH: Concepts
// carry schema_ref, tuples carry reference objects/literal values, and SEARCH
// wraps elements inside hits. The Rust router contract test verifies the host.
const person = {
  id: 'C-1',
  kind: 'concept',
  schema_ref: 'kip://profiles/cognitive-memory@2.1.0/Event',
  name: 'First event',
  attributes: {},
  _system: { version: 1, updated_at: '2026-09-13T00:00:00Z' }
}
const tuple = {
  id: 'P-1',
  kind: 'proposition',
  subject: { id: 'C-1' },
  predicate_ref: 'kip://brain/test@1.0.0/related_to',
  object: { id: 'C-2' }
}

function ok(...values: unknown[]) {
  return {
    kip: '2.0',
    status: 'succeeded',
    results: values.map((result) => ({ status: 'succeeded', result }))
  }
}

function createApi() {
  const executeKipReadonly = vi.fn(async (request: KipRequest) =>
    ok(
      ...request.operations.map(({ command }) => {
        if (command.startsWith('LIST TYPES'))
          return [
            {
              ref: person.schema_ref,
              local_name: 'Event',
              package_ref: 'kip://profiles/cognitive-memory@2.1.0',
              status: 'active'
            }
          ]
        if (command.startsWith('LIST PREDICATES'))
          return [
            {
              ref: tuple.predicate_ref,
              local_name: 'related_to',
              package_ref: 'kip://brain/test@1.0.0',
              status: 'active'
            }
          ]
        if (command.includes('FIND(?link, ?o)'))
          return [[tuple, { ...person, id: 'C-2', name: 'Neighbor' }]]
        if (command.includes('FIND(?s, ?link)')) return [[person, tuple]]
        if (command.includes('?link')) return [tuple]
        if (command.includes('?node')) return [person]
        if (command.startsWith('SEARCH'))
          return {
            hits: [{ id: person.id, kind: 'concept', score: 0.5, element: person }],
            search_context: { mode: 'keyword', score_semantics: 'bm25_relevance_not_confidence' }
          }
        throw new Error(`unexpected KIP request: ${JSON.stringify(request)}`)
      })
    )
  )
  return {
    api: { status: vi.fn(async () => status), executeKipReadonly } as unknown as BrainApi,
    executeKipReadonly
  }
}

function commandObjects(request: KipRequest): KipOperation[] {
  return request.operations
}

describe('BrainGraphData', () => {
  it('loads a bounded overview without preloading every proposition', async () => {
    const { api, executeKipReadonly } = createApi()
    const graph = new BrainGraphData(api)

    await graph.loadOverview()

    const requests = executeKipReadonly.mock.calls.map((call) => call[0] as KipRequest)
    const overviewCommands = JSON.stringify(requests)
    expect(overviewCommands).toContain('FIND(?node)')
    expect(overviewCommands).not.toContain('FIND(?link)')
    for (const request of requests) {
      for (const command of commandObjects(request)) {
        if (command.command.includes('FIND(?node)')) {
          expect(command.parameters?.limit).toBeLessThanOrEqual(12)
        }
      }
    }
    expect(graph.nodes.has('C-1')).toBe(true)
    expect(Array.from(graph.links.values()).some((link) => !link._virtual)).toBe(false)
  })

  it('loads predicate links without projecting unbound proposition endpoint variables', async () => {
    const { api, executeKipReadonly } = createApi()
    const graph = new BrainGraphData(api)

    await graph.loadSchema()
    await graph.loadLinksByPredicate(['related_to'])

    const linkRequest = executeKipReadonly.mock.calls.at(-1)?.[0] as KipRequest
    expect(JSON.stringify(linkRequest.operations)).toContain('FIND(?link)')
    expect(JSON.stringify(linkRequest.operations)).not.toContain('FIND(?link, ?s')
    expect(JSON.stringify(linkRequest.operations)).not.toContain('?predicate')
    expect(graph.links.has('P-1')).toBe(true)
  })

  it('caps node expansion queries', async () => {
    const { api, executeKipReadonly } = createApi()
    const graph = new BrainGraphData(api)
    graph.addConcept({ id: 'C-1', type: 'Event', name: 'First event', attributes: {} })

    await graph.expandConcept('C-1')

    const expandRequest = executeKipReadonly.mock.calls.at(-1)?.[0] as KipRequest
    expect(graph.nodes.get('C-2')?.name).toBe('Neighbor')
    expect(graph.links.get('P-1')?.object).toBe('C-2')
    expect(expandRequest.operations).toHaveLength(2)
    for (const command of commandObjects(expandRequest)) {
      expect(command.command).toContain('LIMIT :limit')
      expect(command.parameters?.limit).toBeLessThanOrEqual(180)
    }
  })

  it('deduplicates concurrent overview loading', async () => {
    const { api, executeKipReadonly } = createApi()
    const graph = new BrainGraphData(api)

    await Promise.all([graph.loadOverview(), graph.loadOverview()])

    expect(executeKipReadonly).toHaveBeenCalledTimes(2)
  })

  it('deduplicates repeated concept expansion requests', async () => {
    const { api, executeKipReadonly } = createApi()
    const graph = new BrainGraphData(api)
    graph.addConcept({ id: 'C-1', type: 'Event', name: 'First event', attributes: {} })

    await Promise.all([graph.expandConcept('C-1'), graph.expandConcept('C-1')])

    expect(executeKipReadonly).toHaveBeenCalledTimes(1)
  })

  it('can reuse schema and cache items through a global graph reference', async () => {
    const { api } = createApi()
    const globalGraph = new BrainGraphData(api)
    globalGraph.addConcept({ id: 'ct-event', type: '$ConceptType', name: 'Event', attributes: {} })
    globalGraph.addConcept({
      id: 'pt-related',
      type: '$PropositionType',
      name: 'related_to',
      attributes: {}
    })

    const localExecute = vi.fn()
    const localGraph = new BrainGraphData(
      {
        status: vi.fn(async () => status),
        executeKipReadonly: localExecute
      } as unknown as BrainApi,
      globalGraph
    )

    await localGraph.ready()
    localGraph.addConcept({ id: 'event-1', type: 'Event', name: 'First event', attributes: {} })

    expect(localExecute).not.toHaveBeenCalled()
    expect(localGraph.nodes.has('ct-event')).toBe(true)
    expect(localGraph.links.has('virtual:instance_of:event-1:ct-event')).toBe(true)
    expect(globalGraph.nodes.has('event-1')).toBe(true)
  })

  it('surfaces an operation failure even under a succeeded envelope', async () => {
    const api = {
      status: vi.fn(async () => status),
      executeKipReadonly: vi.fn(async () => ({
        kip: '2.0',
        status: 'succeeded',
        results: [
          {
            status: 'failed',
            result: [],
            error: { code: 'UnboundVariable', message: 'Unbound variable: s' }
          }
        ]
      }))
    } as unknown as BrainApi
    await expect(new BrainGraphData(api).loadOverview()).rejects.toThrow(
      'UnboundVariable: Unbound variable: s'
    )
  })

  it('reads SEARCH hits without treating relevance as confidence or an empty hit list as memory', async () => {
    const { api, executeKipReadonly } = createApi()
    const graph = new BrainGraphData(api)
    expect(await graph.searchConcepts('event')).toMatchObject([{ id: 'C-1', type: 'Event' }])
    expect(graph.nodes.get('C-1')?.attributes).not.toHaveProperty('confidence')
    expect(graph.nodes.get('C-1')?.metadata?.updated_at).toBe('2026-09-13T00:00:00Z')
    executeKipReadonly.mockResolvedValueOnce(ok({ hits: [], search_context: { mode: 'keyword' } }))
    expect(await graph.searchConcepts('missing')).toEqual([])
  })

  it('keeps literal endpoints distinct from reference ids and preserves raw audit data', () => {
    const { api } = createApi()
    const graph = new BrainGraphData(api)
    graph.ingest(ok([person, tuple, { ...tuple, id: 'P-2', object: 'C-2' }]))
    expect(graph.links.get('P-1')?.object).toBe('C-2')
    expect(graph.links.get('P-2')?.object).toBe('literal:P-2:object')
    expect(graph.nodes.get('literal:P-2:object')).toMatchObject({
      type: 'Literal',
      attributes: { value: 'C-2' }
    })
    expect(graph.links.get('P-2')?._raw?.object).toBe('C-2')
    expect(graph.links.get('P-2')?.attributes).not.toHaveProperty('confidence')
    expect(() =>
      graph.ingest(ok([{ ...tuple, id: 'P-3', object: { arbitrary: 'not a reference' } }]))
    ).toThrow('Invalid KIP tuple endpoint')
  })

  it('represents schema symbols only as display nodes and uses their qualified references in queries', async () => {
    const { api, executeKipReadonly } = createApi()
    const graph = new BrainGraphData(api)
    await graph.loadSchema()
    expect(graph.conceptTypeNames()).toEqual([person.schema_ref])
    expect([...graph.nodes.values()].every((node) => node.metadata?.display_only === true)).toBe(
      true
    )
    const schemaId = [...graph.nodes.values()].find((node) => node.type === '$ConceptType')!.id
    await graph.expandConcept(schemaId)
    expect(executeKipReadonly.mock.calls.at(-1)?.[0].operations[0].parameters?.type).toBe(
      person.schema_ref
    )
  })

  it('declares independent execution for every schema, overview, link and expansion batch', async () => {
    const { api, executeKipReadonly } = createApi()
    const graph = new BrainGraphData(api)
    await graph.loadSchema()
    await graph.loadConceptsByType(['Person', 'Event'], 5)
    await graph.loadLinksByPredicate(['prefers', 'related_to'])
    await graph.expandConcept('C-1')
    const batches = executeKipReadonly.mock.calls
      .map(([request]) => request)
      .filter((request) => request.operations.length > 1)
    expect(batches).toHaveLength(4)
    for (const request of batches) expect(request.execution).toEqual({ mode: 'independent' })
  })
})
