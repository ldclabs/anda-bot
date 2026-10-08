import { delay } from '$lib/utils/async'
import { scriptWithImplicitReturn } from './browser-script'
import {
  actionTimeoutMs,
  activateTab,
  errorMessage,
  normalizeOptionalText,
  positiveInteger,
  tabSummary
} from './browser-tabs'
import { pageActionDispatcher } from './page-scripts'
import type { BrowserActionArgs, BrowserActionResult, ChromeApi, ChromeTabInfo } from './types'

/**
 * Everything Anda does through the Chrome DevTools Protocol.
 *
 * The debugger covers what `scripting.executeScript` cannot: evaluating in the
 * page's own world past a strict CSP, capturing beyond the viewport, printing to
 * PDF, reading the accessibility tree, waiting for network idle, and dispatching
 * input events that pages accept as real user gestures.
 *
 * CDP sessions are not reentrant, so debugger work serializes through a per-tab
 * lock. A session stays attached for a few idle seconds after its last action:
 * every attach shows the browser's debugging infobar, which resizes the
 * viewport, so a run of actions keeps one stable layout instead of flickering.
 */

const DEBUGGER_PROTOCOL_VERSION = '1.3'
const DEBUGGER_COMMAND_MAX_RETRIES = 2
const DEBUGGER_IDLE_DETACH_MS = 5_000
/** Lets the infobar of a fresh attach finish resizing the viewport. */
const DEBUGGER_INFOBAR_SETTLE_MS = 250
const NETWORK_IDLE_QUIET_MS = 500
/** Network idleness is a best-effort hint; busy pages never reach it. */
const NETWORK_IDLE_MAX_WAIT_MS = 3_000
/** Taller full-page captures exceed the GPU texture limit at 2x scale. */
const FULL_PAGE_MAX_HEIGHT = 8_000

const debuggerActionLocks = new Map<number, Promise<void>>()
const idleDetachTimers = new Map<number, ReturnType<typeof setTimeout>>()

type DebuggerTarget = { tabId: number }
type AttachedDebuggerTarget = DebuggerTarget & {
  /** True when this task attached the session instead of reusing one. */
  freshlyAttached: boolean
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
}
type RuntimeCompileScriptResult = { exceptionDetails?: RuntimeExceptionDetails }
type PageCaptureScreenshotResult = { data?: string }
type PagePrintToPdfResult = { data?: string }
type PageLayoutMetricsResult = {
  /** CSS pixels, which clips use. */
  cssContentSize?: { width?: number; height?: number }
  /** Device pixels; only for browsers without `cssContentSize`. */
  contentSize?: { width?: number; height?: number }
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
  const ensureAttached = () => attachDebuggerIfNeeded(chromeApi, target)

  cancelIdleDetach(tabId)
  try {
    const attachedTarget: AttachedDebuggerTarget = {
      ...target,
      freshlyAttached: await ensureAttached(),
      sendCommand: (method, commandParams) =>
        sendAttachedDebuggerCommand(chromeApi, target, method, commandParams, async () => {
          await ensureAttached()
        })
    }
    return await task(attachedTarget)
  } finally {
    scheduleIdleDetach(chromeApi, tabId)
  }
}

function cancelIdleDetach(tabId: number): void {
  const timer = idleDetachTimers.get(tabId)
  if (timer) {
    clearTimeout(timer)
    idleDetachTimers.delete(tabId)
  }
}

function scheduleIdleDetach(chromeApi: ChromeApi, tabId: number): void {
  cancelIdleDetach(tabId)
  idleDetachTimers.set(
    tabId,
    setTimeout(() => {
      idleDetachTimers.delete(tabId)
      void chromeApi.debugger?.detach({ tabId }).catch(() => undefined)
    }, DEBUGGER_IDLE_DETACH_MS)
  )
}

