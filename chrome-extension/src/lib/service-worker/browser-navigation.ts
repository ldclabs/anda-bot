import { waitForNetworkIdle } from './browser-debugger'
import {
  DEFAULT_PAGE_READY_TIMEOUT_MS,
  actionTimeoutMs,
  errorMessage,
  nonNegativeInteger,
  normalizeOptionalText,
  tabSummary
} from './browser-tabs'
import type {
  BrowserActionArgs,
  BrowserActionResult,
  ChromeApi,
  ChromeTabInfo,
  ChromeWebNavigationDetails
} from './types'

/**
 * Waiting for a page to settle after an action, and describing what it settled
 * into.
 *
 * A watcher is armed *before* the action runs, because a navigation can commit
 * before the action's own promise resolves. `webNavigation` events are preferred
 * when the permission is granted — they distinguish a same-document history
 * change from a real load — with `tabs.onUpdated` as the fallback. Either way
 * the wait is bounded and best effort: a page that never goes idle yields a
 * `page_ready` block saying so rather than failing the action.
 */

const ACTION_SETTLE_NO_LOAD_TIMEOUT_MS = 1_000
const SCRIPT_NAVIGATION_SETTLE_NO_LOAD_TIMEOUT_MS = 2_500

/** How far into a load an action is willing to wait before answering. */
export type NavigationWaitUntil = 'committed' | 'domcontentloaded' | 'complete' | 'history_change'

export type NavigationEventName =
  | 'before_navigate'
  | 'committed'
  | 'dom_content_loaded'
  | 'completed'
  | 'error_occurred'
  | 'history_state_updated'
  | 'reference_fragment_updated'

export function shouldWaitAfterAction(args: BrowserActionArgs): boolean {
  const action = args.action
  return (
    action === 'click' ||
    action === 'type_text' ||
    action === 'press_key' ||
    action === 'scroll' ||
    action === 'scroll_to' ||
    action === 'drag_and_drop' ||
    action === 'select_dropdown' ||
    action === 'upload_file' ||
    scriptMayTriggerPageChange(args)
  )
}

