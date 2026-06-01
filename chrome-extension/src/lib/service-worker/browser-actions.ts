import type {
  BrowserActionArgs,
  BrowserActionResult,
  BrowserCommand,
  ChromeApi,
  ChromeCookieInfo,
  ChromeDownloadItem,
  ChromeTabInfo,
  ChromeWebNavigationDetails,
  ChromeWebNavigationFrame
} from './types'

type BrowserActionDependencies = {
  chromeApi: ChromeApi
}

const debuggerActionLocks = new Map<number, Promise<void>>()
let rememberedActiveTabId: number | null = null
const DEBUGGER_PROTOCOL_VERSION = '1.3'
const DEBUGGER_COMMAND_MAX_RETRIES = 2
const NETWORK_IDLE_QUIET_MS = 500
const DEFAULT_PAGE_READY_TIMEOUT_MS = 30_000
const PRE_ACTION_LOADING_TIMEOUT_MS = 1_500
const ACTION_SETTLE_NO_LOAD_TIMEOUT_MS = 1_000
const SCRIPT_NAVIGATION_SETTLE_NO_LOAD_TIMEOUT_MS = 2_500

type NavigationWaitUntil = 'committed' | 'domcontentloaded' | 'complete' | 'history_change'
type NavigationEventName =
  | 'before_navigate'
  | 'committed'
  | 'dom_content_loaded'
  | 'completed'
  | 'error_occurred'
  | 'history_state_updated'
  | 'reference_fragment_updated'

function delay(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms))
}

export async function executeBrowserAction(
  command: BrowserCommand,
  deps: BrowserActionDependencies
): Promise<BrowserActionResult> {
  const { chromeApi } = deps
  const args = command.args || {}

  if (args.action === 'list_tabs') {
    return listTabs(chromeApi, args)
  }

  if (args.action === 'download') {
    return downloadFile(chromeApi, args)
  }

  if (args.action === 'list_downloads') {
    return listDownloads(chromeApi, args)
  }

  if (args.action === 'cancel_download') {
    return cancelDownload(chromeApi, args)
  }

  if (args.action === 'open_download') {
    return openDownload(chromeApi, args)
  }

  if (args.action === 'get_cookies') {
    return getCookies(chromeApi, args)
  }

  if (args.action === 'set_cookie') {
    return setCookie(chromeApi, args)
  }

  if (args.action === 'delete_cookie') {
    return deleteCookie(chromeApi, args)
  }

  if (args.action === 'clear_browser_cache') {
    return clearBrowserCache(chromeApi, args)
  }

  if (args.action === 'get_current_tab') {
    return { tab: tabSummary(await activeTab(chromeApi)) }
  }

  if (args.action === 'get_frames') {
    return getNavigationFrames(chromeApi, args)
  }

  if (args.action === 'open_tab') {
    const tab = await chromeApi.tabs.create({
      url: normalizeOptionalText(args.url),
      active: args.active ?? true,
      windowId: positiveInteger(args.window_id) || undefined
    })
    if (args.active ?? true) {
      rememberActiveTab(tab)
    }
    const pageReady = tab.id ? await waitForTabReady(chromeApi, tab.id, args) : null
    return withTopLevelTab({ opened: true }, pageReady, tab)
  }

  if (args.action === 'switch_tab') {
    const tabId = requirePositiveInteger(args.tab_id, 'switch_tab requires tab_id')
    const tab = await activateTab(chromeApi, tabId)
    const pageReady = await waitForTabReadyIfLoading(chromeApi, tabId, args)
    return withTopLevelTab({ switched: true }, pageReady, tab)
  }

  if (args.action === 'close_tab') {
    const tabId = requirePositiveInteger(args.tab_id, 'close_tab requires tab_id')
    await chromeApi.tabs.remove(tabId)
    forgetActiveTab(tabId)
    return { closed: true, tab_id: tabId }
  }

  if (args.action === 'launch_browser') {
    return { launched: false, connected: true, reason: 'browser is already running' }
  }

  if (args.action === 'navigate') {
    if (!args.url) {
      throw new Error('navigate requires url')
    }
    const tab = await tabForAction(chromeApi, args)
    const active = args.active ?? true
    if (!tab?.id) {
      const created = await chromeApi.tabs.create({
        url: args.url,
        active,
        windowId: positiveInteger(args.window_id) || undefined
      })
      const pageReady = created.id ? await waitForTabReady(chromeApi, created.id, args) : null
      return withTopLevelTab({ navigated: true, url: args.url }, pageReady, created)
    }
    const loadWatcher = createTabLoadWatcher(chromeApi, tab.id, actionTimeoutMs(args), args)
    try {
      const updated = await chromeApi.tabs.update(tab.id, { url: args.url, active })
      if (active && updated) {
        await focusWindow(chromeApi, updated.windowId).catch(() => undefined)
        rememberActiveTab(updated)
      }
      const pageReady = await waitForTabReady(chromeApi, tab.id, args, loadWatcher)
      return withTopLevelTab({ navigated: true, url: args.url }, pageReady, updated)
    } catch (error) {
      loadWatcher.cancel()
      throw error
    }
  }

  if (args.action === 'reload') {
    const tab = await tabForPageAction(chromeApi, args)
    const tabId = tab?.id
    if (!tabId) {
      throw new Error('no target tab')
    }
    const loadWatcher = createTabLoadWatcher(chromeApi, tabId, actionTimeoutMs(args), args)
    try {
      await chromeApi.tabs.reload(tabId, { bypassCache: args.bypass_cache ?? false })
      const pageReady = await waitForTabReady(chromeApi, tabId, args, loadWatcher)
      return withTopLevelTab(
        { reloaded: true, bypass_cache: args.bypass_cache ?? false },
        pageReady,
        tab
      )
    } catch (error) {
      loadWatcher.cancel()
      throw error
    }
  }

  if (args.action === 'go_back' || args.action === 'go_forward') {
    const tab = await tabForPageAction(chromeApi, args)
    const tabId = tab?.id
    if (!tabId) {
      throw new Error('no target tab')
    }
    const loadWatcher = createTabLoadWatcher(chromeApi, tabId, actionTimeoutMs(args), args)
    try {
      if (args.action === 'go_back' && chromeApi.tabs.goBack) {
        try {
          await chromeApi.tabs.goBack(tabId)
        } catch (error) {
          await chromeApi.scripting
            .executeScript<BrowserActionResult, BrowserActionArgs>({
              target: scriptTarget(tabId, args),
              world: 'ISOLATED',
              func: pageActionDispatcher,
              args: [args]
            })
            .catch(() => {
              throw error
            })
        }
      } else if (args.action === 'go_forward' && chromeApi.tabs.goForward) {
        try {
          await chromeApi.tabs.goForward(tabId)
        } catch (error) {
          await chromeApi.scripting
            .executeScript<BrowserActionResult, BrowserActionArgs>({
              target: scriptTarget(tabId, args),
              world: 'ISOLATED',
              func: pageActionDispatcher,
              args: [args]
            })
            .catch(() => {
              throw error
            })
        }
      } else {
        await chromeApi.scripting.executeScript<BrowserActionResult, BrowserActionArgs>({
          target: scriptTarget(tabId, args),
          world: 'ISOLATED',
          func: pageActionDispatcher,
          args: [args]
        })
      }
      const pageReady = await waitForTabReady(chromeApi, tabId, args, loadWatcher)
      return withTopLevelTab(
        { [args.action === 'go_back' ? 'went_back' : 'went_forward']: true },
        pageReady,
        tab
      )
    } catch (error) {
      loadWatcher.cancel()
      throw error
    }
  }

  const tab = await tabForPageAction(chromeApi, args)
  const tabId = tab?.id
  if (!tabId) {
    throw new Error('no target tab')
  }

  await waitForTabReadyIfLoading(chromeApi, tabId, args, {
    bestEffort: true,
    timeoutMs: PRE_ACTION_LOADING_TIMEOUT_MS,
    noLoadTimeoutMs: ACTION_SETTLE_NO_LOAD_TIMEOUT_MS
  })

  const postActionLoadWatcher = shouldWaitAfterAction(args)
    ? createTabLoadWatcher(chromeApi, tabId, actionTimeoutMs(args), args)
    : null

  if (args.action === 'screenshot') {
    if (args.full_page || args.selector || hasViewportOverride(args)) {
      return captureScreenshotWithDebugger(chromeApi, tabId, args, tab)
    }
    const activeTab = await activateTab(chromeApi, tabId).catch(() => tab)
    const dataUrl = await chromeApi.tabs.captureVisibleTab(activeTab?.windowId || tab?.windowId, {
      format: 'png'
    })
    return {
      captured: true,
      tab: tabSummary(activeTab || tab),
      mime_type: 'image/png',
      size: dataUrl.length,
      data_url: args.include_data_url ? dataUrl : undefined
    }
  }

  if (args.action === 'get_accessibility_tree') {
    return getAccessibilityTree(chromeApi, tabId, args)
  }

  if (args.action === 'print_to_pdf') {
    return printToPdf(chromeApi, tabId, args, tab)
  }

  if (args.action === 'handle_dialog') {
    return handleDialog(chromeApi, tabId, args)
  }

  if (args.action === 'upload_file') {
    let result: BrowserActionResult
    try {
      result = await uploadFile(chromeApi, tabId, args)
    } catch (error) {
      postActionLoadWatcher?.cancel()
      throw error
    }
    return withPageReady(
      result,
      await waitForPageSettleAfterAction(chromeApi, tabId, args, postActionLoadWatcher)
    )
  }

  if (
    (args.action === 'click' || args.action === 'hover') &&
    !args.frame_id &&
    chromeApi.debugger
  ) {
    let result: BrowserActionResult
    try {
      result = await dispatchNativePointerAction(chromeApi, tabId, args)
    } catch (error) {
      postActionLoadWatcher?.cancel()
      throw error
    }
    return withPageReady(
      result,
      await waitForPageSettleAfterAction(chromeApi, tabId, args, postActionLoadWatcher)
    )
  }

  if (args.action === 'type_text' && !args.frame_id && chromeApi.debugger) {
    let result: BrowserActionResult | null
    try {
      result = await dispatchNativeTextInput(chromeApi, tabId, args)
    } catch (error) {
      postActionLoadWatcher?.cancel()
      throw error
    }
    if (result) {
      return withPageReady(
        result,
        await waitForPageSettleAfterAction(chromeApi, tabId, args, postActionLoadWatcher)
      )
    }
  }

  if (args.action === 'press_key' && !args.frame_id && chromeApi.debugger) {
    let result: BrowserActionResult
    try {
      result = await dispatchNativeKey(chromeApi, tabId, args)
    } catch (error) {
      postActionLoadWatcher?.cancel()
      throw error
    }
    return withPageReady(
      result,
      await waitForPageSettleAfterAction(chromeApi, tabId, args, postActionLoadWatcher)
    )
  }

  if (args.action === 'execute_javascript' && scriptExecutionMode(args) === 'debugger') {
    let result: BrowserActionResult
    try {
      result = await executeJavaScriptWithDebugger(chromeApi, tabId, args)
    } catch (error) {
      postActionLoadWatcher?.cancel()
      throw error
    }
    return withPageReady(
      result,
      await waitForPageSettleAfterAction(chromeApi, tabId, args, postActionLoadWatcher)
    )
  }

  let execution: { result: Awaited<BrowserActionResult> } | undefined
  try {
    ;[execution] = await chromeApi.scripting.executeScript<BrowserActionResult, BrowserActionArgs>({
      target: scriptTarget(tabId, args),
      world: args.action === 'execute_javascript' ? scriptExecutionWorld(args) : 'ISOLATED',
      func: pageActionDispatcher,
      args: [args]
    })
  } catch (error) {
    postActionLoadWatcher?.cancel()
    throw error
  }
  if (execution?.result === undefined) {
    const action = args.action || 'browser action'
    throw new Error(`${action} did not return a script result`)
  }
  const result = execution.result
  return withPageReady(
    result,
    await waitForPageSettleAfterAction(chromeApi, tabId, args, postActionLoadWatcher)
  )
}

