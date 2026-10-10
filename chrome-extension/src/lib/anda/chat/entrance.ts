import type { ChatMessage } from '../client/types'

/** A message older than this mounts in place: it is history, not news. */
export const entranceFreshMs = 30_000
/** Items that mount within this window of each other enter one after another. */
const burstWindowMs = 90
const burstStepMs = 70
const maxBurstSteps = 6
/** An optimistic prompt's server copy re-keys the row; it arrives without a second entrance. */
const optimisticCopyMs = 120_000
const maxRemembered = 4000

/**
 * Decides which transcript items animate in. Only a message that just arrived
 * gets an entrance, once: loading history, switching chats, the poll loop
 * re-rendering a turn and an optimistic prompt being replaced by its server
 * copy (a new id with the same text) all mount rows without one.
 */
export class EntranceLedger {
  #seen = new Set<string>()
  #optimistic = new Map<string, number>()
  #burstAt = Number.NEGATIVE_INFINITY
  #burst = 0

  /** The entrance delay in ms for a message mounting now, or null for none. */
  claim(message: ChatMessage, now = Date.now()): number | null {
    if (this.#seen.has(message.id)) {
      return null
    }
    this.#remember(message.id)

    const signature = `${message.role}\u0000${message.text.trim()}`
    if (message.pending) {
      this.#optimistic.set(signature, now)
    } else {
      const optimisticAt = this.#optimistic.get(signature)
      if (optimisticAt !== undefined) {
        this.#optimistic.delete(signature)
        if (now - optimisticAt < optimisticCopyMs) {
          return null
        }
      }
    }

    const timestamp = message.timestamp || 0
    if (!timestamp || Math.abs(now - timestamp) > entranceFreshMs) {
      return null
    }

    this.#burst = now - this.#burstAt < burstWindowMs ? this.#burst + 1 : 0
    this.#burstAt = now
    return Math.min(this.#burst, maxBurstSteps) * burstStepMs
  }

  #remember(id: string) {
    if (this.#seen.size >= maxRemembered) {
      // Sets iterate in insertion order: forget the oldest half.
      let drop = maxRemembered / 2
      for (const seen of this.#seen) {
        if (drop-- <= 0) break
        this.#seen.delete(seen)
      }
    }
    this.#seen.add(id)
  }
}

/** The one ledger every transcript shares, so a remount never replays an entrance. */
export const transcriptEntrances = new EntranceLedger()

export function prefersReducedMotion(): boolean {
  return typeof matchMedia === 'function' && matchMedia('(prefers-reduced-motion: reduce)').matches
}
