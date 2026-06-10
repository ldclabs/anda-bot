import { describe, expect, it, vi } from 'vitest'
import type { BrainApi, BrainStatus, KipCommandItem, KipRequest } from './api'
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

function createApi() {
  const executeKipReadonly = vi.fn(async (request: KipRequest) => {
    if (request.commands?.some((command) => JSON.stringify(command).includes('?node'))) {
      return {
        result: [
          {
            result: [{ id: 'event-1', type: 'Event', name: 'First event', attributes: {} }]
          }
        ]
      }
    }

    if (request.commands?.some((command) => JSON.stringify(command).includes('?link'))) {
      return {
        result: [
          {
            result: [
              {
                id: 'link-1',
                subject: 'event-1',
                predicate: 'related_to',
                object: 'ct-event',
                attributes: {}
              }
            ]
          }
        ]
      }
    }

    if (request.commands?.some((command) => JSON.stringify(command).includes('$ConceptType'))) {
      return {
        result: [
          {
            result: [
              { id: 'ct-concept', type: '$ConceptType', name: '$ConceptType', attributes: {} },
              { id: 'ct-proposition', type: '$ConceptType', name: '$PropositionType', attributes: {} },
              { id: 'ct-event', type: '$ConceptType', name: 'Event', attributes: {} }
            ]
          },
          {
            result: [
              {
                id: 'pt-related',
                type: '$PropositionType',
                name: 'related_to',
                attributes: {}
              }
            ]
          }
        ]
      }
    }

    throw new Error(`unexpected KIP request: ${JSON.stringify(request)}`)
  })

  return {
    api: {
      status: vi.fn(async () => status),
      executeKipReadonly
    } as unknown as BrainApi,
    executeKipReadonly
  }
}

function commandObjects(request: KipRequest): Array<Extract<KipCommandItem, { command: string }>> {
  return (request.commands || []).filter(
    (command): command is Extract<KipCommandItem, { command: string }> => typeof command !== 'string'
  )
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
    expect(graph.nodes.has('event-1')).toBe(true)
    expect(Array.from(graph.links.values()).some((link) => !link._virtual)).toBe(false)
  })

  it('loads predicate links without projecting unbound proposition endpoint variables', async () => {
    const { api, executeKipReadonly } = createApi()
    const graph = new BrainGraphData(api)

    await graph.loadSchema()
    await graph.loadLinksByPredicate(['related_to'])

    const linkRequest = executeKipReadonly.mock.calls.at(-1)?.[0] as KipRequest
    expect(JSON.stringify(linkRequest.commands)).toContain('FIND(?link)')
    expect(JSON.stringify(linkRequest.commands)).not.toContain('FIND(?link, ?s')
    expect(JSON.stringify(linkRequest.commands)).not.toContain('?predicate')
    expect(graph.links.has('link-1')).toBe(true)
  })

  it('caps node expansion queries', async () => {
    const { api, executeKipReadonly } = createApi()
    const graph = new BrainGraphData(api)
    graph.addConcept({ id: 'event-1', type: 'Event', name: 'First event', attributes: {} })

    await graph.expandConcept('event-1')

    const expandRequest = executeKipReadonly.mock.calls.at(-1)?.[0] as KipRequest
    expect(expandRequest.commands).toHaveLength(2)
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
    graph.addConcept({ id: 'event-1', type: 'Event', name: 'First event', attributes: {} })

    await Promise.all([graph.expandConcept('event-1'), graph.expandConcept('event-1')])

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

  it('surfaces nested KIP batch errors', async () => {
    const api = {
      status: vi.fn(async () => status),
      executeKipReadonly: vi.fn(async () => ({
        result: [
          {
            error: {
              code: 'KIP_3001',
              message: 'Unbound variable: "s"',
              hint: 'Bind the variable first'
            }
          }
        ]
      }))
    } as unknown as BrainApi
    const graph = new BrainGraphData(api)

    await expect(graph.loadOverview()).rejects.toThrow('KIP_3001: Unbound variable: "s"')
  })
})