async function listTabs(
  chromeApi: ChromeApi,
  args: BrowserActionArgs
): Promise<BrowserActionResult> {
  const windowId = positiveInteger(args.window_id)
  const queryInfo = windowId ? { windowId } : {}
  const tabs = await chromeApi.tabs.query(queryInfo)
  const active = windowId ? tabs.find((tab) => tab.active) || null : await activeTab(chromeApi)
  return {
    tabs: tabs.map(tabSummary),
    active_tab_id: active?.id || null
  }
}

async function downloadFile(
  chromeApi: ChromeApi,
  args: BrowserActionArgs
): Promise<BrowserActionResult> {
  const downloads = requireDownloads(chromeApi)
  const url = normalizeOptionalText(args.url)
  if (!url) {
    throw new Error('download requires url')
  }
  const filename = normalizeOptionalText(args.filename)
  const downloadId = await downloads.download({
    url,
    filename,
    saveAs: args.save_as ?? false
  })
  const download = await waitForDownloadComplete(downloads, downloadId, actionTimeoutMs(args))
  return {
    downloaded: true,
    download_id: downloadId,
    url,
    filename: filename || null,
    download: downloadSummary(download)
  }
}

async function listDownloads(
  chromeApi: ChromeApi,
  args: BrowserActionArgs
): Promise<BrowserActionResult> {
  const downloads = requireDownloads(chromeApi)
  const downloadId = positiveInteger(args.download_id)
  const limit = Math.max(1, Math.min(100, positiveInteger(args.amount) || 50))
  const query: { id?: number; limit?: number; orderBy?: string[]; state?: string } = {
    limit,
    orderBy: ['-startTime']
  }
  if (downloadId) {
    query.id = downloadId
  }
  const state = normalizeOptionalText(args.value)
  if (state) {
    query.state = state
  }
  const downloadsFound = await downloads.search(query)
  return {
    downloads: downloadsFound.map(downloadSummary),
    count: downloadsFound.length
  }
}

async function cancelDownload(
  chromeApi: ChromeApi,
  args: BrowserActionArgs
): Promise<BrowserActionResult> {
  const downloads = requireDownloads(chromeApi)
  const downloadId = requirePositiveInteger(
    args.download_id,
    'cancel_download requires download_id'
  )
  await downloads.cancel(downloadId)
  return { canceled: true, download_id: downloadId }
}

async function openDownload(
  chromeApi: ChromeApi,
  args: BrowserActionArgs
): Promise<BrowserActionResult> {
  const downloads = requireDownloads(chromeApi)
  const downloadId = requirePositiveInteger(args.download_id, 'open_download requires download_id')
  await downloads.open(downloadId)
  return { opened: true, download_id: downloadId }
}

async function getCookies(
  chromeApi: ChromeApi,
  args: BrowserActionArgs
): Promise<BrowserActionResult> {
  const cookies = requireCookies(chromeApi)
  const domain = normalizeOptionalText(args.domain)
  const name = normalizeOptionalText(args.name)
  const storeId = normalizeOptionalText(args.store_id)
  const details: { url?: string; domain?: string; name?: string; storeId?: string } = {}
  if (domain) {
    details.domain = domain
  } else {
    details.url = await cookieUrl(chromeApi, args)
  }
  if (name) {
    details.name = name
  }
  if (storeId) {
    details.storeId = storeId
  }
  const found = await cookies.getAll(details)
  return { cookies: found.map(cookieSummary), count: found.length }
}

async function setCookie(
  chromeApi: ChromeApi,
  args: BrowserActionArgs
): Promise<BrowserActionResult> {
  const cookies = requireCookies(chromeApi)
  const name = normalizeOptionalText(args.name)
  if (!name) {
    throw new Error('set_cookie requires name')
  }
  const value = typeof args.value === 'string' ? args.value : ''
  const details = {
    url: await cookieUrl(chromeApi, args),
    name,
    value,
    domain: normalizeOptionalText(args.domain),
    path: normalizeOptionalText(args.path),
    secure: args.secure,
    httpOnly: args.http_only,
    sameSite: args.same_site,
    expirationDate: typeof args.expiration_date === 'number' ? args.expiration_date : undefined,
    storeId: normalizeOptionalText(args.store_id)
  }
  const cookie = await cookies.set(details)
  return { set: true, cookie: cookieSummary(cookie) }
}

async function deleteCookie(
  chromeApi: ChromeApi,
  args: BrowserActionArgs
): Promise<BrowserActionResult> {
  const cookies = requireCookies(chromeApi)
  const name = normalizeOptionalText(args.name)
  if (!name) {
    throw new Error('delete_cookie requires name')
  }
  const removed = await cookies.remove({
    url: await cookieUrl(chromeApi, args),
    name,
    storeId: normalizeOptionalText(args.store_id)
  })
  return { deleted: Boolean(removed), cookie: removed || null }
}

async function clearBrowserCache(
  chromeApi: ChromeApi,
  args: BrowserActionArgs
): Promise<BrowserActionResult> {
  if (!chromeApi.browsingData) {
    throw new Error('Chrome browsingData API is unavailable; enable the browsingData permission')
  }
  const origins = normalizedOrigins(args)
  const since =
    typeof args.since_ms === 'number' && Number.isFinite(args.since_ms)
      ? Math.max(0, Math.floor(args.since_ms))
      : 0
  await chromeApi.browsingData.remove(
    { since, origins: origins.length ? origins : undefined },
    {
      cache: true,
      cacheStorage: true,
      indexedDB: true,
      localStorage: true,
      serviceWorkers: true
    }
  )
  return { cleared: true, origins, since_ms: since }
}

function requireDownloads(chromeApi: ChromeApi): NonNullable<ChromeApi['downloads']> {
  if (!chromeApi.downloads) {
    throw new Error('Chrome downloads API is unavailable; enable the downloads permission')
  }
  return chromeApi.downloads
}

function requireCookies(chromeApi: ChromeApi): NonNullable<ChromeApi['cookies']> {
  if (!chromeApi.cookies) {
    throw new Error('Chrome cookies API is unavailable; enable the cookies permission')
  }
  return chromeApi.cookies
}

function downloadSummary(item: ChromeDownloadItem): Record<string, unknown> {
  return {
    id: item.id,
    url: item.url || '',
    final_url: item.finalUrl || '',
    filename: item.filename || '',
    state: item.state || '',
    paused: Boolean(item.paused),
    error: item.error || null,
    bytes_received: item.bytesReceived || 0,
    total_bytes: item.totalBytes || 0,
    start_time: item.startTime || null,
    end_time: item.endTime || null,
    exists: item.exists ?? null
  }
}

function waitForDownloadComplete(
  downloads: NonNullable<ChromeApi['downloads']>,
  downloadId: number,
  timeout: number
): Promise<ChromeDownloadItem> {
  const startedAt = Date.now()

  return new Promise((resolve, reject) => {
    const poll = async () => {
      try {
        const [item] = await downloads.search({ id: downloadId, limit: 1 })
        if (item?.state === 'complete') {
          resolve(item)
          return
        }
        if (item?.state === 'interrupted') {
          reject(new Error(`download interrupted: ${item.error || downloadId}`))
          return
        }
        if (Date.now() - startedAt >= timeout) {
          reject(new Error(`download ${downloadId} did not complete before timeout: ${timeout}ms`))
          return
        }
        setTimeout(poll, 250)
      } catch (error) {
        reject(error instanceof Error ? error : new Error(String(error)))
      }
    }

    void poll()
  })
}

function cookieSummary(
  cookie: ChromeCookieInfo | null | undefined
): Record<string, unknown> | null {
  if (!cookie) {
    return null
  }
  return {
    name: cookie.name || '',
    value: cookie.value || '',
    domain: cookie.domain || '',
    path: cookie.path || '',
    secure: Boolean(cookie.secure),
    http_only: Boolean(cookie.httpOnly),
    same_site: cookie.sameSite || null,
    expiration_date: cookie.expirationDate || null,
    session: Boolean(cookie.session),
    store_id: cookie.storeId || null
  }
}

async function cookieUrl(chromeApi: ChromeApi, args: BrowserActionArgs): Promise<string> {
  const explicit = normalizeOptionalText(args.url)
  if (explicit) {
    return explicit
  }
  const tab = await activeTab(chromeApi)
  const url = normalizeOptionalText(tab?.url)
  if (!url) {
    throw new Error('cookie action requires url or an active tab with a URL')
  }
  return url
}

function normalizedOrigins(args: BrowserActionArgs): string[] {
  const origins = Array.isArray(args.origins)
    ? args.origins
        .filter((origin) => typeof origin === 'string' && origin.trim())
        .map((origin) => origin.trim())
    : []
  const url = normalizeOptionalText(args.url)
  if (!url) {
    return origins
  }
  try {
    origins.push(new URL(url).origin)
  } catch (_error) {
    origins.push(url)
  }
  return Array.from(new Set(origins))
}

async function getNavigationFrames(
  chromeApi: ChromeApi,
  args: BrowserActionArgs
): Promise<BrowserActionResult> {
  const webNavigation = chromeApi.webNavigation
  if (!webNavigation?.getAllFrames) {
    throw new Error(
      'Chrome webNavigation frame API is unavailable; enable webNavigation permission'
    )
  }
  const tab = await tabForAction(chromeApi, args)
  const tabId = tab?.id
  if (!tabId) {
    throw new Error('no target tab')
  }
  const requestedFrameId = nonNegativeInteger(args.frame_id)
  if (requestedFrameId !== null && webNavigation.getFrame) {
    const frame = await webNavigation.getFrame({ tabId, frameId: requestedFrameId })
    return {
      frames: frame ? [navigationFrameSummary(frame)] : [],
      count: frame ? 1 : 0,
      tab: tabSummary(tab)
    }
  }
  const frames = (await webNavigation.getAllFrames({ tabId })) || []
  return {
    frames: frames.map(navigationFrameSummary),
    count: frames.length,
    tab: tabSummary(tab)
  }
}

function navigationFrameSummary(frame: ChromeWebNavigationFrame): Record<string, unknown> {
  return {
    frame_id: frame.frameId,
    parent_frame_id: frame.parentFrameId ?? null,
    process_id: frame.processId ?? null,
    url: frame.url || '',
    error_occurred: Boolean(frame.errorOccurred)
  }
}

async function tabForAction(
  chromeApi: ChromeApi,
  args: BrowserActionArgs
): Promise<ChromeTabInfo | null> {
  const tabId = positiveInteger(args.tab_id)
  if (tabId) {
    return chromeApi.tabs.get(tabId)
  }
  return activeTab(chromeApi)
}

async function tabForPageAction(
  chromeApi: ChromeApi,
  args: BrowserActionArgs
): Promise<ChromeTabInfo | null> {
  const tabId = positiveInteger(args.tab_id)
  if (tabId) {
    return activateTab(chromeApi, tabId)
  }
  return activeTab(chromeApi)
}

function scriptTarget(
  tabId: number,
  args: BrowserActionArgs
): { tabId: number; frameIds?: number[] } {
  const frameId = positiveInteger(args.frame_id)
  return frameId ? { tabId, frameIds: [frameId] } : { tabId }
}

