import type { BrowserActionArgs, ChromeApi, ChromeTabInfo } from './types'

/**
 * Tab resolution and the argument coercions every browser action shares.
 *
 * Chrome's active-tab queries are unreliable while the side panel has focus —
 * the panel itself can answer as the active tab — so the last tab Anda acted on
 * is remembered here and preferred, with the queries as fallbacks. The service
 * worker keeps that memory fresh through its tab and `webNavigation` listeners.
 */

/** How long an action waits for a page to become ready, unless it says otherwise. */
export const DEFAULT_PAGE_READY_TIMEOUT_MS = 30_000

let rememberedActiveTabId: number | null = null

export function actionTimeoutMs(
  args: BrowserActionArgs,
  defaultTimeout = DEFAULT_PAGE_READY_TIMEOUT_MS
): number {
  const requested =
    typeof args.timeout_ms === 'number' && Number.isFinite(args.timeout_ms)
      ? args.timeout_ms
      : defaultTimeout
  return Math.max(100, Math.min(120000, Math.floor(requested)))
}

export function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error)
}

export function rememberActiveTab(tabOrId: ChromeTabInfo | number | null | undefined): void {
  if (tabOrId == null) {
    rememberedActiveTabId = null
    return
  }
  const tabId = typeof tabOrId === 'number' ? tabOrId : tabOrId?.id
  if (typeof tabId === 'number' && Number.isFinite(tabId)) {
    rememberedActiveTabId = tabId
  }
}

export function forgetActiveTab(tabId: number): void {
  if (rememberedActiveTabId === tabId) {
    rememberedActiveTabId = null
  }
}

async function rememberedActiveTab(chromeApi: ChromeApi): Promise<ChromeTabInfo | null> {
  if (!rememberedActiveTabId) {
    return null
  }
  try {
    const tab = await chromeApi.tabs.get(rememberedActiveTabId)
    if (tab?.active) {
      return tab
    }
  } catch (_error) {
    // The remembered tab may have been closed; fall back to Chrome's active-tab queries.
  }
  rememberedActiveTabId = null
  return null
}

export async function activeTab(chromeApi: ChromeApi): Promise<ChromeTabInfo | null> {
  const remembered = await rememberedActiveTab(chromeApi)
  if (remembered) {
    return remembered
  }

  const [tab] = await chromeApi.tabs.query({ active: true, lastFocusedWindow: true })
  if (tab) {
    rememberActiveTab(tab)
    return tab
  }
  const [currentWindowTab] = await chromeApi.tabs.query({ active: true, currentWindow: true })
  if (currentWindowTab) {
    rememberActiveTab(currentWindowTab)
    return currentWindowTab
  }
  const [anyActiveTab] = await chromeApi.tabs.query({ active: true })
  if (anyActiveTab) {
    rememberActiveTab(anyActiveTab)
    return anyActiveTab
  }
  const [fallbackTab] = await chromeApi.tabs.query({})
  return fallbackTab || null
}

export async function activateTab(chromeApi: ChromeApi, tabId: number): Promise<ChromeTabInfo> {
  const tab = (await chromeApi.tabs.update(tabId, { active: true })) as ChromeTabInfo
  await focusWindow(chromeApi, tab.windowId).catch(() => undefined)
  rememberActiveTab(tab)
  return tab
}

export async function focusWindow(chromeApi: ChromeApi, windowId?: number): Promise<void> {
  if (typeof windowId === 'number' && chromeApi.windows?.update) {
    await chromeApi.windows.update(windowId, { focused: true })
  }
}

export function tabSummary(tab: ChromeTabInfo | null | undefined): Record<string, unknown> | null {
  if (!tab) {
    return null
  }
  return {
    id: tab.id,
    window_id: tab.windowId,
    index: tab.index,
    active: tab.active,
    highlighted: tab.highlighted,
    pinned: tab.pinned,
    status: tab.status,
    title: tab.title || '',
    url: tab.url || ''
  }
}

export function positiveInteger(value: unknown): number | null {
  return typeof value === 'number' && Number.isInteger(value) && value > 0 ? value : null
}

export function nonNegativeInteger(value: unknown): number | null {
  return typeof value === 'number' && Number.isInteger(value) && value >= 0 ? value : null
}

export function requirePositiveInteger(value: unknown, message: string): number {
  const integer = positiveInteger(value)
  if (!integer) {
    throw new Error(message)
  }
  return integer
}

export function normalizeOptionalText(value: unknown): string | undefined {
  return typeof value === 'string' && value.trim() ? value.trim() : undefined
}