/** Attaches unless a session is already open; true when it attached now. */
async function attachDebuggerIfNeeded(
  chromeApi: ChromeApi,
  target: DebuggerTarget
): Promise<boolean> {
  try {
    await chromeApi.debugger!.attach(target, DEBUGGER_PROTOCOL_VERSION)
    return true
  } catch (error) {
    if (isDebuggerAlreadyAttachedError(error)) {
      return false
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

export async function executeJavaScriptWithDebugger(
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
    throw new Error('execute_javascript runs in the top frame and does not accept frame_id')
  }

  return withAttachedDebugger(chromeApi, tabId, async (target) => {
    await target.sendCommand('Runtime.enable').catch(() => undefined)
    const result = await evaluateDebuggerJavaScript(target, code)
    return { executed: true, world: 'debugger', result }
  })
}

/**
 * Runs `code` once, shaped like a console entry: a bare expression returns its
 * value, statements return their final expression, and anything else runs as
 * a function body. The shape is chosen by compiling, never by running, so a
 * script that throws at run time (a failed `JSON.parse`, an invalid selector)
 * is not run again in another shape. Async wrappers let any shape `await`.
 */
async function evaluateDebuggerJavaScript(
  target: AttachedDebuggerTarget,
  code: string
): Promise<unknown> {
  const expression = code.trim().replace(/;+$/, '')
  const implicitReturn = scriptWithImplicitReturn(code)
  const shapes = [
    `(async () => (\n${expression}\n))()`,
    ...(implicitReturn ? [`(async () => {\n${implicitReturn}\n})()`] : [])
  ]
  let source = `(async () => {\n${code}\n})()`
  for (const shape of shapes) {
    if (await debuggerScriptCompiles(target, shape)) {
      source = shape
      break
    }
  }
  return debuggerEvaluationValue(await sendDebuggerRuntimeEvaluate(target, source))
}

async function debuggerScriptCompiles(
  target: AttachedDebuggerTarget,
  expression: string
): Promise<boolean> {
  const compiled = await target.sendCommand<RuntimeCompileScriptResult>('Runtime.compileScript', {
    expression,
    sourceURL: '',
    persistScript: false
  })
  return !compiled?.exceptionDetails
}

function sendDebuggerRuntimeEvaluate(
  target: AttachedDebuggerTarget,
  expression: string
): Promise<RuntimeEvaluateResult> {
  const params: RuntimeEvaluateParams = {
    expression,
    awaitPromise: true,
    returnByValue: true,
    userGesture: true
  }
  return target.sendCommand<RuntimeEvaluateResult>('Runtime.evaluate', params)
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

export async function captureScreenshotWithDebugger(
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
        const page = args.full_page && !args.selector ? await fullPageScreenshotClip(target) : null
        const clip = args.selector ? await elementScreenshotClip(target, args.selector) : page?.clip
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
        return { data: result.data, clip, viewport, contentHeight: page?.contentHeight }
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
    viewport: capture.viewport || null,
    ...(capture.clip && capture.contentHeight && capture.contentHeight > capture.clip.height
      ? { truncated: true, content_height: capture.contentHeight }
      : {})
  }
}

export function hasViewportOverride(args: BrowserActionArgs): boolean {
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
    element.scrollIntoView({ block: 'center', inline: 'center', behavior: 'instant' });
    const rect = element.getBoundingClientRect();
    return {
      x: Math.max(0, rect.left + window.scrollX),
      y: Math.max(0, rect.top + window.scrollY),
      width: Math.max(1, rect.width),
      height: Math.max(1, rect.height),
      scale: 1
    };
  })()`
  const clip = debuggerEvaluationValue(await sendDebuggerRuntimeEvaluate(target, script))
  if (!isScreenshotClip(clip)) {
    throw new Error(`selector not found or has no visible bounds: ${selector}`)
  }
  return clip
}

/** The whole page up to {@link FULL_PAGE_MAX_HEIGHT}, in CSS pixels. */
async function fullPageScreenshotClip(
  target: AttachedDebuggerTarget
): Promise<{ clip: Record<string, number>; contentHeight: number }> {
  const metrics = await target.sendCommand<PageLayoutMetricsResult>('Page.getLayoutMetrics')
  const contentSize = metrics.cssContentSize || metrics.contentSize || {}
  const contentHeight = Math.max(1, Math.ceil(contentSize.height || 1))
  return {
    clip: {
      x: 0,
      y: 0,
      width: Math.max(1, Math.ceil(contentSize.width || 1)),
      height: Math.min(contentHeight, FULL_PAGE_MAX_HEIGHT),
      scale: 1
    },
    contentHeight
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

export async function getAccessibilityTree(
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

export async function printToPdf(
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

export async function waitForNetworkIdle(
  chromeApi: ChromeApi,
  tabId: number,
  args: BrowserActionArgs,
  timeoutMs = NETWORK_IDLE_MAX_WAIT_MS
): Promise<BrowserActionResult> {
  const timeout = Math.min(timeoutMs, actionTimeoutMs(args))
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

  const inFlight = new Set<string>()
  let quietTimer: ReturnType<typeof setTimeout> | null = null
  let timeoutTimer: ReturnType<typeof setTimeout> | null = null
  let cleanedUp = false

  let resolveWait: (value: BrowserActionResult) => void
  const wait = new Promise<BrowserActionResult>((resolve) => {
    resolveWait = resolve
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

  const scheduleQuietCheck = () => {
    if (quietTimer) {
      clearTimeout(quietTimer)
    }
    if (inFlight.size > 0) {
      return
    }
    quietTimer = setTimeout(() => {
      settle({ network_idle: true, quiet_ms: NETWORK_IDLE_QUIET_MS })
    }, NETWORK_IDLE_QUIET_MS)
  }

  const listener = (
    source: { tabId?: number },
    method: string,
    params?: Record<string, unknown>
  ) => {
    if (source.tabId !== target.tabId) {
      return
    }
    if (method === 'Network.requestWillBeSent') {
      if (typeof params?.requestId === 'string') inFlight.add(params.requestId)
      if (quietTimer) {
        clearTimeout(quietTimer)
        quietTimer = null
      }
      return
    }
    if (method === 'Network.loadingFinished' || method === 'Network.loadingFailed') {
      if (typeof params?.requestId === 'string') inFlight.delete(params.requestId)
      scheduleQuietCheck()
    }
  }

  event.addListener(listener)
  timeoutTimer = setTimeout(() => {
    settle({ network_idle: false, waited_ms: timeout, in_flight: inFlight.size })
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

export async function handleDialog(
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

export async function uploadFile(
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
      // Only the root node id is needed; DOM.querySelector resolves from it.
      const documentResult = await target.sendCommand<DomGetDocumentResult>('DOM.getDocument', {
        depth: 0
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

export async function dispatchNativePointerAction(
  chromeApi: ChromeApi,
  tabId: number,
  args: BrowserActionArgs
): Promise<BrowserActionResult> {
  return runExclusiveDebuggerAction(tabId, () =>
    withAttachedDebugger(chromeApi, tabId, async (debuggerTarget) => {
      const target = await resolveNativeInputTarget(chromeApi, debuggerTarget, args)
      const point = targetPoint(target)
      if (!point) {
        throw new Error('could not resolve a native input coordinate')
      }
      await dispatchNativeMouseMove(debuggerTarget, point.x, point.y)
      if (args.action === 'click') {
        await dispatchNativePrimaryClick(
          debuggerTarget,
          point.x,
          point.y,
          target.mobile_like === true
        )
      }
      return {
        [args.action === 'click' ? 'clicked' : 'hovered']: true,
        native: true,
        selector: args.selector || null,
        label: target.label || '',
        ...point,
        bounding_box: target.bounding_box || null
      }
    })
  )
}

/** Null when the target is not a native text input or did not take the text. */
export async function dispatchNativeTextInput(
  chromeApi: ChromeApi,
  tabId: number,
  args: BrowserActionArgs
): Promise<BrowserActionResult | null> {
  const text = String(args.text || '')
  return runExclusiveDebuggerAction(tabId, () =>
    withAttachedDebugger(chromeApi, tabId, async (target) => {
      const inputTarget = await resolveNativeInputTarget(chromeApi, target, args)
      if (inputTarget.native_text_input === false) {
        return null
      }
      const point = targetPoint(inputTarget)
      if (!point) {
        throw new Error('could not resolve a native text input coordinate')
      }
      await dispatchNativeMouseMove(target, point.x, point.y)
      await dispatchNativePrimaryClick(target, point.x, point.y, inputTarget.mobile_like === true)
      await delay(50)
      await dispatchSelectAll(target)
      await dispatchKeyDefinition(target, keyDefinition('Backspace'))
      if (text) {
        await target.sendCommand('Input.insertText', { text })
      }
      const verified = await verifyNativeTextInput(target, inputTarget.selector, text)
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
        ...point,
        bounding_box: inputTarget.bounding_box || null
      }
    })
  )
}

/**
 * Locates the element to act on, with the debugger already attached: its
 * infobar resizes the viewport, which would move a target found earlier.
 */
async function resolveNativeInputTarget(
  chromeApi: ChromeApi,
  target: AttachedDebuggerTarget,
  args: BrowserActionArgs
): Promise<Record<string, unknown>> {
  if (target.freshlyAttached) {
    await delay(DEBUGGER_INFOBAR_SETTLE_MS)
  }
  const [execution] = await chromeApi.scripting.executeScript<
    BrowserActionResult,
    BrowserActionArgs
  >({
    target: { tabId: target.tabId },
    world: 'ISOLATED',
    func: pageActionDispatcher,
    args: [{ ...args, resolve_input_target: true }]
  })
  const result = execution?.result
  return result && typeof result === 'object' ? (result as Record<string, unknown>) : {}
}

function targetPoint(target: Record<string, unknown>): { x: number; y: number } | null {
  return typeof target.x === 'number' && typeof target.y === 'number'
    ? { x: target.x, y: target.y }
    : null
}

/**
 * Wheels at the viewport center, which scrolls whatever is under it: the
 * inner containers many single-page apps scroll instead of the window too.
 */
export async function dispatchNativeScroll(
  chromeApi: ChromeApi,
  tabId: number,
  args: BrowserActionArgs
): Promise<BrowserActionResult> {
  const amount = typeof args.amount === 'number' && Number.isFinite(args.amount) ? args.amount : 700
  return runExclusiveDebuggerAction(tabId, () =>
    withAttachedDebugger(chromeApi, tabId, async (target) => {
      const viewport = debuggerEvaluationValue(
        await sendDebuggerRuntimeEvaluate(target, '({ width: innerWidth, height: innerHeight })')
      ) as { width?: number; height?: number } | null
      const x = Math.round((viewport?.width || 0) / 2)
      const y = Math.round((viewport?.height || 0) / 2)
      await target.sendCommand('Input.dispatchMouseEvent', {
        type: 'mouseWheel',
        x,
        y,
        deltaX: 0,
        deltaY: amount
      })
      return { scrolled: true, native: true, amount, x, y }
    })
  )
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

export async function dispatchNativeKey(
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
  // Enter and Space carry text: the character event they produce is what
  // submits a form or activates a focused button.
  const special: Record<string, { code: string; windowsVirtualKeyCode: number; text?: string }> = {
    Enter: { code: 'Enter', windowsVirtualKeyCode: 13, text: '\r' },
    Escape: { code: 'Escape', windowsVirtualKeyCode: 27 },
    Tab: { code: 'Tab', windowsVirtualKeyCode: 9 },
    Space: { code: 'Space', windowsVirtualKeyCode: 32, text: ' ' },
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
      ...(mapped.text ? { text: mapped.text, unmodifiedText: mapped.text } : {}),
      windowsVirtualKeyCode: mapped.windowsVirtualKeyCode,
      nativeVirtualKeyCode: mapped.windowsVirtualKeyCode
    }
  }
  const text = normalizedKey.length === 1 ? normalizedKey : ''
  const upper = text.toUpperCase()
  const windowsVirtualKeyCode = upper ? upper.charCodeAt(0) : 0
  const code = /^[A-Z]$/.test(upper)
    ? `Key${upper}`
    : /^[0-9]$/.test(upper)
      ? `Digit${upper}`
      : normalizedKey
  return {
    key: normalizedKey,
    code,
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