type ScriptExecutionMode = 'debugger' | 'scripting'
type ScriptExecutionWorld = 'ISOLATED' | 'MAIN'
type RequestedScriptWorld = ScriptExecutionWorld | 'DEBUGGER'

function scriptExecutionMode(args: BrowserActionArgs): ScriptExecutionMode {
  const world = requestedScriptWorld(args.world)
  if (world === 'DEBUGGER') {
    return 'debugger'
  }
  return args.use_bridge === false ? 'scripting' : 'debugger'
}

function scriptExecutionWorld(args: BrowserActionArgs): ScriptExecutionWorld {
  return requestedScriptWorld(args.world) === 'MAIN' ? 'MAIN' : 'ISOLATED'
}

function requestedScriptWorld(value: unknown): RequestedScriptWorld {
  const normalized = typeof value === 'string' ? value.trim().toLowerCase() : ''
  if (normalized === 'main') {
    return 'MAIN'
  }
  if (normalized === 'debugger' || normalized === 'bridge') {
    return 'DEBUGGER'
  }
  return 'ISOLATED'
}

function scriptWithImplicitReturn(code: string): string | null {
  const body = code.trim().replace(/;+$/, '')
  if (!body) {
    return null
  }
  const splitAt = lastTopLevelSemicolon(body)
  if (splitAt < 0) {
    return null
  }
  const prefix = body.slice(0, splitAt + 1)
  const tail = body.slice(splitAt + 1).trim()
  if (!tail || !canImplicitlyReturn(tail)) {
    return null
  }
  return `${prefix}\nreturn (${tail});`
}

function canImplicitlyReturn(statement: string): boolean {
  return !/^(break|catch|class|const|continue|do|export|finally|for|function|if|import|let|return|switch|throw|try|var|while)\b/.test(
    statement
  )
}

function lastTopLevelSemicolon(code: string): number {
  let quote: string | null = null
  let escaped = false
  let lineComment = false
  let blockComment = false
  let parenDepth = 0
  let braceDepth = 0
  let bracketDepth = 0
  let last = -1

  for (let index = 0; index < code.length; index += 1) {
    const char = code[index]
    const next = code[index + 1]

    if (lineComment) {
      if (char === '\n' || char === '\r') {
        lineComment = false
      }
      continue
    }
    if (blockComment) {
      if (char === '*' && next === '/') {
        blockComment = false
        index += 1
      }
      continue
    }
    if (quote) {
      if (escaped) {
        escaped = false
      } else if (char === '\\') {
        escaped = true
      } else if (char === quote) {
        quote = null
      }
      continue
    }

    if (char === '/' && next === '/') {
      lineComment = true
      index += 1
      continue
    }
    if (char === '/' && next === '*') {
      blockComment = true
      index += 1
      continue
    }
    if (char === '"' || char === "'" || char === '`') {
      quote = char
      continue
    }
    if (char === '(') {
      parenDepth += 1
    } else if (char === ')') {
      parenDepth = Math.max(0, parenDepth - 1)
    } else if (char === '{') {
      braceDepth += 1
    } else if (char === '}') {
      braceDepth = Math.max(0, braceDepth - 1)
    } else if (char === '[') {
      bracketDepth += 1
    } else if (char === ']') {
      bracketDepth = Math.max(0, bracketDepth - 1)
    } else if (char === ';' && parenDepth === 0 && braceDepth === 0 && bracketDepth === 0) {
      last = index
    }
  }

  return last
}

type DebuggerTarget = { tabId: number }
type AttachedDebuggerTarget = DebuggerTarget & {
  sendCommand<Result = unknown>(method: string, commandParams?: object): Promise<Result>
}
type RuntimeRemoteObject = {
  type?: string
  subtype?: string
  value?: unknown
  unserializableValue?: string
  description?: string
}
type RuntimeExceptionDetails = {
  text?: string
  exception?: RuntimeRemoteObject
  lineNumber?: number
  columnNumber?: number
}
type RuntimeEvaluateResult = {
  result?: RuntimeRemoteObject
  exceptionDetails?: RuntimeExceptionDetails
}
type RuntimeEvaluateParams = {
  expression: string
  awaitPromise?: boolean
  returnByValue?: boolean
  userGesture?: boolean
  replMode?: boolean
}
type PageCaptureScreenshotResult = { data?: string }
type PagePrintToPdfResult = { data?: string }
type PageLayoutMetricsResult = {
  contentSize?: { x?: number; y?: number; width?: number; height?: number }
}
type DomGetDocumentResult = { root?: { nodeId?: number } }
type DomQuerySelectorResult = { nodeId?: number }

async function withAttachedDebugger<T>(
  chromeApi: ChromeApi,
  tabId: number,
  task: (target: AttachedDebuggerTarget) => Promise<T>
): Promise<T> {
  if (!chromeApi.debugger) {
    throw new Error('Chrome debugger API is unavailable; enable the debugger permission')
  }

  const target = { tabId }
  let shouldDetach = false
  const ensureAttached = async () => {
    await attachDebuggerIfNeeded(chromeApi, target)
    shouldDetach = true
  }

  try {
    await ensureAttached()
    const attachedTarget: AttachedDebuggerTarget = {
      ...target,
      sendCommand: (method, commandParams) =>
        sendAttachedDebuggerCommand(chromeApi, target, method, commandParams, ensureAttached)
    }
    return await task(attachedTarget)
  } finally {
    if (shouldDetach) {
      await chromeApi.debugger.detach(target).catch(() => undefined)
    }
  }
}

async function attachDebuggerIfNeeded(chromeApi: ChromeApi, target: DebuggerTarget): Promise<void> {
  try {
    await chromeApi.debugger!.attach(target, DEBUGGER_PROTOCOL_VERSION)
  } catch (error) {
    if (isDebuggerAlreadyAttachedError(error)) {
      return
    }
    throw error
  }
}

async function sendAttachedDebuggerCommand<Result = unknown>(
  chromeApi: ChromeApi,
  target: DebuggerTarget,
  method: string,
  commandParams: object | undefined,
  ensureAttached: () => Promise<void>,
  retryCount = 0
): Promise<Result> {
  try {
    return await chromeApi.debugger!.sendCommand<Result>(
      target,
      method,
      commandParams as Record<string, unknown> | undefined
    )
  } catch (error) {
    if (isDebuggerDetachError(error) && retryCount < DEBUGGER_COMMAND_MAX_RETRIES) {
      await ensureAttached()
      return sendAttachedDebuggerCommand<Result>(
        chromeApi,
        target,
        method,
        commandParams,
        ensureAttached,
        retryCount + 1
      )
    }
    throw error
  }
}

function isDebuggerAlreadyAttachedError(error: unknown): boolean {
  return errorMessage(error).includes('Another debugger is already attached')
}

function isDebuggerDetachError(error: unknown): boolean {
  const message = errorMessage(error)
  return (
    message.includes('Debugger is not attached') ||
    message.includes('Cannot access a Target') ||
    message.includes('No target with given id')
  )
}

async function executeJavaScriptWithDebugger(
  chromeApi: ChromeApi,
  tabId: number,
  args: BrowserActionArgs
): Promise<Record<string, unknown>> {
  return runExclusiveDebuggerAction(tabId, () =>
    executeJavaScriptWithAttachedDebugger(chromeApi, tabId, args)
  )
}

function runExclusiveDebuggerAction<T>(tabId: number, task: () => Promise<T>): Promise<T> {
  const previous = debuggerActionLocks.get(tabId) || Promise.resolve()
  const run = previous.catch(() => undefined).then(task)
  const release = run.then(
    () => undefined,
    () => undefined
  )
  debuggerActionLocks.set(tabId, release)
  return run.finally(() => {
    if (debuggerActionLocks.get(tabId) === release) {
      debuggerActionLocks.delete(tabId)
    }
  })
}

async function executeJavaScriptWithAttachedDebugger(
  chromeApi: ChromeApi,
  tabId: number,
  args: BrowserActionArgs
): Promise<Record<string, unknown>> {
  const code = String(args.code || '')
  if (!code.trim()) {
    throw new Error('execute_javascript requires code')
  }
  if (args.frame_id !== undefined && args.frame_id !== null) {
    throw new Error(
      'execute_javascript frame_id is only supported when use_bridge is false and world is isolated or main'
    )
  }

  return withAttachedDebugger(chromeApi, tabId, async (target) => {
    await target.sendCommand('Runtime.enable').catch(() => undefined)
    const result = await evaluateDebuggerJavaScript(target, code)
    return { executed: true, world: 'debugger', result }
  })
}

async function evaluateDebuggerJavaScript(
  target: AttachedDebuggerTarget,
  code: string
): Promise<unknown> {
  const expression = code.trim().replace(/;+$/, '')
  const expressionResult = await sendDebuggerRuntimeEvaluate(target, `(${expression})`)
  if (!isSyntaxException(expressionResult.exceptionDetails)) {
    return debuggerEvaluationValue(expressionResult)
  }

  const implicitReturn = scriptWithImplicitReturn(code)
  if (implicitReturn) {
    const implicitResult = await sendDebuggerRuntimeEvaluate(
      target,
      `(function () {\n${implicitReturn}\n})()`
    )
    if (!isSyntaxException(implicitResult.exceptionDetails)) {
      return debuggerEvaluationValue(implicitResult)
    }
  }

  const bodyResult = await sendDebuggerRuntimeEvaluate(target, `(function () {\n${code}\n})()`)
  return debuggerEvaluationValue(bodyResult)
}

function sendDebuggerRuntimeEvaluate(
  target: AttachedDebuggerTarget,
  expression: string
): Promise<RuntimeEvaluateResult> {
  const params: RuntimeEvaluateParams = {
    expression,
    awaitPromise: true,
    returnByValue: true,
    userGesture: true,
    replMode: true
  }
  return target.sendCommand<RuntimeEvaluateResult>('Runtime.evaluate', params)
}

function isSyntaxException(details?: RuntimeExceptionDetails): boolean {
  return debuggerExceptionText(details).includes('SyntaxError')
}

function debuggerEvaluationValue(evaluation: RuntimeEvaluateResult): unknown {
  if (evaluation.exceptionDetails) {
    throw new Error(
      `execute_javascript failed: ${debuggerExceptionText(evaluation.exceptionDetails)}`
    )
  }
  const result = evaluation.result
  if (!result || result.type === 'undefined') {
    return null
  }
  if ('value' in result) {
    return result.value
  }
  return result.unserializableValue || result.description || null
}

function debuggerExceptionText(details?: RuntimeExceptionDetails): string {
  return (
    details?.exception?.description ||
    details?.exception?.value ||
    details?.text ||
    'Unknown JavaScript exception'
  ).toString()
}

