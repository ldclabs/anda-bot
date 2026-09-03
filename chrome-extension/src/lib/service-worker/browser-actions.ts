import { getMessage } from '$lib/i18n'
import {
  cancelDownload,
  clearBrowserCache,
  deleteCookie,
  downloadFile,
  getCookies,
  listDownloads,
  listTabs,
  openDownload,
  setCookie
} from './browser-state'
import {
  actionTimeoutMs,
  activateTab,
  activeTab,
  focusWindow,
  forgetActiveTab,
  nonNegativeInteger,
  normalizeOptionalText,
  positiveInteger,
  rememberActiveTab,
  requirePositiveInteger,
  tabSummary
} from './browser-tabs'
import {
  captureScreenshotWithDebugger,
  dispatchNativeKey,
  dispatchNativePointerAction,
  dispatchNativeTextInput,
  executeJavaScriptWithDebugger,
  getAccessibilityTree,
  handleDialog,
  hasViewportOverride,
  printToPdf,
  uploadFile
} from './browser-debugger'
import {
  createTabLoadWatcher,
  shouldWaitAfterAction,
  waitForPageSettleAfterAction,
  waitForTabReady,
  waitForTabReadyIfLoading,
  withPageReady,
  withTopLevelTab,
  type TabLoadWatcher
} from './browser-navigation'
import { scriptExecutionMode, scriptExecutionWorld } from './browser-script'
import { pageActionDispatcher } from './page-scripts'
import type {
  BrowserActionArgs,
  BrowserActionResult,
  BrowserCommand,
  ChromeApi,
  ChromeTabInfo,
  ChromeWebNavigationFrame
} from './types'

type BrowserActionDependencies = {
  chromeApi: ChromeApi
}

const PRE_ACTION_LOADING_TIMEOUT_MS = 1_500
const ACTION_SETTLE_NO_LOAD_TIMEOUT_MS = 1_000

const LOCAL_FILE_ACCESS_DISABLED_ERROR_CODE = 'local_file_access_disabled'
const LOCAL_FILE_ACCESS_DISABLED_FALLBACK_MESSAGE =
  'Cannot navigate to a file URL without local file access. Enable "Allow access to file URLs" for the Anda browser extension in the browser extension details, then retry.'

class BrowserActionError extends Error {
  code: string

  constructor(code: string, message: string) {
    super(message)
    this.name = 'BrowserActionError'
    this.code = code
  }
}

function localFileAccessDisabledMessage(chromeApi: ChromeApi): string {
  return (
    getMessage('localFileAccessDisabled') ||
    chromeApi.i18n?.getMessage?.('localFileAccessDisabled') ||
    LOCAL_FILE_ACCESS_DISABLED_FALLBACK_MESSAGE
  )
}

function isFileSchemeUrl(url: unknown): boolean {
  return (normalizeOptionalText(url)?.toLowerCase() || '').startsWith('file://')
}

async function ensureLocalFileAccess(chromeApi: ChromeApi, url: unknown): Promise<void> {
  if (!isFileSchemeUrl(url)) {
    return
  }

  const extensionApi = chromeApi.extension
  if (!extensionApi?.isAllowedFileSchemeAccess) {
    return
  }

  const isAllowed = await new Promise<boolean>((resolve, reject) => {
    try {
      extensionApi.isAllowedFileSchemeAccess?.((allowedAccess) => {
        const lastError = chromeApi.runtime.lastError?.message
        if (lastError) {
          reject(new Error(lastError))
          return
        }
        resolve(Boolean(allowedAccess))
      })
    } catch (error) {
      reject(error)
    }
  })

  if (!isAllowed) {
    throw new BrowserActionError(
      LOCAL_FILE_ACCESS_DISABLED_ERROR_CODE,
      localFileAccessDisabledMessage(chromeApi)
    )
  }
}

/**
 * Actions that never target a page: they answer from browser state alone, so
 * they run before any tab is resolved or waited on.
 */
type BrowserStateAction = (
  chromeApi: ChromeApi,
  args: BrowserActionArgs
) => Promise<BrowserActionResult>

// Null-prototype so an action name like 'constructor' or 'toString' cannot
// resolve to an inherited Object member and be invoked as a handler.
const browserStateActions: Record<string, BrowserStateAction> = Object.assign(
  Object.create(null) as Record<string, BrowserStateAction>,
  {
    list_tabs: listTabs,
    download: downloadFile,
    list_downloads: listDownloads,
    cancel_download: cancelDownload,
    open_download: openDownload,
    get_cookies: getCookies,
    set_cookie: setCookie,
    delete_cookie: deleteCookie,
    clear_browser_cache: clearBrowserCache,
    get_frames: getNavigationFrames,
    get_current_tab: async (chromeApi: ChromeApi) => ({
      tab: tabSummary(await activeTab(chromeApi))
    }),
    launch_browser: async () => ({
      launched: false,
      connected: true,
      reason: 'browser is already running'
    })
  }
)

