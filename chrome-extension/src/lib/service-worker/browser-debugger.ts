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
import { resolveInputTarget } from './page-scripts'
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
 * lock and the session detaches once the last action on that tab finishes.
 */

const DEBUGGER_PROTOCOL_VERSION = '1.3'
const DEBUGGER_COMMAND_MAX_RETRIES = 2
const NETWORK_IDLE_QUIET_MS = 500

const debuggerActionLocks = new Map<number, Promise<void>>()

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
  timeoutMs?: number
): Promise<BrowserActionResult> {
  const timeout = timeoutMs ?? actionTimeoutMs(args, 30000)
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

export async function dispatchNativePointerAction(
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

export async function dispatchNativeTextInput(
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
