import { describe, expect, it, vi } from 'vitest'
import { ResourceCache } from './resources'
import type { DaemonApi } from './daemon'

const summary = (id: number) => ({ _id: id, name: String(id), tags: [] })
const full = (id: number) => ({ ...summary(id), blob: 'AAAA' })

describe('ResourceCache', () => {
  it('deduplicates reads and evicts the least recently used bytes', async () => {
    const toolCall = vi.fn(async (_name, args) => ({ output: { result: full(args._id) } }))
    const cache = new ResourceCache({ toolCall } as unknown as DaemonApi, 16)
    await Promise.all([cache.load(summary(1)), cache.load(summary(1))])
    await cache.load(summary(2))
    await cache.load(summary(1))
    await cache.load(summary(3))
    await cache.load(summary(2))
    expect(toolCall.mock.calls.map(([, args]) => args._id)).toEqual([1, 2, 3, 2])
  })

  it('limits concurrency and drops requests from a previous connection', async () => {
    const complete: Array<() => void> = []
    const toolCall = vi.fn(
      (_name, args) =>
        new Promise((resolve) => {
          complete.push(() => resolve({ output: { result: full(args._id) } }))
        })
    )
    const cache = new ResourceCache({ toolCall } as unknown as DaemonApi)
    const requests = Array.from({ length: 6 }, (_, id) => cache.load(summary(id + 1)))
    const results = Promise.allSettled(requests)
    expect(toolCall).toHaveBeenCalledTimes(4)
    cache.clear()
    complete.forEach((finish) => finish())
    expect((await results).every((result) => result.status === 'rejected')).toBe(true)
    expect(toolCall).toHaveBeenCalledTimes(4)
    const next = cache.load(summary(1))
    complete.at(-1)!()
    await expect(next).resolves.toMatchObject(full(1))
  })
})