async function captureScreenshotWithDebugger(
  chromeApi: ChromeApi,
  tabId: number,
  args: BrowserActionArgs,
  tab: ChromeTabInfo | null
): Promise<BrowserActionResult> {
  const active = await activateTab(chromeApi, tabId).catch(() => tab)
  const capture = await runExclusiveDebuggerAction(tabId, () =>
    withAttachedDebugger(chromeApi, tabId, async (target) => {
      await target.sendCommand('Page.enable').catch(() => undefined)
      const viewport = viewportOverrideParams(args)
      if (viewport) {
        await target.sendCommand('Emulation.setDeviceMetricsOverride', viewport)
      }
      try {
        const clip = args.selector
          ? await elementScreenshotClip(target, args.selector)
          : args.full_page
            ? await fullPageScreenshotClip(target)
            : undefined
        const params: Record<string, unknown> = {
          format: 'png',
          fromSurface: true,
          captureBeyondViewport: Boolean(args.full_page || args.selector)
        }
        if (clip) {
          params.clip = clip
        }
        const result = await target.sendCommand<PageCaptureScreenshotResult>(
          'Page.captureScreenshot',
          params
        )
        if (!result.data) {
          throw new Error('Page.captureScreenshot returned no image data')
        }
        return { data: result.data, clip, viewport }
      } finally {
        if (viewport) {
          await target.sendCommand('Emulation.clearDeviceMetricsOverride').catch(() => undefined)
        }
      }
    })
  )
  const dataUrl = `data:image/png;base64,${capture.data}`
  return {
    captured: true,
    tab: tabSummary(active || tab),
    mime_type: 'image/png',
    size: dataUrl.length,
    data_url: args.include_data_url ? dataUrl : undefined,
    full_page: Boolean(args.full_page),
    selector: args.selector || null,
    clip: capture.clip || null,
    viewport: capture.viewport || null
  }
}

function hasViewportOverride(args: BrowserActionArgs): boolean {
  return (
    args.viewport_width !== undefined ||
    args.viewport_height !== undefined ||
    args.device_scale_factor !== undefined
  )
}

function viewportOverrideParams(args: BrowserActionArgs): Record<string, unknown> | null {
  if (!hasViewportOverride(args)) {
    return null
  }
  const width = positiveInteger(args.viewport_width)
  const height = positiveInteger(args.viewport_height)
  if (!width || !height) {
    throw new Error('viewport override requires viewport_width and viewport_height')
  }
  const deviceScaleFactor =
    typeof args.device_scale_factor === 'number' && Number.isFinite(args.device_scale_factor)
      ? Math.max(0.1, Math.min(5, args.device_scale_factor))
      : 1
  return {
    width: Math.min(10000, width),
    height: Math.min(10000, height),
    deviceScaleFactor,
    mobile: false
  }
}

async function elementScreenshotClip(
  target: AttachedDebuggerTarget,
  selector: string
): Promise<Record<string, number>> {
  const script = `(() => {
    const selector = ${JSON.stringify(selector)};
    function deepQuery(root, query) {
      const direct = root.querySelector(query);
      if (direct) return direct;
      for (const element of Array.from(root.querySelectorAll('*'))) {
        const shadowRoot = element.shadowRoot;
        if (!shadowRoot) continue;
        const found = deepQuery(shadowRoot, query);
        if (found) return found;
      }
      return null;
    }
    const element = deepQuery(document, selector);
    if (!element) return null;
    element.scrollIntoView({ block: 'center', inline: 'center' });
    const rect = element.getBoundingClientRect();
    return {
      x: Math.max(0, rect.left + window.scrollX),
      y: Math.max(0, rect.top + window.scrollY),
      width: Math.max(1, rect.width),
      height: Math.max(1, rect.height),
      scale: 1
    };
  })()`
  const clip = await evaluateDebuggerJavaScript(target, script)
  if (!isScreenshotClip(clip)) {
    throw new Error(`selector not found or has no visible bounds: ${selector}`)
  }
  return clip
}

async function fullPageScreenshotClip(
  target: AttachedDebuggerTarget
): Promise<Record<string, number>> {
  const metrics = await target.sendCommand<PageLayoutMetricsResult>('Page.getLayoutMetrics')
  const contentSize = metrics.contentSize || {}
  return {
    x: 0,
    y: 0,
    width: Math.max(1, Math.ceil(contentSize.width || 1)),
    height: Math.max(1, Math.ceil(contentSize.height || 1)),
    scale: 1
  }
}

function isScreenshotClip(value: unknown): value is Record<string, number> {
  if (!value || typeof value !== 'object') {
    return false
  }
  const clip = value as Record<string, unknown>
  return ['x', 'y', 'width', 'height', 'scale'].every(
    (key) => typeof clip[key] === 'number' && Number.isFinite(clip[key])
  )
}

async function getAccessibilityTree(
  chromeApi: ChromeApi,
  tabId: number,
  args: BrowserActionArgs
): Promise<BrowserActionResult> {
  return runExclusiveDebuggerAction(tabId, () =>
    withAttachedDebugger(chromeApi, tabId, async (target) => {
      await target.sendCommand('Accessibility.enable').catch(() => undefined)
      const result = await target.sendCommand<{ nodes?: unknown[] }>('Accessibility.getFullAXTree')
      const nodes = Array.isArray(result.nodes) ? result.nodes : []
      const maxNodes = Math.max(50, Math.min(1000, positiveInteger(args.amount) || 500))
      return {
        accessibility_tree: nodes.slice(0, maxNodes).map(compactAccessibilityNode),
        count: nodes.length,
        truncated: nodes.length > maxNodes
      }
    })
  )
}

function compactAccessibilityNode(node: unknown): Record<string, unknown> {
  const entry = node && typeof node === 'object' ? (node as Record<string, unknown>) : {}
  return {
    node_id: entry.nodeId || null,
    backend_dom_node_id: entry.backendDOMNodeId || null,
    role: remoteValue(entry.role),
    name: remoteValue(entry.name),
    value: remoteValue(entry.value),
    description: remoteValue(entry.description),
    ignored: Boolean(entry.ignored),
    child_ids: Array.isArray(entry.childIds) ? entry.childIds.slice(0, 80) : [],
    properties: compactAccessibilityProperties(entry.properties)
  }
}

function compactAccessibilityProperties(value: unknown): Array<Record<string, unknown>> {
  if (!Array.isArray(value)) {
    return []
  }
  return value.slice(0, 40).map((property) => {
    const entry =
      property && typeof property === 'object' ? (property as Record<string, unknown>) : {}
    return { name: entry.name || '', value: remoteValue(entry.value) }
  })
}

function remoteValue(value: unknown): unknown {
  if (!value || typeof value !== 'object') {
    return value ?? null
  }
  const entry = value as Record<string, unknown>
  if ('value' in entry) {
    return entry.value ?? null
  }
  if ('description' in entry) {
    return entry.description ?? null
  }
  return null
}

async function printToPdf(
  chromeApi: ChromeApi,
  tabId: number,
  args: BrowserActionArgs,
  tab: ChromeTabInfo | null
): Promise<BrowserActionResult> {
  const result = await runExclusiveDebuggerAction(tabId, () =>
    withAttachedDebugger(chromeApi, tabId, async (target) => {
      await target.sendCommand('Page.enable').catch(() => undefined)
      const pdf = await target.sendCommand<PagePrintToPdfResult>('Page.printToPDF', {
        printBackground: true,
        preferCSSPageSize: true
      })
      if (!pdf.data) {
        throw new Error('Page.printToPDF returned no PDF data')
      }
      return pdf
    })
  )
  const dataUrl = `data:application/pdf;base64,${result.data}`
  return {
    printed: true,
    tab: tabSummary(tab),
    mime_type: 'application/pdf',
    size: dataUrl.length,
    data_url: args.include_data_url ? dataUrl : undefined
  }
}

async function waitForNetworkIdle(
  chromeApi: ChromeApi,
  tabId: number,
  args: BrowserActionArgs
): Promise<BrowserActionResult> {
  const timeout = actionTimeoutMs(args, 30000)
  return runExclusiveDebuggerAction(tabId, () =>
    withAttachedDebugger(chromeApi, tabId, (target) =>
      waitForNetworkIdleWithAttachedDebugger(chromeApi, target, timeout)
    )
  )
}

async function waitForNetworkIdleWithAttachedDebugger(
  chromeApi: ChromeApi,
  target: AttachedDebuggerTarget,
  timeout: number
): Promise<BrowserActionResult> {
  const event = chromeApi.debugger?.onEvent
  if (!event) {
    throw new Error('Chrome debugger event API is unavailable; cannot wait for network idle')
  }

  let inFlight = 0
  let quietTimer: ReturnType<typeof setTimeout> | null = null
  let timeoutTimer: ReturnType<typeof setTimeout> | null = null
  let cleanedUp = false

  let resolveWait: (value: BrowserActionResult) => void
  let rejectWait: (reason: Error) => void
  const wait = new Promise<BrowserActionResult>((resolve, reject) => {
    resolveWait = resolve
    rejectWait = reject
  })

  const cleanup = () => {
    if (cleanedUp) {
      return
    }
    cleanedUp = true
    event.removeListener(listener)
    if (quietTimer) {
      clearTimeout(quietTimer)
    }
    if (timeoutTimer) {
      clearTimeout(timeoutTimer)
    }
  }

  const settle = (value: BrowserActionResult) => {
    cleanup()
    resolveWait(value)
  }

  const fail = (error: Error) => {
    cleanup()
    rejectWait(error)
  }

  const scheduleQuietCheck = () => {
    if (quietTimer) {
      clearTimeout(quietTimer)
    }
    if (inFlight > 0) {
      return
    }
    quietTimer = setTimeout(() => {
      settle({ network_idle: true, quiet_ms: NETWORK_IDLE_QUIET_MS })
    }, NETWORK_IDLE_QUIET_MS)
  }

  const listener = (
    source: { tabId?: number },
    method: string,
    _params?: Record<string, unknown>
  ) => {
    if (source.tabId !== target.tabId) {
      return
    }
    if (method === 'Network.requestWillBeSent') {
      inFlight += 1
      if (quietTimer) {
        clearTimeout(quietTimer)
        quietTimer = null
      }
      return
    }
    if (method === 'Network.loadingFinished' || method === 'Network.loadingFailed') {
      inFlight = Math.max(0, inFlight - 1)
      scheduleQuietCheck()
    }
  }

  event.addListener(listener)
  timeoutTimer = setTimeout(() => {
    fail(new Error(`network did not become idle before timeout: ${timeout}ms`))
  }, timeout)

  try {
    await target.sendCommand('Network.enable')
    scheduleQuietCheck()
    return await wait
  } finally {
    cleanup()
    await target.sendCommand('Network.disable').catch(() => undefined)
  }
}

async function handleDialog(
  chromeApi: ChromeApi,
  tabId: number,
  args: BrowserActionArgs
): Promise<BrowserActionResult> {
  return runExclusiveDebuggerAction(tabId, () =>
    withAttachedDebugger(chromeApi, tabId, async (target) => {
      await target.sendCommand('Page.enable').catch(() => undefined)
      await target.sendCommand('Page.handleJavaScriptDialog', {
        accept: args.accept ?? true,
        promptText: normalizeOptionalText(args.prompt_text)
      })
      return { handled_dialog: true, accepted: args.accept ?? true }
    })
  )
}

async function uploadFile(
  chromeApi: ChromeApi,
  tabId: number,
  args: BrowserActionArgs
): Promise<BrowserActionResult> {
  const selector = normalizeOptionalText(args.selector)
  if (!selector) {
    throw new Error('upload_file requires selector')
  }
  const files = Array.isArray(args.files)
    ? args.files
        .filter((file) => typeof file === 'string' && file.trim())
        .map((file) => file.trim())
    : []
  if (!files.length) {
    throw new Error('upload_file requires files')
  }

  return runExclusiveDebuggerAction(tabId, () =>
    withAttachedDebugger(chromeApi, tabId, async (target) => {
      await target.sendCommand('DOM.enable').catch(() => undefined)
      const documentResult = await target.sendCommand<DomGetDocumentResult>('DOM.getDocument', {
        depth: -1,
        pierce: true
      })
      const rootNodeId = documentResult.root?.nodeId
      if (!rootNodeId) {
        throw new Error('DOM.getDocument returned no root node')
      }
      const queryResult = await target.sendCommand<DomQuerySelectorResult>('DOM.querySelector', {
        nodeId: rootNodeId,
        selector
      })
      const nodeId = queryResult.nodeId
      if (!nodeId) {
        throw new Error(`selector not found: ${selector}`)
      }
      await target.sendCommand('DOM.setFileInputFiles', { nodeId, files })
      return { uploaded: true, selector, files, count: files.length }
    })
  )
}

