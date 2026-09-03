import { getMessage } from '$lib/i18n'
import { errorToMessage } from '$lib/service-worker/settings'
import type { QuickPrompt } from './types'

export const quickPromptsStorageKey = 'quickPrompts'
export const quickPromptsMaxItems = 20
const quickPromptMaxTextChars = 2_000

/** The slice of `chrome.storage.local` this module owns. */
export interface QuickPromptStorage {
  get(keys: string[]): Promise<{ quickPrompts?: unknown }>
  set(items: Record<string, unknown>): Promise<void>
}

/**
 * The composer's saved prompts, in most-recently-used order.
 *
 * Text is the identity: it is normalized (CRLF folded, trimmed, capped at
 * {@link quickPromptMaxTextChars}) before every comparison, so the same prompt
 * never lands twice. The list is capped at {@link quickPromptsMaxItems}; when it
 * overflows, the least-used entry is evicted rather than the oldest.
 *
 * Writes are serialized through one chain, so concurrent calls cannot clobber
 * each other, and a storage failure leaves `items` untouched and is surfaced
 * through `reportError` instead of thrown.
 */
export class QuickPrompts {
  #storage: QuickPromptStorage
  #reportError: (message: string) => void
  #write: Promise<void> = Promise.resolve()

  items = $state<QuickPrompt[]>([])

  constructor(storage: QuickPromptStorage, reportError: (message: string) => void) {
    this.#storage = storage
    this.#reportError = reportError
  }

  /** Reads the persisted list; call once before rendering the composer. */
  async load(): Promise<void> {
    const saved = await this.#storage.get([quickPromptsStorageKey])
    this.items = normalizeQuickPrompts(saved.quickPrompts)
  }

  has(text: string): boolean {
    const normalized = normalizeQuickPromptText(text)
    return Boolean(normalized && this.items.some((prompt) => prompt.text === normalized))
  }

  async toggle(text: string): Promise<void> {
    if (this.has(text)) {
      await this.remove(text)
      return
    }
    await this.add(text)
  }

  /** Saves a prompt, or refreshes an existing one and moves it to the front. */
  async add(text: string): Promise<void> {
    const normalized = normalizeQuickPromptText(text)
    if (!normalized) {
      return
    }
    await this.#apply((prompts) => {
      const now = Date.now()
      const existing = prompts.find((prompt) => prompt.text === normalized)
      const next: QuickPrompt = existing
        ? { ...existing, updatedAt: now }
        : {
            id: quickPromptId(normalized),
            text: normalized,
            createdAt: now,
            updatedAt: now,
            usedAt: now,
            useCount: 0
          }
      return limitQuickPrompts([next, ...prompts.filter((prompt) => prompt.text !== normalized)])
    })
  }

  /** Records a use, which protects the prompt from eviction. */
  async use(text: string): Promise<void> {
    const normalized = normalizeQuickPromptText(text)
    if (!normalized) {
      return
    }
    await this.#apply((prompts) => {
      const prompt = prompts.find((item) => item.text === normalized)
      if (!prompt) {
        return prompts
      }
      const now = Date.now()
      return limitQuickPrompts([
        { ...prompt, usedAt: now, useCount: prompt.useCount + 1 },
        ...prompts.filter((item) => item.text !== normalized)
      ])
    })
  }

  async remove(text: string): Promise<void> {
    const normalized = normalizeQuickPromptText(text)
    if (!normalized) {
      return
    }
    await this.#apply((prompts) => prompts.filter((prompt) => prompt.text !== normalized))
  }

  async clear(): Promise<void> {
    if (this.items.length === 0) {
      return
    }
    await this.#apply(() => [])
  }

  async #apply(updater: (prompts: QuickPrompt[]) => QuickPrompt[]): Promise<void> {
    try {
      await this.#update(updater)
    } catch (error) {
      this.#reportError(quickPromptsUpdateErrorMessage(error))
    }
  }

  /** Serializes writes so a slow persist cannot be overtaken by a later one. */
  async #update(updater: (prompts: QuickPrompt[]) => QuickPrompt[]): Promise<void> {
    const write = this.#write.then(async () => {
      const current = quickPromptStorageSnapshot(this.items)
      const next = limitQuickPrompts(updater(current))
      if (JSON.stringify(next) === JSON.stringify(current)) {
        return
      }
      await this.#storage.set({ [quickPromptsStorageKey]: quickPromptStorageSnapshot(next) })
      this.items = next
    })
    this.#write = write.catch(() => undefined)
    await write
  }
}

