import { describe, expect, it } from 'vitest'
import type { EntityClaim, MemoryBelief } from './api'
import { beliefKey, groupClaims, isCurrent, layoutEntityGraph } from './entity'

function claim(
  id: string,
  predicate: string,
  other: string | null,
  options: {
    direction?: EntityClaim['direction']
    state?: string
    belief?: Partial<MemoryBelief> | null
  } = {}
): EntityClaim {
  return {
    direction: options.direction ?? 'outgoing',
    other: { id: other, label: other ? `Entity ${other}` : 'literal' },
    belief:
      options.belief === null
        ? null
        : { status: 'accepted', excluded_reason: null, ...options.belief },
    record: {
      id,
      revision: '1',
      text: id,
      kind: 'other',
      scope: {},
      effective_at: null,
      subject_label: 'Owner',
      predicate_label: predicate,
      object_label: other ?? 'literal',
      about_owner: true,
      stance: 'support',
      state: options.state ?? 'active',
      updated_at: null,
      sources: [],
      sources_complete: true,
      allowed_actions: []
    }
  }
}

describe('entity claims', () => {
  it('treats replaced, rejected and inactive claims as earlier', () => {
    expect(isCurrent(claim('A-1', 'prefers', 'C-2'))).toBe(true)
    expect(isCurrent(claim('A-1', 'prefers', 'C-2', { belief: null }))).toBe(true)
    // Imported memory the policy does not count has not been replaced.
    expect(
      isCurrent(
        claim('A-1', 'prefers', 'C-2', {
          belief: { status: 'insufficient', excluded_reason: 'policy_excluded' }
        })
      )
    ).toBe(true)
    for (const options of [
      { belief: { status: 'insufficient', excluded_reason: 'outside_valid_time' } },
      { belief: { status: 'rejected' } },
      { state: 'superseded' },
      { state: 'archived' }
    ]) {
      expect(isCurrent(claim('A-1', 'prefers', 'C-2', options))).toBe(false)
    }
  })

  it('names beliefs by why a claim does or does not count', () => {
    expect(beliefKey(claim('A-1', 'prefers', 'C-2'))).toBe('memoryBelief_accepted')
    expect(
      beliefKey(
        claim('A-1', 'prefers', 'C-2', {
          belief: { status: 'insufficient', excluded_reason: 'outside_valid_time' }
        })
      )
    ).toBe('memoryBelief_ended')
    expect(
      beliefKey(claim('A-1', 'prefers', 'C-2', { belief: { excluded_reason: 'untrusted' } }))
    ).toBe('memoryBelief_excluded')
    expect(beliefKey(claim('A-1', 'prefers', 'C-2', { belief: { status: 'contested' } }))).toBe(
      'memoryBelief_contested'
    )
    expect(beliefKey(claim('A-1', 'prefers', 'C-2', { belief: { status: 'new' } }))).toBeNull()
    expect(beliefKey(claim('A-1', 'prefers', 'C-2', { belief: null }))).toBeNull()
  })

  it('groups by relation and direction, putting relations with a current claim first', () => {
    const groups = groupClaims([
      claim('A-5', 'mentions', 'C-5', { state: 'retracted' }),
      claim('A-4', 'prefers', 'C-4'),
      claim('A-3', 'involves', 'C-3', { direction: 'incoming' }),
      claim('A-2', 'prefers', 'C-2', {
        belief: { status: 'insufficient', excluded_reason: 'outside_valid_time' }
      })
    ])
    expect(groups.map((group) => group.key)).toEqual([
      'outgoing:prefers',
      'incoming:involves',
      'outgoing:mentions'
    ])
    expect(groups[0].current.map((item) => item.record.id)).toEqual(['A-4'])
    expect(groups[0].earlier.map((item) => item.record.id)).toEqual(['A-2'])
    expect(groups[2].current).toEqual([])
  })
})

describe('entity graph layout', () => {
  it('draws each related entity once: relations into it left, out of it right', () => {
    const layout = layoutEntityGraph('C-1', [
      claim('A-1', 'prefers', 'C-2'),
      claim('A-2', 'prefers', 'C-3', {
        belief: { status: 'insufficient', excluded_reason: 'outside_valid_time' }
      }),
      claim('A-3', 'involves', 'C-4', { direction: 'incoming' }),
      claim('A-4', 'mentions', 'C-2'),
      claim('A-5', 'mentions', 'C-1'),
      claim('A-6', 'named', null)
    ])
    expect(layout.nodes.map((node) => [node.id, node.x])).toEqual([
      ['C-4', 22],
      ['C-2', 138],
      ['C-3', 138]
    ])
    expect(layout.relations).toEqual([
      { direction: 'outgoing', predicate: 'prefers', group: 0, nodes: 2 },
      { direction: 'incoming', predicate: 'involves', group: 1, nodes: 1 }
    ])
    // Rows are centred and one row apart; no two labels share a row.
    expect(layout.height).toBe(48)
    expect(layout.nodes.map((node) => node.y)).toEqual([24, 20, 28])
    expect(layout.nodes[2].current).toBe(false)
    expect(layout.edges.find((edge) => edge.id === 'C-3')!.current).toBe(false)
    expect(layout.hidden).toBe(0)
  })

  it('points arrows at the object of each relation', () => {
    const layout = layoutEntityGraph('C-1', [
      claim('A-1', 'prefers', 'C-2'),
      claim('A-2', 'involves', 'C-3', { direction: 'incoming' })
    ])
    const center = { x: 80, y: layout.height / 2 }
    const distance = ([x, y]: number[]) => Math.hypot(x - center.x, y - center.y)
    const arrow = (id: string) => {
      const [tip, left, right] = layout.edges
        .find((edge) => edge.id === id)!
        .arrow.split(' ')
        .map((point) => point.split(',').map(Number))
      return {
        tip: distance(tip),
        base: distance([(left[0] + right[0]) / 2, (left[1] + right[1]) / 2])
      }
    }
    // Outgoing: the tip points away from the entity, towards the related one.
    expect(arrow('C-2').tip).toBeGreaterThan(arrow('C-2').base)
    // Incoming: the tip points at the entity itself.
    expect(arrow('C-3').tip).toBeLessThan(arrow('C-3').base)
  })

  it('caps each column, grows with its rows and reports what it left out', () => {
    const claims = Array.from({ length: 30 }, (_, index) =>
      claim(`A-${index}`, 'mentions', `C-${index + 10}`)
    )
    const layout = layoutEntityGraph('C-1', claims, 12, 8)
    expect(layout.nodes).toHaveLength(12)
    expect(layout.edges).toHaveLength(12)
    expect(layout.hidden).toBe(18)
    expect(layout.height).toBe(112)
    const rows = layout.nodes.map((node) => node.y)
    expect(Math.min(...rows)).toBeGreaterThan(0)
    expect(Math.max(...rows)).toBeLessThan(layout.height)
    expect(new Set(rows).size).toBe(12)
    expect(layoutEntityGraph('C-1', [])).toMatchObject({
      nodes: [],
      edges: [],
      relations: [],
      hidden: 0
    })
  })
})