/**
 * Runs one browser command from the agent.
 *
 * Commands fall into three groups, handled in order:
 *
 * 1. Browser-state actions ({@link browserStateActions}) answer immediately.
 * 2. Navigating actions (`open_tab`, `switch_tab`, `navigate`, `reload`,
 *    `go_back`, `go_forward`, `close_tab`) drive the tab themselves and wait
 *    for the resulting load before answering.
 * 3. Page actions run against the active tab. Each waits for a
 *    still-loading page first (best effort), performs the action through the
 *    debugger where one is attachable and through `scripting.executeScript`
 *    otherwise, then reports the page-ready state it settled into.
 *
 * Every result for groups 2 and 3 carries the tab summary and, when the action
 * could navigate, a `page_ready` block describing what the page settled to.
 */
export async function executeBrowserAction(
  command: BrowserCommand,
  deps: BrowserActionDependencies
): Promise<BrowserActionResult> {
  const { chromeApi } = deps
  const args = command.args || {}
  const action = args.action || ''

  const browserStateAction = browserStateActions[action]
  if (browserStateAction) {
    return browserStateAction(chromeApi, args)
  }

  if (action === 'open_tab') {
    const url = normalizeOptionalText(args.url)
    await ensureLocalFileAccess(chromeApi, url)
    const active = args.active ?? true
    const tab = await chromeApi.tabs.create({
      url,
      active,
      windowId: positiveInteger(args.window_id) || undefined
    })
    if (active) {
      rememberActiveTab(tab)
    }
    const pageReady = tab.id ? await waitForTabReady(chromeApi, tab.id, args) : null
    return withTopLevelTab({ opened: true }, pageReady, tab)
  }

  if (action === 'switch_tab') {
    const tabId = requirePositiveInteger(args.tab_id, 'switch_tab requires tab_id')
    const tab = await activateTab(chromeApi, tabId)
    const pageReady = await waitForTabReadyIfLoading(chromeApi, tabId, args)
    return withTopLevelTab({ switched: true }, pageReady, tab)
  }

  if (action === 'close_tab') {
    const tabId = requirePositiveInteger(args.tab_id, 'close_tab requires tab_id')
    await chromeApi.tabs.remove(tabId)
    forgetActiveTab(tabId)
    return { closed: true, tab_id: tabId }
  }

  if (action === 'navigate') {
    return navigate(chromeApi, args)
  }

  if (action === 'reload' || action === 'go_back' || action === 'go_forward') {
    return historyAction(chromeApi, args, action)
  }

  return executePageAction(chromeApi, args)
}

/** Points a tab at `args.url`, creating one when there is no target tab. */
async function navigate(
  chromeApi: ChromeApi,
  args: BrowserActionArgs
): Promise<BrowserActionResult> {
  const url = normalizeOptionalText(args.url)
  if (!url) {
    throw new Error('navigate requires url')
  }
  await ensureLocalFileAccess(chromeApi, url)
  const tab = await tabForAction(chromeApi, args)
  const active = args.active ?? true

  if (!tab?.id) {
    const created = await chromeApi.tabs.create({
      url,
      active,
      windowId: positiveInteger(args.window_id) || undefined
    })
    const pageReady = created.id ? await waitForTabReady(chromeApi, created.id, args) : null
    return withTopLevelTab({ navigated: true, url }, pageReady, created)
  }

  return withLoadWatcher(chromeApi, tab.id, args, async (watcher) => {
    const updated = await chromeApi.tabs.update(tab.id!, { url, active })
    if (active && updated) {
      await focusWindow(chromeApi, updated.windowId).catch(() => undefined)
      rememberActiveTab(updated)
    }
    const pageReady = await waitForTabReady(chromeApi, tab.id!, args, watcher)
    return withTopLevelTab({ navigated: true, url }, pageReady, updated)
  })
}

/** Reload plus the two history moves, which share the same wait-for-load shape. */
async function historyAction(
  chromeApi: ChromeApi,
  args: BrowserActionArgs,
  action: 'reload' | 'go_back' | 'go_forward'
): Promise<BrowserActionResult> {
  const tab = await tabForPageAction(chromeApi, args)
  const tabId = tab?.id
  if (!tabId) {
    throw new Error('no target tab')
  }

  return withLoadWatcher(chromeApi, tabId, args, async (watcher) => {
    let summary: Record<string, unknown>
    if (action === 'reload') {
      const bypassCache = args.bypass_cache ?? false
      await chromeApi.tabs.reload(tabId, { bypassCache })
      summary = { reloaded: true, bypass_cache: bypassCache }
    } else {
      // history.back()/forward() in the page is the fallback when the tabs API
      // move is unavailable or rejects (for example on a restored tab).
      const move = action === 'go_back' ? chromeApi.tabs.goBack : chromeApi.tabs.goForward
      if (move) {
        await move.call(chromeApi.tabs, tabId).catch(async (error: unknown) => {
          await runPageScript(chromeApi, tabId, args).catch(() => {
            throw error
          })
        })
      } else {
        await runPageScript(chromeApi, tabId, args)
      }
      summary = { [action === 'go_back' ? 'went_back' : 'went_forward']: true }
    }
    const pageReady = await waitForTabReady(chromeApi, tabId, args, watcher)
    return withTopLevelTab(summary, pageReady, tab)
  })
}