async function dispatchNativePointerAction(
  chromeApi: ChromeApi,
  tabId: number,
  args: BrowserActionArgs
): Promise<BrowserActionResult> {
  const [execution] = await chromeApi.scripting.executeScript<
    Record<string, unknown>,
    BrowserActionArgs
  >({
    target: { tabId },
    world: 'ISOLATED',
    func: resolveInputTarget,
    args: [args]
  })
  const target = execution?.result
  const x = typeof target?.x === 'number' ? target.x : null
  const y = typeof target?.y === 'number' ? target.y : null
  if (x === null || y === null) {
    throw new Error('could not resolve a native input coordinate')
  }

  await runExclusiveDebuggerAction(tabId, () =>
    withAttachedDebugger(chromeApi, tabId, async (debuggerTarget) => {
      await dispatchNativeMouseMove(debuggerTarget, x, y)
      if (args.action === 'click') {
        await dispatchNativePrimaryClick(
          debuggerTarget,
          x,
          y,
          await isMobileLikeTarget(debuggerTarget)
        )
      }
    })
  )

  return {
    [args.action === 'click' ? 'clicked' : 'hovered']: true,
    native: true,
    selector: args.selector || null,
    label: target.label || '',
    x,
    y,
    bounding_box: target.bounding_box || null
  }
}

async function dispatchNativeTextInput(
  chromeApi: ChromeApi,
  tabId: number,
  args: BrowserActionArgs
): Promise<BrowserActionResult | null> {
  const [execution] = await chromeApi.scripting.executeScript<
    Record<string, unknown>,
    BrowserActionArgs
  >({
    target: { tabId },
    world: 'ISOLATED',
    func: resolveInputTarget,
    args: [args]
  })
  const inputTarget = execution?.result
  if (inputTarget?.native_text_input === false) {
    return null
  }
  const x = typeof inputTarget?.x === 'number' ? inputTarget.x : null
  const y = typeof inputTarget?.y === 'number' ? inputTarget.y : null
  if (x === null || y === null) {
    throw new Error('could not resolve a native text input coordinate')
  }

  const text = String(args.text || '')
  let verified: boolean | null = null
  await runExclusiveDebuggerAction(tabId, () =>
    withAttachedDebugger(chromeApi, tabId, async (target) => {
      const useTouch = await isMobileLikeTarget(target)
      await dispatchNativeMouseMove(target, x, y)
      await dispatchNativePrimaryClick(target, x, y, useTouch)
      await delay(50)
      await dispatchSelectAll(target)
      await dispatchKeyDefinition(target, keyDefinition('Backspace'))
      if (text) {
        await target.sendCommand('Input.insertText', { text })
      }
      verified = await verifyNativeTextInput(target, inputTarget.selector, text)
    })
  )

  if (verified === false) {
    return null
  }

  return {
    typed: true,
    native: true,
    selector: inputTarget.selector || args.selector || null,
    active_element: !args.selector,
    verified,
    label: inputTarget.label || '',
    length: text.length,
    x,
    y,
    bounding_box: inputTarget.bounding_box || null
  }
}

async function verifyNativeTextInput(
  target: AttachedDebuggerTarget,
  selector: unknown,
  expectedText: string
): Promise<boolean | null> {
  if (typeof selector !== 'string' || !selector.trim()) {
    return null
  }

  try {
    const evaluation = await sendDebuggerRuntimeEvaluate(
      target,
      `(() => {
        const element = document.querySelector(${JSON.stringify(selector)});
        if (!element) return null;
        const value = 'value' in element ? String(element.value) : String(element.textContent || '');
        return { value, matches: value === ${JSON.stringify(expectedText)} };
      })()`
    )
    const value = debuggerEvaluationValue(evaluation)
    return value && typeof value === 'object' && 'matches' in value
      ? Boolean((value as Record<string, unknown>).matches)
      : null
  } catch (_error) {
    return null
  }
}

async function isMobileLikeTarget(target: AttachedDebuggerTarget): Promise<boolean> {
  const result = await target.sendCommand<RuntimeEvaluateResult>('Runtime.evaluate', {
    expression:
      '(/Android|iPhone|iPad|iPod|Mobile/i.test(navigator.userAgent) || navigator.maxTouchPoints > 1)',
    returnByValue: true
  })
  return Boolean(result.result?.value)
}

async function dispatchNativeMouseMove(
  target: AttachedDebuggerTarget,
  x: number,
  y: number
): Promise<void> {
  await target.sendCommand('Input.dispatchMouseEvent', {
    type: 'mouseMoved',
    x,
    y
  })
}

async function dispatchNativePrimaryClick(
  target: AttachedDebuggerTarget,
  x: number,
  y: number,
  useTouch: boolean
): Promise<void> {
  if (useTouch) {
    const touchPoints = [{ x: Math.round(x), y: Math.round(y) }]
    await target.sendCommand('Input.dispatchTouchEvent', {
      type: 'touchStart',
      touchPoints,
      modifiers: 0
    })
    await target.sendCommand('Input.dispatchTouchEvent', {
      type: 'touchEnd',
      touchPoints: [],
      modifiers: 0
    })
    return
  }

  await target.sendCommand('Input.dispatchMouseEvent', {
    type: 'mousePressed',
    x,
    y,
    button: 'left',
    clickCount: 1
  })
  await target.sendCommand('Input.dispatchMouseEvent', {
    type: 'mouseReleased',
    x,
    y,
    button: 'left',
    clickCount: 1
  })
}

async function dispatchSelectAll(target: AttachedDebuggerTarget): Promise<void> {
  await target.sendCommand('Input.dispatchKeyEvent', {
    type: 'keyDown',
    commands: ['selectAll']
  })
  await target.sendCommand('Input.dispatchKeyEvent', {
    type: 'keyUp',
    commands: ['selectAll']
  })
}

async function dispatchKeyDefinition(
  target: AttachedDebuggerTarget,
  definition: Record<string, unknown>
): Promise<void> {
  await target.sendCommand('Input.dispatchKeyEvent', {
    type: 'keyDown',
    ...definition
  })
  await target.sendCommand('Input.dispatchKeyEvent', {
    type: 'keyUp',
    key: definition.key,
    code: definition.code,
    windowsVirtualKeyCode: definition.windowsVirtualKeyCode,
    nativeVirtualKeyCode: definition.nativeVirtualKeyCode
  })
}

async function dispatchNativeKey(
  chromeApi: ChromeApi,
  tabId: number,
  args: BrowserActionArgs
): Promise<BrowserActionResult> {
  const key = normalizeOptionalText(args.key) || 'Enter'
  const definition = keyDefinition(key)
  await runExclusiveDebuggerAction(tabId, () =>
    withAttachedDebugger(chromeApi, tabId, async (target) => {
      await dispatchKeyDefinition(target, definition)
    })
  )
  return { pressed: true, native: true, key }
}

function keyDefinition(key: string): Record<string, unknown> {
  const normalizedKey = keyAliases[key] || key
  const special: Record<string, { code: string; windowsVirtualKeyCode: number }> = {
    Enter: { code: 'Enter', windowsVirtualKeyCode: 13 },
    Escape: { code: 'Escape', windowsVirtualKeyCode: 27 },
    Tab: { code: 'Tab', windowsVirtualKeyCode: 9 },
    Space: { code: 'Space', windowsVirtualKeyCode: 32 },
    Backspace: { code: 'Backspace', windowsVirtualKeyCode: 8 },
    Delete: { code: 'Delete', windowsVirtualKeyCode: 46 },
    ArrowUp: { code: 'ArrowUp', windowsVirtualKeyCode: 38 },
    ArrowDown: { code: 'ArrowDown', windowsVirtualKeyCode: 40 },
    ArrowLeft: { code: 'ArrowLeft', windowsVirtualKeyCode: 37 },
    ArrowRight: { code: 'ArrowRight', windowsVirtualKeyCode: 39 },
    Home: { code: 'Home', windowsVirtualKeyCode: 36 },
    End: { code: 'End', windowsVirtualKeyCode: 35 },
    PageUp: { code: 'PageUp', windowsVirtualKeyCode: 33 },
    PageDown: { code: 'PageDown', windowsVirtualKeyCode: 34 }
  }
  const mapped = special[normalizedKey]
  if (mapped) {
    return {
      key: normalizedKey === 'Space' ? ' ' : normalizedKey,
      code: mapped.code,
      windowsVirtualKeyCode: mapped.windowsVirtualKeyCode,
      nativeVirtualKeyCode: mapped.windowsVirtualKeyCode
    }
  }
  const text = normalizedKey.length === 1 ? normalizedKey : ''
  const upper = text.toUpperCase()
  const windowsVirtualKeyCode = upper ? upper.charCodeAt(0) : 0
  return {
    key: normalizedKey,
    code: upper && /^[A-Z]$/.test(upper) ? `Key${upper}` : normalizedKey,
    text,
    windowsVirtualKeyCode,
    nativeVirtualKeyCode: windowsVirtualKeyCode
  }
}

const keyAliases: Record<string, string> = {
  Esc: 'Escape',
  Return: 'Enter',
  ' ': 'Space',
  Spacebar: 'Space'
}

function actionTimeoutMs(
  args: BrowserActionArgs,
  defaultTimeout = DEFAULT_PAGE_READY_TIMEOUT_MS
): number {
  const requested =
    typeof args.timeout_ms === 'number' && Number.isFinite(args.timeout_ms)
      ? args.timeout_ms
      : defaultTimeout
  return Math.max(100, Math.min(120000, Math.floor(requested)))
}