function scriptMayTriggerPageChange(args: BrowserActionArgs): boolean {
  if (args.action !== 'execute_javascript') {
    return false
  }
  const code = normalizeOptionalText(args.code)
  if (!code) {
    return false
  }
  return [
    /\b(?:window\.|document\.)?location\s*=/i,
    /\b(?:window\.|document\.)?location\.(?:href|hash|search|pathname)\s*=/i,
    /\b(?:window\.|document\.)?location\.(?:assign|replace|reload)\s*\(/i,
    /\b(?:window\.)?history\.(?:pushstate|replacestate|back|forward|go)\s*\(/i,
    /\bwindow\.open\s*\(/i,
    /\.submit\s*\(/i
  ].some((pattern) => pattern.test(code))
}

export function withPageReady(
  result: BrowserActionResult,
  pageReady: Record<string, unknown> | null
): BrowserActionResult {
  if (!pageReady) {
    return result
  }
  const tab = pageReadyTab(pageReady)
  const compactPageReady = compactPageReadyInfo(pageReady, { omitTab: true })
  if (result && typeof result === 'object' && !Array.isArray(result)) {
    const output: Record<string, unknown> = {
      ...(result as Record<string, unknown>),
      page_ready: compactPageReady
    }
    if (tab && !('tab' in output)) {
      output.tab = tab
    }
    return output
  }
  return { value: result, tab, page_ready: compactPageReady }
}

export function withTopLevelTab(
  result: Record<string, unknown>,
  pageReady: Record<string, unknown> | null,
  fallbackTab?: ChromeTabInfo | null
): BrowserActionResult {
  return {
    ...result,
    tab: pageReadyTab(pageReady) || tabSummary(fallbackTab),
    page_ready: compactPageReadyInfo(pageReady, { omitTab: true })
  }
}

function pageReadyTab(pageReady: Record<string, unknown> | null): Record<string, unknown> | null {
  if (!pageReady || typeof pageReady.tab !== 'object' || Array.isArray(pageReady.tab)) {
    return null
  }
  return pageReady.tab as Record<string, unknown>
}

function compactPageReadyInfo(
  pageReady: Record<string, unknown> | null,
  options: { omitTab?: boolean } = {}
): Record<string, unknown> | null {
  if (!pageReady) {
    return null
  }
  const compact = { ...pageReady }
  if (compact.load && typeof compact.load === 'object' && !Array.isArray(compact.load)) {
    const { tab: _tab, ...load } = compact.load as Record<string, unknown>
    compact.load = load
  }
  if (options.omitTab) {
    delete compact.tab
  }
  return compact
}

export type TabLoadWatcher = {
  wait(options?: { noLoadTimeoutMs?: number }): Promise<Record<string, unknown>>
  cancel(): void
}

export function createTabLoadWatcher(
  chromeApi: ChromeApi,
  tabId: number,
  timeout: number,
  args: BrowserActionArgs = {}
): TabLoadWatcher {
  if (hasWebNavigationWaitSupport(chromeApi)) {
    return createWebNavigationWaiter(
      chromeApi,
      tabId,
      { ...args, timeout_ms: timeout },
      { waitUntil: 'complete', allowAlreadyComplete: true }
    )
  }
  return createTabsLoadWatcher(chromeApi, tabId, timeout)
}

function hasWebNavigationWaitSupport(chromeApi: ChromeApi): boolean {
  const webNavigation = chromeApi.webNavigation
  return Boolean(
    webNavigation?.onCompleted &&
    webNavigation.onErrorOccurred &&
    (webNavigation.onCommitted || webNavigation.onBeforeNavigate)
  )
}

function createWebNavigationWaiter(
  chromeApi: ChromeApi,
  tabId: number,
  args: BrowserActionArgs,
  options: { waitUntil: NavigationWaitUntil; allowAlreadyComplete?: boolean }
): TabLoadWatcher {
  const webNavigation = chromeApi.webNavigation
  if (!webNavigation) {
    throw new Error('Chrome webNavigation API is unavailable; enable webNavigation permission')
  }

  const timeout = actionTimeoutMs(args)
  const frameId = nonNegativeInteger(args.frame_id) ?? 0
  const expectedUrl = navigationExpectedUrl(args)
  let done = false
  let sawLoading = false
  let sawCommitted = false
  let sawDomContentLoaded = false
  let sawComplete = false
  let sawHistoryChange = false
  let lastDetails: ChromeWebNavigationDetails | null = null
  let lastTab: ChromeTabInfo | null = null
  let noLoadTimer: ReturnType<typeof setTimeout> | null = null
  let resolveWait: (value: Record<string, unknown>) => void
  let rejectWait: (reason: Error) => void

  const wait = new Promise<Record<string, unknown>>((resolve, reject) => {
    resolveWait = resolve
    rejectWait = reject
  })
  // The action can still be running when a navigation fails; attach a handler
  // now, while preserving the rejection for wait().
  void wait.catch(() => undefined)

  const cleanup = () => {
    if (done) {
      return
    }
    done = true
    clearTimeout(timer)
    if (noLoadTimer) {
      clearTimeout(noLoadTimer)
      noLoadTimer = null
    }
    webNavigation.onBeforeNavigate?.removeListener(onBeforeNavigate)
    webNavigation.onCommitted?.removeListener(onCommitted)
    webNavigation.onDOMContentLoaded?.removeListener(onDOMContentLoaded)
    webNavigation.onCompleted?.removeListener(onCompleted)
    webNavigation.onErrorOccurred?.removeListener(onErrorOccurred)
    webNavigation.onHistoryStateUpdated?.removeListener(onHistoryStateUpdated)
    webNavigation.onReferenceFragmentUpdated?.removeListener(onReferenceFragmentUpdated)
  }

  const finish = (
    details: ChromeWebNavigationDetails | null,
    event: NavigationEventName | 'already_complete' | 'no_load_detected',
    extra: Record<string, unknown> = {}
  ) => {
    if (done) {
      return
    }
    lastDetails = details || lastDetails
    const alreadyComplete = event === 'already_complete'
    cleanup()
    void chromeApi.tabs
      .get(tabId)
      .catch(() => lastTab)
      .then((tab) => {
        if (tab) {
          lastTab = tab
        }
        resolveWait({
          loaded: extra.loaded ?? true,
          source: 'web_navigation',
          event,
          wait_until: options.waitUntil,
          saw_loading: sawLoading,
          saw_committed: sawCommitted,
          saw_dom_content_loaded: sawDomContentLoaded,
          saw_complete: sawComplete,
          saw_history_change: sawHistoryChange,
          already_complete: alreadyComplete,
          navigation: navigationDetailsSummary(lastDetails, event),
          tab: tabSummary(tab || lastTab),
          ...extra
        })
      })
  }

  const fail = (details: ChromeWebNavigationDetails | null, message: string) => {
    if (done) {
      return
    }
    lastDetails = details || lastDetails
    cleanup()
    rejectWait(new Error(message))
  }

  const record = (details: ChromeWebNavigationDetails, event: NavigationEventName) => {
    if (!matchesNavigation(details, tabId, frameId)) {
      return false
    }
    lastDetails = details
    if (event === 'before_navigate') {
      sawLoading = true
    } else if (event === 'committed') {
      sawCommitted = true
    } else if (event === 'dom_content_loaded') {
      sawDomContentLoaded = true
    } else if (event === 'completed') {
      sawComplete = true
    } else if (event === 'history_state_updated' || event === 'reference_fragment_updated') {
      sawHistoryChange = true
    }
    return true
  }

  const maybeFinish = (details: ChromeWebNavigationDetails, event: NavigationEventName) => {
    if (!record(details, event)) {
      return
    }
    if (navigationEventSatisfiesWait(event, options.waitUntil)) {
      finish(details, event, {
        same_document: event === 'history_state_updated' || event === 'reference_fragment_updated'
      })
    }
  }

  const onBeforeNavigate = (details: ChromeWebNavigationDetails) => {
    record(details, 'before_navigate')
  }
  const onCommitted = (details: ChromeWebNavigationDetails) => {
    maybeFinish(details, 'committed')
  }
  const onDOMContentLoaded = (details: ChromeWebNavigationDetails) => {
    maybeFinish(details, 'dom_content_loaded')
  }
  const onCompleted = (details: ChromeWebNavigationDetails) => {
    maybeFinish(details, 'completed')
  }
  const onErrorOccurred = (details: ChromeWebNavigationDetails) => {
    if (!matchesNavigation(details, tabId, frameId)) {
      return
    }
    fail(
      details,
      `navigation failed${details.error ? `: ${details.error}` : ''}${
        details.url ? ` (${details.url})` : ''
      }`
    )
  }
  const onHistoryStateUpdated = (details: ChromeWebNavigationDetails) => {
    maybeFinish(details, 'history_state_updated')
  }
  const onReferenceFragmentUpdated = (details: ChromeWebNavigationDetails) => {
    maybeFinish(details, 'reference_fragment_updated')
  }

  const timer = setTimeout(() => {
    fail(lastDetails, `navigation did not reach ${options.waitUntil} before timeout: ${timeout}ms`)
  }, timeout)

  webNavigation.onBeforeNavigate?.addListener(onBeforeNavigate)
  webNavigation.onCommitted?.addListener(onCommitted)
  webNavigation.onDOMContentLoaded?.addListener(onDOMContentLoaded)
  webNavigation.onCompleted?.addListener(onCompleted)
  webNavigation.onErrorOccurred?.addListener(onErrorOccurred)
  webNavigation.onHistoryStateUpdated?.addListener(onHistoryStateUpdated)
  webNavigation.onReferenceFragmentUpdated?.addListener(onReferenceFragmentUpdated)

  return {
    async wait(waitOptions?: { noLoadTimeoutMs?: number }) {
      const current = await chromeApi.tabs.get(tabId).catch(() => null)
      if (current) {
        lastTab = current
      }
      const currentMatches =
        sawLoading ||
        sawCommitted ||
        args.action === 'open_tab' ||
        urlMatchesExpected(current?.url || '', expectedUrl)
      if (
        options.allowAlreadyComplete &&
        currentMatches &&
        (current?.status === 'complete' || isInstantLoadUrl(current?.url))
      ) {
        finish(navigationDetailsFromTab(current, tabId, frameId), 'already_complete')
      }
      if (done) {
        return wait
      }
      const noLoadTimeoutMs = waitOptions?.noLoadTimeoutMs
      if (noLoadTimeoutMs && noLoadTimeoutMs > 0) {
        noLoadTimer = setTimeout(async () => {
          if (done || sawLoading || sawCommitted || sawComplete || sawHistoryChange) {
            return
          }
          const latest = await chromeApi.tabs.get(tabId).catch(() => lastTab)
          if (latest) {
            lastTab = latest
          }
          finish(navigationDetailsFromTab(latest || current, tabId, frameId), 'no_load_detected', {
            loaded: false,
            no_load_detected: true
          })
        }, noLoadTimeoutMs)
      }
      return wait
    },
    cancel() {
      cleanup()
    }
  }
}

async function waitForWebNavigation(
  chromeApi: ChromeApi,
  tabId: number,
  args: BrowserActionArgs,
  options: { waitUntil: NavigationWaitUntil; allowAlreadyComplete?: boolean }
): Promise<Record<string, unknown>> {
  const watcher = createWebNavigationWaiter(chromeApi, tabId, args, options)
  try {
    return await watcher.wait()
  } catch (error) {
    watcher.cancel()
    throw error
  }
}

function createTabsLoadWatcher(
  chromeApi: ChromeApi,
  tabId: number,
  timeout: number
): TabLoadWatcher {
  let done = false
  let sawLoading = false
  let sawComplete = false
  let lastTab: ChromeTabInfo | null = null
  let resolveWait: (value: Record<string, unknown>) => void
  let rejectWait: (reason: Error) => void
  let noLoadTimer: ReturnType<typeof setTimeout> | null = null

  const wait = new Promise<Record<string, unknown>>((resolve, reject) => {
    resolveWait = resolve
    rejectWait = reject
  })
  // The action can still be running when a navigation fails; attach a handler
  // now, while preserving the rejection for wait().
  void wait.catch(() => undefined)

  const cleanup = () => {
    if (done) {
      return
    }
    done = true
    clearTimeout(timer)
    if (noLoadTimer) {
      clearTimeout(noLoadTimer)
      noLoadTimer = null
    }
    chromeApi.tabs.onUpdated.removeListener(listener)
  }

  const finish = (
    tab: ChromeTabInfo | null,
    alreadyComplete: boolean,
    extra: Record<string, unknown> = {}
  ) => {
    if (done) {
      return
    }
    cleanup()
    resolveWait({
      loaded: extra.loaded ?? true,
      saw_loading: sawLoading,
      saw_complete: sawComplete,
      already_complete: alreadyComplete,
      tab: tabSummary(tab || lastTab),
      ...extra
    })
  }

  const fail = (message: string) => {
    if (done) {
      return
    }
    cleanup()
    rejectWait(new Error(message))
  }

  const listener = (
    updatedTabId: number,
    changeInfo: { title?: string; url?: string; status?: string },
    tab: ChromeTabInfo
  ) => {
    if (updatedTabId !== tabId) {
      return
    }
    lastTab = tab
    if (changeInfo.status === 'loading' || tab.status === 'loading') {
      sawLoading = true
    }
    if (changeInfo.status === 'complete' || tab.status === 'complete') {
      sawComplete = true
      finish(tab, false)
    }
  }

  const timer = setTimeout(() => {
    fail(`tab did not finish loading before timeout: ${timeout}ms`)
  }, timeout)

  chromeApi.tabs.onUpdated.addListener(listener)

  return {
    async wait(options?: { noLoadTimeoutMs?: number }) {
      const current = await chromeApi.tabs.get(tabId).catch(() => null)
      if (current) {
        lastTab = current
      }
      if (current?.status === 'complete' || isInstantLoadUrl(current?.url)) {
        finish(current, !sawLoading && !sawComplete)
      }
      if (done) {
        return wait
      }
      const noLoadTimeoutMs = options?.noLoadTimeoutMs
      if (noLoadTimeoutMs && noLoadTimeoutMs > 0) {
        noLoadTimer = setTimeout(async () => {
          if (done || sawLoading || sawComplete) {
            return
          }
          const latest = await chromeApi.tabs.get(tabId).catch(() => lastTab)
          if (latest) {
            lastTab = latest
          }
          finish(latest || current, false, { loaded: false, no_load_detected: true })
        }, noLoadTimeoutMs)
      }
      return wait
    },
    cancel() {
      cleanup()
    }
  }
}

function isInstantLoadUrl(url?: string): boolean {
  const normalized = normalizeOptionalText(url)?.toLowerCase() || ''
  return (
    normalized.startsWith('data:') ||
    normalized.startsWith('about:') ||
    normalized.startsWith('blob:') ||
    normalized.startsWith('chrome:') ||
    normalized.startsWith('chrome-extension:')
  )
}

function navigationExpectedUrl(args: BrowserActionArgs): string | undefined {
  if (args.action === 'navigate' || args.action === 'open_tab') {
    return normalizeOptionalText(args.url)
  }
  return undefined
}

function matchesNavigation(
  details: ChromeWebNavigationDetails,
  tabId: number,
  frameId: number,
  expectedUrl?: string
): boolean {
  return (
    details.tabId === tabId &&
    details.frameId === frameId &&
    urlMatchesExpected(details.url || '', expectedUrl)
  )
}

function urlMatchesExpected(actualUrl: string, expectedUrl?: string): boolean {
  const expected = normalizeOptionalText(expectedUrl)
  if (!expected) {
    return true
  }
  if (expected.includes('*')) {
    const pattern = `^${expected
      .split('*')
      .map((part) => part.replace(/[.*+?^${}()|[\]\\]/g, '\\$&'))
      .join('.*')}$`
    return new RegExp(pattern).test(actualUrl)
  }
  return actualUrl === expected || actualUrl.startsWith(expected) || actualUrl.includes(expected)
}

function navigationEventSatisfiesWait(
  event: NavigationEventName,
  waitUntil: NavigationWaitUntil
): boolean {
  if (event === 'history_state_updated' || event === 'reference_fragment_updated') {
    return waitUntil === 'history_change' || waitUntil === 'complete'
  }
  if (waitUntil === 'committed') {
    return event === 'committed' || event === 'dom_content_loaded' || event === 'completed'
  }
  if (waitUntil === 'domcontentloaded') {
    return event === 'dom_content_loaded' || event === 'completed'
  }
  return waitUntil === 'complete' && event === 'completed'
}

function navigationDetailsSummary(
  details: ChromeWebNavigationDetails | null,
  event: string
): Record<string, unknown> | null {
  if (!details) {
    return null
  }
  return {
    event,
    tab_id: details.tabId,
    frame_id: details.frameId,
    parent_frame_id: details.parentFrameId ?? null,
    process_id: details.processId ?? null,
    url: details.url || '',
    error: details.error || null,
    transition_type: details.transitionType || null,
    transition_qualifiers: details.transitionQualifiers || [],
    time_stamp: details.timeStamp ?? null
  }
}

function navigationDetailsFromTab(
  tab: ChromeTabInfo | null,
  tabId: number,
  frameId: number
): ChromeWebNavigationDetails | null {
  const url = normalizeOptionalText(tab?.url)
  if (!url) {
    return null
  }
  return { tabId, frameId, url }
}

export async function waitForTabReady(
  chromeApi: ChromeApi,
  tabId: number,
  args: BrowserActionArgs,
  loadWatcher = createTabLoadWatcher(chromeApi, tabId, actionTimeoutMs(args), args),
  waitOptions?: { noLoadTimeoutMs?: number; networkIdleTimeoutMs?: number }
): Promise<Record<string, unknown>> {
  try {
    const load = await loadWatcher.wait(waitOptions)
    const loaded = load.loaded !== false
    const network = loaded
      ? await waitForNetworkIdleBestEffort(
          chromeApi,
          tabId,
          args,
          waitOptions?.networkIdleTimeoutMs
        )
      : { skipped: true, reason: 'no page load detected' }
    const tab = await chromeApi.tabs.get(tabId).catch(() => null)
    return {
      loaded,
      load: compactPageReadyInfo(load, { omitTab: true }) || load,
      network_idle: network,
      tab: tabSummary(tab) || load.tab || null
    }
  } catch (error) {
    loadWatcher.cancel()
    throw error
  }
}

export async function waitForTabReadyIfLoading(
  chromeApi: ChromeApi,
  tabId: number,
  args: BrowserActionArgs,
  options?: { bestEffort?: boolean; timeoutMs?: number; noLoadTimeoutMs?: number }
): Promise<Record<string, unknown> | null> {
  const current = await chromeApi.tabs.get(tabId).catch(() => null)
  if (current?.status !== 'loading') {
    return null
  }
  const timeout = Math.min(actionTimeoutMs(args), options?.timeoutMs || actionTimeoutMs(args))
  const watcher = createTabLoadWatcher(chromeApi, tabId, timeout, args)
  try {
    return await waitForTabReady(chromeApi, tabId, args, watcher, {
      noLoadTimeoutMs: options?.noLoadTimeoutMs,
      networkIdleTimeoutMs: options?.timeoutMs
    })
  } catch (error) {
    if (options?.bestEffort) {
      return { skipped: true, error: errorMessage(error), tab: tabSummary(current) }
    }
    throw error
  }
}

async function waitForNetworkIdleBestEffort(
  chromeApi: ChromeApi,
  tabId: number,
  args: BrowserActionArgs,
  timeoutMs?: number
): Promise<Record<string, unknown>> {
  if (!chromeApi.debugger?.onEvent) {
    return { skipped: true, reason: 'debugger event API unavailable' }
  }
  try {
    return (await waitForNetworkIdle(chromeApi, tabId, args, timeoutMs)) as Record<string, unknown>
  } catch (error) {
    return { skipped: true, error: errorMessage(error) }
  }
}

export async function waitForPageSettleAfterAction(
  chromeApi: ChromeApi,
  tabId: number,
  args: BrowserActionArgs,
  loadWatcher: TabLoadWatcher | null
): Promise<Record<string, unknown> | null> {
  if (!loadWatcher) {
    return null
  }
  try {
    return await waitForTabReady(chromeApi, tabId, args, loadWatcher, {
      noLoadTimeoutMs: postActionNoLoadTimeoutMs(args)
    })
  } catch (error) {
    return { settled: false, error: errorMessage(error) }
  }
}

function postActionNoLoadTimeoutMs(args: BrowserActionArgs): number {
  return args.action === 'execute_javascript'
    ? SCRIPT_NAVIGATION_SETTLE_NO_LOAD_TIMEOUT_MS
    : ACTION_SETTLE_NO_LOAD_TIMEOUT_MS
}
