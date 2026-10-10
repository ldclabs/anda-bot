import { describe, expect, it } from 'vitest'
import type { ChatMessage } from '../client/types'
import { EntranceLedger, entranceFreshMs } from './entrance'

function message(id: string, patch: Partial<ChatMessage> = {}): ChatMessage {
  return { id, conversation: 1, role: 'assistant', text: 'hello', timestamp: 1_000_000, ...patch }
}

describe('EntranceLedger', () => {
  it('animates a fresh message once', () => {
    const ledger = new EntranceLedger()
    expect(ledger.claim(message('m-1-0'), 1_000_500)).toBe(0)
    expect(ledger.claim(message('m-1-0'), 1_000_600)).toBeNull()
  })

  it('mounts history without an entrance', () => {
    const ledger = new EntranceLedger()
    const now = 1_000_000 + entranceFreshMs + 1
    expect(ledger.claim(message('m-1-0'), now)).toBeNull()
    expect(ledger.claim(message('m-1-1', { timestamp: undefined }), now)).toBeNull()
  })

  it('staggers a burst of arrivals and caps the delay', () => {
    const ledger = new EntranceLedger()
    const delays = Array.from({ length: 9 }, (_, index) =>
      ledger.claim(message(`m-1-${index}`), 1_000_000 + index * 10)
    )
    expect(delays.slice(0, 3)).toEqual([0, 70, 140])
    expect(delays.at(-1)).toBe(420)
    expect(ledger.claim(message('m-1-20'), 1_005_000)).toBe(0)
  })

  it('lets an optimistic prompt enter but not its server copy', () => {
    const ledger = new EntranceLedger()
    const local = message('m-1-1000000-1', { role: 'user', text: ' Go ', pending: true })
    expect(ledger.claim(local, 1_000_100)).toBe(0)
    expect(ledger.claim(message('m-1-4', { role: 'user', text: 'Go' }), 1_002_000)).toBeNull()
    // The suppression is spent: the same words sent again later still enter.
    expect(
      ledger.claim(message('m-1-6', { role: 'user', text: 'Go', timestamp: 1_010_000 }), 1_010_500)
    ).toBe(0)
  })
})
