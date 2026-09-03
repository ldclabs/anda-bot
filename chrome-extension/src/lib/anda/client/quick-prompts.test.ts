import { afterEach, describe, expect, it, vi } from 'vitest'
import {
  QuickPrompts,
  normalizeQuickPrompts,
  quickPromptsMaxItems,
  type QuickPromptStorage
} from './quick-prompts.svelte'
import type { QuickPrompt } from './types'

/**
 * `QuickPrompts` takes a storage port, so these tests use a plain in-memory
 * object instead of a fake `chrome` global.
 */
function createStorage(initial: unknown = undefined, setError?: Error) {
  const state: Record<string, unknown> = { quickPrompts: initial }
  const set = vi.fn(async (items: Record<string, unknown>) => {
    if (setError) {
      throw setError
    }
    Object.assign(state, items)
  })
  const storage: QuickPromptStorage = {
    get: vi.fn(async (keys: string[]) => {
      const result: Record<string, unknown> = {}
      for (const key of keys) {
        if (key in state) {
          result[key] = state[key]
        }
      }
      return result
    }),
    set
  }
  return { storage, set, state }
}

function createPrompts(initial: unknown = undefined, setError?: Error) {
  const { storage, set, state } = createStorage(initial, setError)
  const reportError = vi.fn()
  return { prompts: new QuickPrompts(storage, reportError), set, state, reportError }
}

afterEach(() => {
  vi.useRealTimers()
})

describe('normalizeQuickPrompts', () => {
  it('trims text and drops entries that normalize to nothing', () => {
    const items = normalizeQuickPrompts([
      { id: 'old', text: '  提交变更  ', createdAt: 1, updatedAt: 2, usedAt: 3, useCount: 4 },
      { id: 'object', text: { bad: true }, createdAt: 1, updatedAt: 1, usedAt: 0, useCount: 0 },
      { id: 'blank', text: '   ', createdAt: 1, updatedAt: 1, usedAt: 0, useCount: 0 }
    ])

    expect(items).toHaveLength(1)
    expect(items[0]).toMatchObject({ text: '提交变更', useCount: 4 })
  })

  it('merges duplicates by text, keeping the strongest counters', () => {
    const items = normalizeQuickPrompts([
      { text: 'ship it', createdAt: 5, updatedAt: 5, usedAt: 5, useCount: 1 },
      { text: '  ship it  ', createdAt: 2, updatedAt: 9, usedAt: 9, useCount: 7 }
    ])

    expect(items).toHaveLength(1)
    expect(items[0]).toMatchObject({ createdAt: 2, updatedAt: 9, usedAt: 9, useCount: 7 })
  })

  it('treats non-array storage as empty', () => {
    expect(normalizeQuickPrompts(undefined)).toEqual([])
    expect(normalizeQuickPrompts('nope')).toEqual([])
  })
})

describe('QuickPrompts', () => {
  it('loads, adds, uses, removes, and clears prompts', async () => {
    vi.useFakeTimers()
    vi.setSystemTime(new Date('2026-06-19T00:00:00Z'))
    const { prompts, set, state } = createPrompts([
      { id: 'old', text: '  提交变更  ', createdAt: 1, updatedAt: 2, usedAt: 3, useCount: 4 }
    ])

    await prompts.load()

    expect(prompts.items).toHaveLength(1)
    expect(prompts.has('提交变更')).toBe(true)

    await prompts.add('写测试')
    expect(prompts.has('写测试')).toBe(true)
    expect(set).toHaveBeenLastCalledWith({
      quickPrompts: expect.arrayContaining([expect.objectContaining({ text: '写测试' })])
    })

    // A second reader sees both prompts through the shared storage.
    const reloaded = new QuickPrompts(
      { get: async () => ({ quickPrompts: state.quickPrompts }), set: async () => undefined },
      () => undefined
    )
    await reloaded.load()
    expect(reloaded.has('提交变更')).toBe(true)
    expect(reloaded.has('写测试')).toBe(true)

    await prompts.use('提交变更')
    expect(prompts.items.find((prompt) => prompt.text === '提交变更')).toMatchObject({
      useCount: 5
    })

    await prompts.remove('写测试')
    expect(prompts.has('写测试')).toBe(false)

    await prompts.clear()
    expect(prompts.items).toEqual([])
    expect(set).toHaveBeenLastCalledWith({ quickPrompts: [] })
  })

  it('evicts the least-used prompt when the limit is exceeded', async () => {
    vi.useFakeTimers()
    vi.setSystemTime(new Date('2026-06-19T00:00:00Z'))
    const { prompts } = createPrompts(
      Array.from({ length: quickPromptsMaxItems }, (_, index) => ({
        id: `prompt-${index}`,
        text: `prompt ${index}`,
        createdAt: index,
        updatedAt: index,
        usedAt: index,
        useCount: index === 0 ? 0 : 1
      })) satisfies QuickPrompt[]
    )
    await prompts.load()

    await prompts.add('new prompt')

    expect(prompts.items).toHaveLength(quickPromptsMaxItems)
    expect(prompts.items.some((prompt) => prompt.text === 'prompt 0')).toBe(false)
    expect(prompts.items.some((prompt) => prompt.text === 'new prompt')).toBe(true)
  })

  it('keeps the list unchanged and reports when the write fails', async () => {
    const { prompts, reportError } = createPrompts(undefined, new Error('storage unavailable'))

    await prompts.add('提交变更')

    expect(prompts.items).toEqual([])
    expect(prompts.has('提交变更')).toBe(false)
    // No extension i18n table here, so getMessage falls back to English.
    expect(reportError).toHaveBeenCalledWith('Could not update quick inputs: storage unavailable')
  })

  it('uses the localized failure message when the i18n table is available', async () => {
    vi.stubGlobal('chrome', {
      i18n: {
        getMessage: (key: string, substitutions?: string[]) =>
          substitutions?.length ? `${key}:${substitutions.join(',')}` : key
      }
    })
    const { prompts, reportError } = createPrompts(undefined, new Error('storage unavailable'))

    await prompts.add('提交变更')

    expect(reportError).toHaveBeenCalledWith('quickPromptsUpdateFailed:storage unavailable')
    vi.unstubAllGlobals()
  })

  it('ignores blank text and unknown prompts', async () => {
    const { prompts, set } = createPrompts()

    await prompts.add('   ')
    await prompts.use('never saved')
    await prompts.remove('  ')
    await prompts.clear()

    expect(prompts.items).toEqual([])
    expect(set).not.toHaveBeenCalled()
  })

  it('toggles a prompt on and back off', async () => {
    const { prompts } = createPrompts()

    await prompts.toggle('ship it')
    expect(prompts.has('ship it')).toBe(true)

    await prompts.toggle('  ship it  ')
    expect(prompts.has('ship it')).toBe(false)
  })

  it('serializes concurrent writes rather than clobbering', async () => {
    const { prompts } = createPrompts()

    await Promise.all([prompts.add('first'), prompts.add('second'), prompts.add('third')])

    expect(prompts.items.map((prompt) => prompt.text).sort()).toEqual(['first', 'second', 'third'])
  })
})