function quickPromptsUpdateErrorMessage(error: unknown): string {
  const detail = errorToMessage(error)
  return (
    getMessage('quickPromptsUpdateFailed', [detail]) || `Could not update quick inputs: ${detail}`
  )
}

function numericTimestamp(value: unknown, fallback: number): number {
  return typeof value === 'number' && Number.isFinite(value) && value >= 0 ? value : fallback
}

/** Text is the prompt's identity, so normalize before every comparison. */
export function normalizeQuickPromptText(value: unknown): string {
  if (typeof value !== 'string') {
    return ''
  }
  return value.replace(/\r\n/g, '\n').trim().slice(0, quickPromptMaxTextChars).trim()
}

/** Stable FNV-1a id so the same text keeps the same key across reloads. */
function quickPromptId(text: string): string {
  let hash = 2166136261
  for (let index = 0; index < text.length; index += 1) {
    hash ^= text.charCodeAt(index)
    hash = Math.imul(hash, 16777619)
  }
  return `qp-${(hash >>> 0).toString(36)}-${text.length.toString(36)}`
}

function sortQuickPromptsForDisplay(items: QuickPrompt[]): QuickPrompt[] {
  return [...items].sort(
    (left, right) =>
      right.usedAt - left.usedAt ||
      right.updatedAt - left.updatedAt ||
      right.useCount - left.useCount ||
      left.text.localeCompare(right.text)
  )
}

/** Caps the list, evicting the least-used entries rather than the oldest. */
function limitQuickPrompts(items: QuickPrompt[]): QuickPrompt[] {
  if (items.length <= quickPromptsMaxItems) {
    return sortQuickPromptsForDisplay(items)
  }
  return sortQuickPromptsForDisplay(
    [...items]
      .sort(
        (left, right) =>
          right.useCount - left.useCount ||
          right.usedAt - left.usedAt ||
          right.updatedAt - left.updatedAt ||
          right.createdAt - left.createdAt
      )
      .slice(0, quickPromptsMaxItems)
  )
}

/** Rebuilds the list from untrusted storage, merging duplicates by text. */
export function normalizeQuickPrompts(value: unknown): QuickPrompt[] {
  if (!Array.isArray(value)) {
    return []
  }
  const byText = new Map<string, QuickPrompt>()
  for (const item of value) {
    const raw = item && typeof item === 'object' ? (item as Record<string, unknown>) : {}
    const text = normalizeQuickPromptText(raw.text)
    if (!text) {
      continue
    }
    const now = Date.now()
    const next: QuickPrompt = {
      id: quickPromptId(text),
      text,
      createdAt: numericTimestamp(raw.createdAt, now),
      updatedAt: numericTimestamp(raw.updatedAt, now),
      usedAt: numericTimestamp(raw.usedAt, 0),
      useCount: Math.max(0, Math.floor(numericTimestamp(raw.useCount, 0)))
    }
    const existing = byText.get(text)
    byText.set(
      text,
      existing
        ? {
            ...next,
            createdAt: Math.min(existing.createdAt, next.createdAt),
            updatedAt: Math.max(existing.updatedAt, next.updatedAt),
            usedAt: Math.max(existing.usedAt, next.usedAt),
            useCount: Math.max(existing.useCount, next.useCount)
          }
        : next
    )
  }
  return limitQuickPrompts(Array.from(byText.values()))
}

function quickPromptStorageSnapshot(items: QuickPrompt[]): QuickPrompt[] {
  return items.map((prompt) => ({
    id: prompt.id,
    text: prompt.text,
    createdAt: prompt.createdAt,
    updatedAt: prompt.updatedAt,
    usedAt: prompt.usedAt,
    useCount: prompt.useCount
  }))
}