/**
 * Performs an action against the active tab's page, then reports the state the
 * page settled into. Debugger-backed paths are preferred where available: they
 * dispatch trusted input events and survive strict page CSP.
 */
async function executePageAction(
  chromeApi: ChromeApi,
  args: BrowserActionArgs
): Promise<BrowserActionResult> {
  const tab = await tabForPageAction(chromeApi, args)
  const tabId = tab?.id
  if (!tabId) {
    throw new Error('no target tab')
  }

  // Give a page that is still loading a brief moment before acting on it.
  await waitForTabReadyIfLoading(chromeApi, tabId, args, {
    bestEffort: true,
    timeoutMs: PRE_ACTION_LOADING_TIMEOUT_MS,
    noLoadTimeoutMs: ACTION_SETTLE_NO_LOAD_TIMEOUT_MS
  })

  // These answer from the page without changing it, so no settle is needed.
  if (args.action === 'screenshot') {
    return screenshot(chromeApi, tabId, args, tab)
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

  const watcher = shouldWaitAfterAction(args)
    ? createTabLoadWatcher(chromeApi, tabId, actionTimeoutMs(args), args)
    : null

  const settle = (result: BrowserActionResult) =>
    settleAfterAction(chromeApi, tabId, args, watcher, result)

  try {
    if (args.action === 'upload_file') {
      return settle(await uploadFile(chromeApi, tabId, args))
    }

    const debuggerAvailable = Boolean(chromeApi.debugger) && !args.frame_id
    if (debuggerAvailable && (args.action === 'click' || args.action === 'hover')) {
      return settle(await dispatchNativePointerAction(chromeApi, tabId, args))
    }
    if (debuggerAvailable && args.action === 'press_key') {
      return settle(await dispatchNativeKey(chromeApi, tabId, args))
    }
    if (debuggerAvailable && args.action === 'type_text') {
      // A null result means the debugger could not focus the field; fall
      // through to the scripting path rather than failing the action.
      const typed = await dispatchNativeTextInput(chromeApi, tabId, args)
      if (typed) {
        return settle(typed)
      }
    }
    if (args.action === 'execute_javascript' && scriptExecutionMode(args) === 'debugger') {
      return settle(await executeJavaScriptWithDebugger(chromeApi, tabId, args))
    }

    return settle(await runPageScript(chromeApi, tabId, args))
  } catch (error) {
    watcher?.cancel()
    throw error
  }
}

async function screenshot(
  chromeApi: ChromeApi,
  tabId: number,
  args: BrowserActionArgs,
  tab: ChromeTabInfo | null
): Promise<BrowserActionResult> {
  // Only the debugger can capture beyond the visible viewport.
  if (args.full_page || args.selector || hasViewportOverride(args)) {
    return captureScreenshotWithDebugger(chromeApi, tabId, args, tab)
  }
  const visibleTab = await activateTab(chromeApi, tabId).catch(() => tab)
  const dataUrl = await chromeApi.tabs.captureVisibleTab(visibleTab?.windowId || tab?.windowId, {
    format: 'png'
  })
  return {
    captured: true,
    tab: tabSummary(visibleTab || tab),
    mime_type: 'image/png',
    size: dataUrl.length,
    data_url: args.include_data_url ? dataUrl : undefined
  }
}

/** Runs {@link pageActionDispatcher} in the tab and returns its result. */
async function runPageScript(
  chromeApi: ChromeApi,
  tabId: number,
  args: BrowserActionArgs
): Promise<BrowserActionResult> {
  const [execution] = await chromeApi.scripting.executeScript<
    BrowserActionResult,
    BrowserActionArgs
  >({
    target: scriptTarget(tabId, args),
    world: args.action === 'execute_javascript' ? scriptExecutionWorld(args) : 'ISOLATED',
    func: pageActionDispatcher,
    args: [args]
  })
  if (execution?.result === undefined) {
    throw new Error(`${args.action || 'browser action'} did not return a script result`)
  }
  return execution.result
}

/** Attaches the page-ready block describing what the action settled the page to. */
async function settleAfterAction(
  chromeApi: ChromeApi,
  tabId: number,
  args: BrowserActionArgs,
  watcher: TabLoadWatcher | null,
  result: BrowserActionResult
): Promise<BrowserActionResult> {
  return withPageReady(result, await waitForPageSettleAfterAction(chromeApi, tabId, args, watcher))
}

/** Arms a load watcher for `work`, cancelling it if `work` throws. */
async function withLoadWatcher(
  chromeApi: ChromeApi,
  tabId: number,
  args: BrowserActionArgs,
  work: (watcher: TabLoadWatcher) => Promise<BrowserActionResult>
): Promise<BrowserActionResult> {
  const watcher = createTabLoadWatcher(chromeApi, tabId, actionTimeoutMs(args), args)
  try {
    return await work(watcher)
  } catch (error) {
    watcher.cancel()
    throw error
  }
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

export { activeTab, rememberActiveTab, tabSummary }