function shouldWaitAfterAction(args: BrowserActionArgs): boolean {
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

function withPageReady(
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

function withTopLevelTab(
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

function errorMessage(error: unknown): string {
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

function forgetActiveTab(tabId: number): void {
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

type TabLoadWatcher = {
  wait(options?: { noLoadTimeoutMs?: number }): Promise<Record<string, unknown>>
  cancel(): void
}

function createTabLoadWatcher(
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
    if (!matchesNavigation(details, tabId, frameId, expectedUrl)) {
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
    if (!matchesNavigation(details, tabId, frameId, expectedUrl)) {
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
      const currentMatches = urlMatchesExpected(current?.url || '', expectedUrl)
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

async function waitForTabReady(
  chromeApi: ChromeApi,
  tabId: number,
  args: BrowserActionArgs,
  loadWatcher = createTabLoadWatcher(chromeApi, tabId, actionTimeoutMs(args), args),
  waitOptions?: { noLoadTimeoutMs?: number }
): Promise<Record<string, unknown>> {
  try {
    const load = await loadWatcher.wait(waitOptions)
    const loaded = load.loaded !== false
    const network = loaded
      ? await waitForNetworkIdleBestEffort(chromeApi, tabId, args)
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

async function waitForTabReadyIfLoading(
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
      noLoadTimeoutMs: options?.noLoadTimeoutMs
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
  args: BrowserActionArgs
): Promise<Record<string, unknown>> {
  if (!chromeApi.debugger?.onEvent) {
    return { skipped: true, reason: 'debugger event API unavailable' }
  }
  try {
    return (await waitForNetworkIdle(chromeApi, tabId, args)) as Record<string, unknown>
  } catch (error) {
    return { skipped: true, error: errorMessage(error) }
  }
}

async function waitForPageSettleAfterAction(
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

function resolveInputTarget(args: BrowserActionArgs): Record<string, unknown> {
  function visible(element: Element): boolean {
    const style = window.getComputedStyle(element)
    const rect = element.getBoundingClientRect()
    return (
      style.visibility !== 'hidden' && style.display !== 'none' && rect.width > 0 && rect.height > 0
    )
  }

  function interactable(element: Element): boolean {
    if (!visible(element)) {
      return false
    }
    const rect = element.getBoundingClientRect()
    const hit = element.ownerDocument.elementFromPoint(
      rect.left + Math.max(1, rect.width) / 2,
      rect.top + Math.max(1, rect.height) / 2
    )
    if (!hit) {
      return false
    }
    if (hit === element || element.contains(hit)) {
      return true
    }
    const label = hit.closest('label')
    return (
      label instanceof HTMLLabelElement && (label.control === element || label.contains(element))
    )
  }

  function editableTextInput(element: Element): boolean {
    if (element instanceof HTMLTextAreaElement) {
      return !element.readOnly && !element.disabled
    }
    if (element instanceof HTMLInputElement) {
      const type = (element.getAttribute('type') || 'text').toLowerCase()
      return (
        !element.readOnly &&
        !element.disabled &&
        ['email', 'number', 'password', 'search', 'tel', 'text', 'url'].includes(type)
      )
    }
    return element instanceof HTMLElement && element.isContentEditable
  }

  function preferredMatch(elements: Element[]): Element | null {
    if (args.action === 'type_text') {
      return (
        elements.find((element) => editableTextInput(element) && visible(element)) ||
        elements.find((element) => visible(element)) ||
        elements[0] ||
        null
      )
    }
    return (
      elements.find((element) => interactable(element)) ||
      elements.find((element) => visible(element)) ||
      elements[0] ||
      null
    )
  }

  function deepQuerySelector(
    root: Document | ShadowRoot | Element,
    selector: string
  ): Element | null {
    const direct = preferredMatch(Array.from(root.querySelectorAll(selector)))
    if (direct) {
      return direct
    }
    for (const element of Array.from(root.querySelectorAll('*'))) {
      const shadowRoot = element instanceof HTMLElement ? element.shadowRoot : null
      if (shadowRoot) {
        const found = deepQuerySelector(shadowRoot, selector)
        if (found) {
          return found
        }
      }
      const frameDocument = childFrameDocument(element)
      if (frameDocument) {
        const frameFound = deepQuerySelector(frameDocument, selector)
        if (frameFound) {
          return frameFound
        }
      }
    }
    return null
  }

  function childFrameDocument(element: Element): Document | null {
    if (!(element instanceof HTMLIFrameElement)) {
      return null
    }
    try {
      return element.contentDocument || element.contentWindow?.document || null
    } catch (_error) {
      return null
    }
  }

  function box(element: Element): Record<string, number> {
    const rect = element.getBoundingClientRect()
    return {
      x: rect.x,
      y: rect.y,
      width: rect.width,
      height: rect.height,
      top: rect.top,
      right: rect.right,
      bottom: rect.bottom,
      left: rect.left
    }
  }

  function label(element: Element): string {
    return String(
      element.getAttribute('aria-label') ||
        element.getAttribute('title') ||
        element.getAttribute('placeholder') ||
        (element instanceof HTMLElement ? element.innerText : '') ||
        element.textContent ||
        element.tagName
    ).slice(0, 240)
  }

  let element: Element | null = null
  if (args.selector) {
    element = deepQuerySelector(document, args.selector)
    if (!element) {
      throw new Error(`selector not found: ${args.selector}`)
    }
  } else if (
    typeof args.x === 'number' &&
    typeof args.y === 'number' &&
    Number.isFinite(args.x) &&
    Number.isFinite(args.y)
  ) {
    element = document.elementFromPoint(args.x, args.y)
  } else if (args.action === 'type_text') {
    const active = document.activeElement
    const frameActive = active ? childFrameDocument(active)?.activeElement : null
    element =
      frameActive && frameActive !== frameActive.ownerDocument.body
        ? frameActive
        : active && active !== document.body && active !== document.documentElement
          ? active
          : null
  }

  if (!element) {
    if (args.action === 'type_text') {
      throw new Error('type_text requires selector or an active editable element')
    }
    throw new Error('selector or x/y coordinates are required')
  }

  element.scrollIntoView({ block: 'center', inline: 'center' })
  const rect = element.getBoundingClientRect()
  const useExplicitPoint = !args.selector
  const x =
    useExplicitPoint && typeof args.x === 'number' && Number.isFinite(args.x)
      ? args.x
      : rect.left + Math.max(1, rect.width) / 2
  const y =
    useExplicitPoint && typeof args.y === 'number' && Number.isFinite(args.y)
      ? args.y
      : rect.top + Math.max(1, rect.height) / 2

  if (args.action === 'type_text') {
    if (!editableTextInput(element)) {
      return { native_text_input: false, reason: 'target is not a native text input' }
    }
    return {
      native_text_input: true,
      x,
      y,
      selector: args.selector || null,
      label: label(element),
      bounding_box: box(element)
    }
  }

  return { x, y, label: label(element), bounding_box: box(element) }
}

async function activateTab(chromeApi: ChromeApi, tabId: number): Promise<ChromeTabInfo> {
  const tab = (await chromeApi.tabs.update(tabId, { active: true })) as ChromeTabInfo
  await focusWindow(chromeApi, tab.windowId).catch(() => undefined)
  rememberActiveTab(tab)
  return tab
}

async function focusWindow(chromeApi: ChromeApi, windowId?: number): Promise<void> {
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

function positiveInteger(value: unknown): number | null {
  return typeof value === 'number' && Number.isInteger(value) && value > 0 ? value : null
}

function nonNegativeInteger(value: unknown): number | null {
  return typeof value === 'number' && Number.isInteger(value) && value >= 0 ? value : null
}

function requirePositiveInteger(value: unknown, message: string): number {
  const integer = positiveInteger(value)
  if (!integer) {
    throw new Error(message)
  }
  return integer
}

function normalizeOptionalText(value: unknown): string | undefined {
  return typeof value === 'string' && value.trim() ? value.trim() : undefined
}

export function pageActionDispatcher(
  args: BrowserActionArgs
): BrowserActionResult | Promise<BrowserActionResult> {
  const maxText = 12000
  const defaultHtmlLimit = 200000

  function truncate(value: unknown, limit = maxChars(maxText)): string {
    const text = String(value || '')
    return text.length > limit
      ? `${text.slice(0, limit)}\n[truncated ${text.length - limit} chars]`
      : text
  }

  function maxChars(defaultLimit: number): number {
    const requested =
      typeof args.max_chars === 'number' && Number.isFinite(args.max_chars)
        ? args.max_chars
        : defaultLimit
    return Math.max(1000, Math.min(500000, Math.floor(requested)))
  }

  function timeoutMs(defaultTimeout: number): number {
    const requested =
      typeof args.timeout_ms === 'number' && Number.isFinite(args.timeout_ms)
        ? args.timeout_ms
        : defaultTimeout
    return Math.max(100, Math.min(120000, Math.floor(requested)))
  }

  function cssEscape(value: string): string {
    if (window.CSS && CSS.escape) {
      return CSS.escape(value)
    }
    return String(value).replace(/[^a-zA-Z0-9_-]/g, '\\$&')
  }

  function visible(element: Element): boolean {
    const style = window.getComputedStyle(element)
    const rect = element.getBoundingClientRect()
    return (
      style.visibility !== 'hidden' && style.display !== 'none' && rect.width > 0 && rect.height > 0
    )
  }

  function cssPath(element: Element | null): string {
    if (!element) {
      return ''
    }
    if (element.id) {
      return `#${cssEscape(element.id)}`
    }

    const parts: string[] = []
    let current: Element | null = element
    while (current && current.nodeType === Node.ELEMENT_NODE && parts.length < 5) {
      let part = current.nodeName.toLowerCase()
      if (current.classList.length) {
        part += `.${Array.from(current.classList).slice(0, 2).map(cssEscape).join('.')}`
      }
      const parent: Element | null = current.parentElement
      if (parent) {
        const currentNodeName = current.nodeName
        const siblings = Array.from(parent.children).filter(
          (sibling: Element) => sibling.nodeName === currentNodeName
        )
        if (siblings.length > 1) {
          part += `:nth-of-type(${siblings.indexOf(current) + 1})`
        }
      }
      parts.unshift(part)
      current = parent
    }
    return parts.join(' > ')
  }

  function deepQuerySelector(
    root: Document | ShadowRoot | Element,
    selector: string
  ): Element | null {
    const direct = preferredMatch(Array.from(root.querySelectorAll(selector)))
    if (direct) {
      return direct
    }

    for (const element of Array.from(root.querySelectorAll('*'))) {
      const shadowRoot = element instanceof HTMLElement ? element.shadowRoot : null
      if (shadowRoot) {
        const found = deepQuerySelector(shadowRoot, selector)
        if (found) {
          return found
        }
      }
      const frameDocument = childFrameDocument(element)
      if (frameDocument) {
        const frameFound = deepQuerySelector(frameDocument, selector)
        if (frameFound) {
          return frameFound
        }
      }
    }
    return null
  }

  function childFrameDocument(element: Element): Document | null {
    if (!(element instanceof HTMLIFrameElement)) {
      return null
    }
    try {
      return element.contentDocument || element.contentWindow?.document || null
    } catch (_error) {
      return null
    }
  }

  function interactable(element: Element): boolean {
    if (!visible(element)) {
      return false
    }
    const rect = element.getBoundingClientRect()
    const hit = element.ownerDocument.elementFromPoint(
      rect.left + Math.max(1, rect.width) / 2,
      rect.top + Math.max(1, rect.height) / 2
    )
    if (!hit) {
      return false
    }
    if (hit === element || element.contains(hit)) {
      return true
    }
    const label = hit.closest('label')
    return (
      label instanceof HTMLLabelElement && (label.control === element || label.contains(element))
    )
  }

  function preferredMatch(elements: Element[]): Element | null {
    return (
      elements.find((element) => interactable(element)) ||
      elements.find((element) => visible(element)) ||
      elements[0] ||
      null
    )
  }

  function queryRequired(selector?: string): Element {
    if (!selector) {
      throw new Error('selector is required')
    }
    const element = deepQuerySelector(document, selector)
    if (!element) {
      throw new Error(`selector not found: ${selector}`)
    }
    return element
  }

  function elementLabel(element: Element): string {
    return truncate(
      element.getAttribute('aria-label') ||
        element.getAttribute('title') ||
        element.getAttribute('placeholder') ||
        (element instanceof HTMLElement ? element.innerText : '') ||
        ('value' in element ? String(element.value) : '') ||
        element.textContent ||
        element.tagName,
      240
    )
  }

  function elementAttributes(element: Element): Record<string, string> {
    return Object.fromEntries(Array.from(element.attributes).map((attr) => [attr.name, attr.value]))
  }

  function elementBox(element: Element): Record<string, number> {
    const rect = element.getBoundingClientRect()
    return {
      x: rect.x,
      y: rect.y,
      width: rect.width,
      height: rect.height,
      top: rect.top,
      right: rect.right,
      bottom: rect.bottom,
      left: rect.left
    }
  }

  function elementInfo(element: Element): Record<string, unknown> {
    return {
      selector: cssPath(element),
      tag: element.tagName.toLowerCase(),
      id: element.id || null,
      classes: Array.from(element.classList),
      role: element.getAttribute('role'),
      name: element.getAttribute('name'),
      type: element.getAttribute('type'),
      label: elementLabel(element),
      text: truncate(
        element instanceof HTMLElement ? element.innerText : element.textContent,
        2000
      ),
      value: 'value' in element ? String(element.value) : null,
      attributes: elementAttributes(element),
      bounding_box: elementBox(element),
      visible: visible(element),
      disabled: 'disabled' in element ? Boolean(element.disabled) : false
    }
  }

  function pointFromArgs(prefix = ''): { x: number; y: number } | null {
    const x = prefix === 'to_' ? args.to_x : args.x
    const y = prefix === 'to_' ? args.to_y : args.y
    return typeof x === 'number' &&
      typeof y === 'number' &&
      Number.isFinite(x) &&
      Number.isFinite(y)
      ? { x, y }
      : null
  }

  function elementFromSelectorOrPoint(selector = args.selector): Element {
    if (selector) {
      return queryRequired(selector)
    }
    const point = pointFromArgs()
    if (!point) {
      throw new Error('selector or x/y coordinates are required')
    }
    const element = document.elementFromPoint(point.x, point.y)
    if (!element) {
      throw new Error(`no element at coordinates: ${point.x},${point.y}`)
    }
    return element
  }

  function editableElement(selector = args.selector): Element {
    if (selector) {
      return queryRequired(selector)
    }
    const active = document.activeElement
    const frameActive = active ? childFrameDocument(active)?.activeElement : null
    if (frameActive && frameActive !== frameActive.ownerDocument.body) {
      return frameActive
    }
    if (active && active !== document.body && active !== document.documentElement) {
      return active
    }
    throw new Error('type_text requires selector or an active editable element')
  }

  function selectElement(selector = args.selector): HTMLSelectElement {
    const element = queryRequired(selector)
    if (element instanceof HTMLSelectElement) {
      return element
    }
    const frameDocument = childFrameDocument(element)
    const nestedSelect = frameDocument?.querySelector('select')
    if (nestedSelect instanceof HTMLSelectElement) {
      return nestedSelect
    }
    throw new Error(`selector is not a select element: ${selector}`)
  }

  function centerOf(element: Element): { x: number; y: number } {
    const rect = element.getBoundingClientRect()
    return { x: rect.left + rect.width / 2, y: rect.top + rect.height / 2 }
  }

  function pointerPoint(element: Element): { x: number; y: number } {
    return args.selector ? centerOf(element) : pointFromArgs() || centerOf(element)
  }

  function dispatchMouse(element: Element, type: string, point: { x: number; y: number }): void {
    element.dispatchEvent(
      new MouseEvent(type, {
        bubbles: true,
        cancelable: true,
        clientX: point.x,
        clientY: point.y,
        view: window
      })
    )
  }

  function setElementValue(
    element: HTMLInputElement | HTMLTextAreaElement | HTMLSelectElement,
    value: string
  ): void {
    const prototype = Object.getPrototypeOf(element)
    const descriptor = Object.getOwnPropertyDescriptor(prototype, 'value')
    if (descriptor?.set) {
      descriptor.set.call(element, value)
    } else {
      element.value = value
    }
  }

  function scrollBehavior(): ScrollBehavior {
    return args.behavior === 'auto' || args.behavior === 'smooth' || args.behavior === 'instant'
      ? args.behavior
      : 'smooth'
  }

  function structuredData(): Record<string, unknown> {
    const jsonLd = Array.from(document.querySelectorAll('script[type="application/ld+json"]'))
      .slice(0, 20)
      .map((element) => {
        const text = element.textContent || ''
        try {
          return JSON.parse(text)
        } catch (_error) {
          return { parse_error: true, text: truncate(text, 2000) }
        }
      })
    const meta = Array.from(document.querySelectorAll('meta[name], meta[property]'))
      .slice(0, 80)
      .map((element) => ({
        name: element.getAttribute('name') || element.getAttribute('property'),
        content: truncate(element.getAttribute('content'), 1000)
      }))
    const tables = Array.from(document.querySelectorAll('table'))
      .filter(visible)
      .slice(0, 10)
      .map((table) => {
        const rows = Array.from(table.querySelectorAll('tr')).slice(0, 50)
        return {
          selector: cssPath(table),
          caption: truncate(table.querySelector('caption')?.textContent, 500),
          rows: rows.map((row) =>
            Array.from(row.querySelectorAll('th, td'))
              .slice(0, 20)
              .map((cell) => truncate(cell.textContent, 500))
          )
        }
      })
    const lists = Array.from(document.querySelectorAll('ul, ol'))
      .filter(visible)
      .slice(0, 20)
      .map((list) => ({
        selector: cssPath(list),
        ordered: list.tagName.toLowerCase() === 'ol',
        items: Array.from(list.children)
          .slice(0, 40)
          .map((item) => truncate(item.textContent, 500))
      }))

    return { url: location.href, title: document.title, json_ld: jsonLd, meta, tables, lists }
  }

  function findMatches(): Record<string, unknown> {
    const query = String(args.query || '')
      .trim()
      .toLowerCase()
    if (!query) {
      throw new Error('find_in_page requires query')
    }
    const matches = Array.from(document.body?.querySelectorAll('*') || [])
      .filter(
        (element) => visible(element) && (element.textContent || '').toLowerCase().includes(query)
      )
      .slice(0, 80)
      .map((element) => {
        if (args.highlight && element instanceof HTMLElement) {
          element.dataset.andaFindHighlight = 'true'
          element.style.outline = '2px solid #f59e0b'
          element.style.outlineOffset = '2px'
        }
        return {
          selector: cssPath(element),
          label: elementLabel(element),
          text: truncate(element.textContent, 800),
          bounding_box: elementBox(element)
        }
      })
    return {
      query: args.query,
      count: matches.length,
      highlighted: Boolean(args.highlight),
      matches
    }
  }

  function serializeForResult(value: unknown, depth = 0): unknown {
    if (
      value === null ||
      value === undefined ||
      typeof value === 'string' ||
      typeof value === 'number' ||
      typeof value === 'boolean'
    ) {
      return value
    }
    if (value instanceof Element) {
      return elementInfo(value)
    }
    if (value instanceof Error) {
      return { name: value.name, message: value.message, stack: value.stack }
    }
    if (depth > 4) {
      return String(value)
    }
    if (Array.isArray(value)) {
      return value.slice(0, 200).map((item) => serializeForResult(item, depth + 1))
    }
    if (typeof value === 'object') {
      return Object.fromEntries(
        Object.entries(value as Record<string, unknown>)
          .slice(0, 200)
          .map(([key, entry]) => [key, serializeForResult(entry, depth + 1)])
      )
    }
    return String(value)
  }

  function serializeScriptResult(value: unknown): unknown {
    const serialized = serializeForResult(value)
    return serialized === undefined ? null : serialized
  }

  function compileScriptExpression(code: string): ((args: BrowserActionArgs) => unknown) | null {
    const expression = code.trim().replace(/;+$/, '')
    if (!expression) {
      return null
    }

    try {
      return new Function('args', `"use strict"; return (${expression})`) as (
        args: BrowserActionArgs
      ) => unknown
    } catch (error) {
      if (error instanceof SyntaxError) {
        return null
      }
      throw error
    }
  }

  function scriptWithImplicitReturn(code: string): string | null {
    const body = code.trim().replace(/;+$/, '')
    if (!body) {
      return null
    }
    const splitAt = lastTopLevelSemicolon(body)
    if (splitAt < 0) {
      return null
    }
    const prefix = body.slice(0, splitAt + 1)
    const tail = body.slice(splitAt + 1).trim()
    if (!tail || !canImplicitlyReturn(tail)) {
      return null
    }
    return `${prefix}\nreturn (${tail});`
  }

  function canImplicitlyReturn(statement: string): boolean {
    return !/^(break|catch|class|const|continue|do|export|finally|for|function|if|import|let|return|switch|throw|try|var|while)\b/.test(
      statement
    )
  }

  function lastTopLevelSemicolon(code: string): number {
    let quote: string | null = null
    let escaped = false
    let lineComment = false
    let blockComment = false
    let parenDepth = 0
    let braceDepth = 0
    let bracketDepth = 0
    let last = -1

    for (let index = 0; index < code.length; index += 1) {
      const char = code[index]
      const next = code[index + 1]

      if (lineComment) {
        if (char === '\n' || char === '\r') {
          lineComment = false
        }
        continue
      }
      if (blockComment) {
        if (char === '*' && next === '/') {
          blockComment = false
          index += 1
        }
        continue
      }
      if (quote) {
        if (escaped) {
          escaped = false
        } else if (char === '\\') {
          escaped = true
        } else if (char === quote) {
          quote = null
        }
        continue
      }

      if (char === '/' && next === '/') {
        lineComment = true
        index += 1
        continue
      }
      if (char === '/' && next === '*') {
        blockComment = true
        index += 1
        continue
      }
      if (char === '"' || char === "'" || char === '`') {
        quote = char
        continue
      }
      if (char === '(') {
        parenDepth += 1
      } else if (char === ')') {
        parenDepth = Math.max(0, parenDepth - 1)
      } else if (char === '{') {
        braceDepth += 1
      } else if (char === '}') {
        braceDepth = Math.max(0, braceDepth - 1)
      } else if (char === '[') {
        bracketDepth += 1
      } else if (char === ']') {
        bracketDepth = Math.max(0, bracketDepth - 1)
      } else if (char === ';' && parenDepth === 0 && braceDepth === 0 && bracketDepth === 0) {
        last = index
      }
    }

    return last
  }

  function compileScriptBody(code: string): (args: BrowserActionArgs) => unknown {
    const implicitReturn = scriptWithImplicitReturn(code)
    return new Function('args', `"use strict";\n${implicitReturn || code}`) as (
      args: BrowserActionArgs
    ) => unknown
  }

  function isPromiseLike(value: unknown): value is PromiseLike<unknown> {
    return (
      value !== null &&
      (typeof value === 'object' || typeof value === 'function') &&
      typeof (value as { then?: unknown }).then === 'function'
    )
  }

  function scriptResult(value: unknown): Record<string, unknown> {
    return { executed: true, result: serializeScriptResult(value) }
  }

  function executeJavaScript(
    code: string
  ): Record<string, unknown> | Promise<Record<string, unknown>> {
    const execute = compileScriptExpression(code) || compileScriptBody(code)
    const result = execute(args)
    return isPromiseLike(result) ? Promise.resolve(result).then(scriptResult) : scriptResult(result)
  }

  function copyText(text: string): Record<string, unknown> | Promise<Record<string, unknown>> {
    const textarea = document.createElement('textarea')
    textarea.value = text
    textarea.style.position = 'fixed'
    textarea.style.opacity = '0'
    document.body.appendChild(textarea)
    textarea.focus()
    textarea.select()
    const copied = document.execCommand('copy')
    textarea.remove()
    if (copied) {
      return { copied: true, length: text.length, method: 'execCommand' }
    }
    if (navigator.clipboard?.writeText) {
      let timer: ReturnType<typeof setTimeout> | null = null
      const timeout = new Promise<never>((_resolve, reject) => {
        timer = setTimeout(() => reject(new Error('clipboard write timed out')), 2000)
      })
      const write = navigator.clipboard
        .writeText(text)
        .then(() => ({ copied: true, length: text.length, method: 'clipboard' }))
        .finally(() => {
          if (timer) {
            clearTimeout(timer)
          }
        })
      return Promise.race([write, timeout])
    }
    throw new Error('copy command failed')
  }

  switch (args.action) {
    case 'annotate_viewport': {
      document.getElementById('__anda_viewport_annotations')?.remove()
      const container = document.createElement('div')
      container.id = '__anda_viewport_annotations'
      container.style.position = 'fixed'
      container.style.inset = '0'
      container.style.pointerEvents = 'none'
      container.style.zIndex = '2147483647'
      const candidates = Array.from(
        document.querySelectorAll(
          "a[href], button, input, textarea, select, summary, [role='button'], [role='link'], [role='menuitem'], [tabindex]:not([tabindex='-1'])"
        )
      )
        .filter(visible)
        .filter((element) => {
          const rect = element.getBoundingClientRect()
          return (
            rect.bottom >= 0 &&
            rect.right >= 0 &&
            rect.top <= innerHeight &&
            rect.left <= innerWidth
          )
        })
        .slice(0, 120)
      const markers = candidates.map((element, index) => {
        const markerId = index + 1
        const rect = element.getBoundingClientRect()
        const badge = document.createElement('div')
        badge.textContent = String(markerId)
        badge.style.position = 'fixed'
        badge.style.left = `${Math.max(0, rect.left)}px`
        badge.style.top = `${Math.max(0, rect.top)}px`
        badge.style.minWidth = '18px'
        badge.style.height = '18px'
        badge.style.padding = '0 5px'
        badge.style.borderRadius = '9px'
        badge.style.background = '#f59e0b'
        badge.style.color = '#111827'
        badge.style.border = '1px solid #111827'
        badge.style.font = '700 12px/18px system-ui, sans-serif'
        badge.style.textAlign = 'center'
        badge.style.boxShadow = '0 1px 4px rgba(0,0,0,0.35)'
        container.appendChild(badge)
        return {
          marker: markerId,
          selector: cssPath(element),
          label: elementLabel(element),
          tag: element.tagName.toLowerCase(),
          bounding_box: elementBox(element)
        }
      })
      document.body.appendChild(container)
      return { annotated: true, count: markers.length, markers }
    }
    case 'clear_annotations': {
      const existing = document.getElementById('__anda_viewport_annotations')
      existing?.remove()
      return { cleared: Boolean(existing) }
    }
    case 'snapshot': {
      const links = args.include_links
        ? Array.from(document.querySelectorAll('a[href]'))
            .filter(visible)
            .slice(0, 80)
            .map((element) => ({
              text: elementLabel(element),
              href: element instanceof HTMLAnchorElement ? element.href : '',
              selector: cssPath(element)
            }))
        : []
      const forms = args.include_forms
        ? Array.from(document.querySelectorAll("input, textarea, select, button, [role='button']"))
            .filter(visible)
            .slice(0, 120)
            .map((element) => ({
              tag: element.tagName.toLowerCase(),
              type: element.getAttribute('type') || element.getAttribute('role') || null,
              name: element.getAttribute('name') || null,
              label: elementLabel(element),
              selector: cssPath(element)
            }))
        : []
      return {
        url: location.href,
        title: document.title,
        selection: String(window.getSelection ? window.getSelection() : ''),
        active_element: document.activeElement ? elementInfo(document.activeElement) : null,
        viewport: { width: window.innerWidth, height: window.innerHeight },
        scroll: {
          x: window.scrollX,
          y: window.scrollY,
          max_y: document.documentElement.scrollHeight
        },
        text: truncate(document.body ? document.body.innerText : ''),
        links,
        forms
      }
    }
    case 'extract_text': {
      const element = args.selector ? queryRequired(args.selector) : document.body
      return {
        selector: args.selector || 'body',
        text: truncate(
          element instanceof HTMLElement
            ? element.innerText || element.textContent
            : element?.textContent
        )
      }
    }
    case 'get_full_page_html': {
      return {
        url: location.href,
        title: document.title,
        html: truncate(document.documentElement?.outerHTML || '', maxChars(defaultHtmlLimit))
      }
    }
    case 'get_structured_data': {
      return structuredData()
    }
    case 'get_element_info': {
      const element = queryRequired(args.selector)
      return elementInfo(element)
    }
    case 'get_viewport_size': {
      return {
        viewport: { width: window.innerWidth, height: window.innerHeight },
        screen: { width: window.screen.width, height: window.screen.height },
        device_pixel_ratio: window.devicePixelRatio,
        scroll: {
          x: window.scrollX,
          y: window.scrollY,
          max_x: document.documentElement.scrollWidth,
          max_y: document.documentElement.scrollHeight
        }
      }
    }
    case 'find_in_page': {
      return findMatches()
    }
    case 'wait_for_element': {
      const timeout = timeoutMs(10000)
      const selector = args.selector
      if (!selector) {
        throw new Error('wait_for_element requires selector')
      }
      const existing = deepQuerySelector(document, selector)
      if (existing && visible(existing)) {
        return { found: true, selector, element: elementInfo(existing) }
      }
      return new Promise((resolve, reject) => {
        const observer = new MutationObserver(() => {
          const element = deepQuerySelector(document, selector)
          if (element && visible(element)) {
            clearTimeout(timer)
            observer.disconnect()
            resolve({ found: true, selector, element: elementInfo(element) })
          }
        })
        const timer = setTimeout(() => {
          observer.disconnect()
          reject(new Error(`selector not found before timeout: ${selector}`))
        }, timeout)
        observer.observe(document.documentElement, {
          childList: true,
          subtree: true,
          attributes: true
        })
      })
    }
    case 'read_selection': {
      return { selection: String(window.getSelection ? window.getSelection() : '') }
    }
    case 'click': {
      const element = elementFromSelectorOrPoint()
      element.scrollIntoView({ block: 'center', inline: 'center' })
      const point = pointerPoint(element)
      dispatchMouse(element, 'mouseover', point)
      dispatchMouse(element, 'mousemove', point)
      dispatchMouse(element, 'mousedown', point)
      dispatchMouse(element, 'mouseup', point)
      dispatchMouse(element, 'click', point)
      return { clicked: true, selector: args.selector, label: elementLabel(element) }
    }
    case 'hover': {
      const element = elementFromSelectorOrPoint()
      element.scrollIntoView({ block: 'center', inline: 'center' })
      const point = pointerPoint(element)
      dispatchMouse(element, 'mouseover', point)
      dispatchMouse(element, 'mouseenter', point)
      dispatchMouse(element, 'mousemove', point)
      return { hovered: true, selector: args.selector, label: elementLabel(element) }
    }
    case 'type_text': {
      const element = editableElement()
      element.scrollIntoView({ block: 'center', inline: 'center' })
      if (element instanceof HTMLElement) {
        element.focus()
      }
      if (element instanceof HTMLElement && element.isContentEditable) {
        element.textContent = args.text || ''
      } else if (
        element instanceof HTMLInputElement ||
        element instanceof HTMLTextAreaElement ||
        element instanceof HTMLSelectElement
      ) {
        setElementValue(element, args.text || '')
      } else {
        throw new Error(
          args.selector
            ? `selector is not editable: ${args.selector}`
            : 'active element is not editable'
        )
      }
      element.dispatchEvent(
        new InputEvent('input', {
          bubbles: true,
          inputType: 'insertText',
          data: args.text || ''
        })
      )
      element.dispatchEvent(new Event('change', { bubbles: true }))
      return {
        typed: true,
        selector: args.selector || cssPath(element),
        active_element: !args.selector,
        length: String(args.text || '').length
      }
    }
    case 'select_dropdown': {
      const element = selectElement()
      element.scrollIntoView({ block: 'center', inline: 'center' })
      const value = String(args.value || '')
      const option = Array.from(element.options).find(
        (option) => option.value === value || option.label === value || option.text === value
      )
      if (!option) {
        throw new Error(`select option not found: ${value}`)
      }
      setElementValue(element, option.value)
      element.dispatchEvent(new Event('input', { bubbles: true }))
      element.dispatchEvent(new Event('change', { bubbles: true }))
      return { selected: true, selector: args.selector, value: option.value, label: option.label }
    }
    case 'press_key': {
      const target = document.activeElement || document.body
      const key = args.key || 'Enter'
      target.dispatchEvent(new KeyboardEvent('keydown', { key, bubbles: true }))
      target.dispatchEvent(new KeyboardEvent('keyup', { key, bubbles: true }))
      return { pressed: true, key }
    }
    case 'scroll': {
      const amount =
        typeof args.amount === 'number' && Number.isFinite(args.amount) ? args.amount : 700
      window.scrollBy({ top: amount, behavior: 'smooth' })
      return { scrolled: true, amount, scroll_y: window.scrollY }
    }
    case 'scroll_to': {
      if (args.selector) {
        const element = queryRequired(args.selector)
        element.scrollIntoView({ block: 'center', inline: 'center', behavior: scrollBehavior() })
        return { scrolled_to: true, selector: args.selector, label: elementLabel(element) }
      }
      const point = pointFromArgs()
      if (!point) {
        throw new Error('scroll_to requires selector or x/y coordinates')
      }
      window.scrollTo({ left: point.x, top: point.y, behavior: scrollBehavior() })
      return {
        scrolled_to: true,
        x: point.x,
        y: point.y,
        scroll_x: window.scrollX,
        scroll_y: window.scrollY
      }
    }
    case 'drag_and_drop': {
      const source = queryRequired(args.from_selector)
      const target = args.to_selector
        ? queryRequired(args.to_selector)
        : (() => {
            const point = pointFromArgs('to_')
            if (!point) {
              throw new Error('drag_and_drop requires to_selector or to_x/to_y')
            }
            const element = document.elementFromPoint(point.x, point.y)
            if (!element) {
              throw new Error(`no drop target at coordinates: ${point.x},${point.y}`)
            }
            return element
          })()
      source.scrollIntoView({ block: 'center', inline: 'center' })
      const sourcePoint = centerOf(source)
      const targetPoint = args.to_selector
        ? centerOf(target)
        : pointFromArgs('to_') || centerOf(target)
      const dataTransfer = new DataTransfer()
      source.dispatchEvent(
        new DragEvent('dragstart', { bubbles: true, cancelable: true, dataTransfer })
      )
      dispatchMouse(source, 'mousedown', sourcePoint)
      dispatchMouse(target, 'mousemove', targetPoint)
      target.dispatchEvent(
        new DragEvent('dragenter', { bubbles: true, cancelable: true, dataTransfer })
      )
      target.dispatchEvent(
        new DragEvent('dragover', { bubbles: true, cancelable: true, dataTransfer })
      )
      target.dispatchEvent(new DragEvent('drop', { bubbles: true, cancelable: true, dataTransfer }))
      dispatchMouse(target, 'mouseup', targetPoint)
      source.dispatchEvent(
        new DragEvent('dragend', { bubbles: true, cancelable: true, dataTransfer })
      )
      return {
        dragged: true,
        from_selector: args.from_selector,
        to_selector: args.to_selector || cssPath(target),
        from: elementLabel(source),
        to: elementLabel(target)
      }
    }
    case 'copy_to_clipboard': {
      return copyText(String(args.text || ''))
    }
    case 'go_back': {
      history.back()
      return { went_back: true, url: location.href }
    }
    case 'go_forward': {
      history.forward()
      return { went_forward: true, url: location.href }
    }
    case 'execute_javascript': {
      const code = String(args.code || '')
      if (!code.trim()) {
        throw new Error('execute_javascript requires code')
      }
      return executeJavaScript(code)
    }
    default:
      throw new Error(`unsupported browser action: ${args.action}`)
  }
}
