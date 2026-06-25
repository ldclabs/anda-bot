import {
  activeTab,
  executeBrowserAction,
  rememberActiveTab
} from '$lib/service-worker/browser-actions'
import { getChromeApi, isDevelopmentMode } from '$lib/service-worker/chrome'
import { applyUiLanguage, initI18n, uiLanguageStorageKey } from '$lib/i18n'
import { handlePageAudioCapture, handlePageSpeechRecognition } from '$lib/service-worker/page-voice'
import {
  browserSession,
  connectionKey,
  defaultSettings,
  errorToCode,
  errorToMessage,
  loadSettings,
  normalizeSettings,
  websocketUrl
} from '$lib/service-worker/settings'
import { chromeTtsAvailable, speakWithChromeTts } from '$lib/service-worker/tts'
import {
  isPageElementInfo,
  pageElementAttachmentMessageType,
  pageElementAttachmentRequestStorageKey,
  pageElementCaptureMessageType,
  pageElementContextMenuId,
  pageElementDomMemoryKey,
  pageElementMemoryKey,
  pageElementStorageKey,
  type PageElementAttachmentRequest,
  type PageElementInfo
} from '$lib/anda/page-element'
import type {
  BrowserCommand,
  ChromeContextMenuClickInfo,
  ChromeTabInfo,
  ChromeWebNavigationDetails,
  ChromeWebNavigationTabReplacedDetails,
  ChromeWebNavigationTargetDetails,
  ExtensionMessage,
  ExtensionResponse,
  PendingRpc,
  RpcResponseMessage,
  SettingsState
} from '$lib/service-worker/types'

const keepAliveIntervalMs = 20_000
const reconnectDelayMs = 3_000
const rpcTimeoutMs = 30 * 60 * 1000
const pageElementCaptureMaxAgeMs = 5 * 60 * 1000

const chromeApi = getChromeApi()
const isDevelopmentModePromise = isDevelopmentMode()
let currentSettings: SettingsState = { ...defaultSettings }
let socket: WebSocket | null = null
let socketKey = ''
let opening: Promise<void> | null = null
let openingReject: ((error: Error) => void) | null = null
let reconnectTimer: ReturnType<typeof setTimeout> | null = null
let keepAliveTimer: ReturnType<typeof setInterval> | null = null
let nextMessageId = 1
let status = 'starting'
const pending = new Map<number, PendingRpc>()
let sessionRefreshTimer: ReturnType<typeof setTimeout> | null = null
let browserActionQueue: Promise<void> = Promise.resolve()

void initI18n()
chromeApi.storage?.onChanged?.addListener?.((changes, areaName) => {
  if (areaName === 'local' && changes[uiLanguageStorageKey]) {
    void applyUiLanguage(changes[uiLanguageStorageKey].newValue)
  }
})

chromeApi.runtime.onInstalled.addListener((details) => {
  if (chromeApi.sidePanel?.setPanelBehavior) {
    chromeApi.sidePanel.setPanelBehavior({ openPanelOnActionClick: true }).catch(() => {})
  }
  createPageElementContextMenu().catch(() => undefined)
  injectPageElementContentScriptIntoOpenTabs().catch(() => undefined)
  loadSettingsAndConnect()
  if (details.reason === 'install') {
    chromeApi.tabs.create({
      url: 'index.html'
    })
  }
})

chromeApi.runtime.onStartup.addListener(() => {
  injectPageElementContentScriptIntoOpenTabs().catch(() => undefined)
  loadSettingsAndConnect()
})

chromeApi.action.onClicked.addListener((tab) => {
  rememberActiveTab(tab)
  openSidePanel(tab)
})

chromeApi.tabs.onActivated.addListener((activeInfo) => {
  rememberActiveTab(activeInfo.tabId)
  scheduleBrowserSessionRefresh()
})

chromeApi.tabs.onUpdated.addListener((_tabId, changeInfo, tab) => {
  if (tab.active || changeInfo.title || changeInfo.url) {
    if (tab.active) {
      rememberActiveTab(tab)
    }
    scheduleBrowserSessionRefresh()
  }
})

chromeApi.windows?.onFocusChanged?.addListener((windowId) => {
  if (windowId < 0) {
    return
  }
  chromeApi.tabs
    .query({ active: true, windowId })
    .then(([tab]) => {
      rememberActiveTab(tab)
      scheduleBrowserSessionRefresh()
    })
    .catch(() => undefined)
})

registerWebNavigationSessionRefreshListeners()
registerPageElementContextMenuListener()

chromeApi.runtime.onMessage.addListener((message, _sender, sendResponse) => {
  handleExtensionMessage(message)
    .then((res) => {
      sendResponse(res)
      isDevelopmentModePromise.then((dev) => {
        if (dev) {
          console.log(
            `onMessage: ${message.type || 'unknown'}`,
            extensionMessageLogSummary(message),
            extensionResponseLogSummary(res)
          )
        }
      })
    })
    .catch((error) => {
      sendResponse({ ok: false, error: errorToMessage(error), status })
    })
  return true
})

loadSettingsAndConnect()

async function handleExtensionMessage(message: ExtensionMessage): Promise<ExtensionResponse> {
  if (message.settings) {
    currentSettings = normalizeSettings(message.settings)
  }

  switch (message.type) {
    case 'anda_rpc': {
      if (!message.method) {
        throw new Error('missing RPC method')
      }
      const result = await sendRpc(message.method, message.params || [], currentSettings)
      return { ok: true, result, status }
    }
    case 'anda_settings_changed': {
      await chromeApi.storage.local.set(currentSettings)
      if (currentSettings.token) {
        await ensureSocket(currentSettings)
        await registerBrowserSession(currentSettings)
      } else {
        closeSocket('missing bearer token')
        status = 'ready'
      }
      return { ok: true, status }
    }
    case 'anda_register': {
      const session = await registerBrowserSession(currentSettings)
      return { ok: true, result: { session }, status }
    }
    case 'anda_status': {
      return { ok: true, result: { status }, status }
    }
    case 'anda_chrome_tts_available': {
      return { ok: true, result: { available: chromeTtsAvailable(chromeApi) }, status }
    }
    case 'anda_chrome_tts_speak': {
      await speakWithChromeTts(chromeApi, message.text || '')
      return { ok: true, result: { spoken: true }, status }
    }
    case 'anda_chrome_tts_stop': {
      chromeApi.tts?.stop?.()
      return { ok: true, result: { stopped: true }, status }
    }
    case 'anda_page_speech_available': {
      const result = await handlePageSpeechRecognition(chromeApi, { action: 'available' })
      return { ok: true, result, status }
    }
    case 'anda_page_speech_start': {
      const result = await handlePageSpeechRecognition(chromeApi, {
        action: 'start',
        language: message.language
      })
      return { ok: true, result, status }
    }
    case 'anda_page_speech_stop': {
      const result = await handlePageSpeechRecognition(chromeApi, { action: 'stop' })
      return { ok: true, result, status }
    }
    case 'anda_page_speech_cancel': {
      const result = await handlePageSpeechRecognition(chromeApi, { action: 'cancel' })
      return { ok: true, result, status }
    }
    case 'anda_page_audio_available': {
      const result = await handlePageAudioCapture(chromeApi, { action: 'available' })
      return { ok: true, result, status }
    }
    case 'anda_page_audio_start': {
      const result = await handlePageAudioCapture(chromeApi, {
        action: 'start',
        mimeType: message.mimeType
      })
      return { ok: true, result, status }
    }
    case 'anda_page_audio_stop': {
      const result = await handlePageAudioCapture(chromeApi, { action: 'stop' })
      return { ok: true, result, status }
    }
    case 'anda_page_audio_cancel': {
      const result = await handlePageAudioCapture(chromeApi, { action: 'cancel' })
      return { ok: true, result, status }
    }
    case pageElementCaptureMessageType: {
      await storeCapturedPageElement(message.pageElementInfo)
      return { ok: true, status }
    }
    default:
      throw new Error(`unsupported extension message: ${message.type || 'unknown'}`)
  }
}

async function loadSettingsAndConnect(): Promise<void> {
  currentSettings = await loadSettings(chromeApi)
  if (!currentSettings.token) {
    status = 'ready'
    return
  }

  try {
    await ensureSocket(currentSettings)
    await registerBrowserSession(currentSettings)
  } catch (_error) {
    scheduleReconnect(currentSettings)
  }
}

async function openSidePanel(tab: ChromeTabInfo): Promise<void> {
  if (!chromeApi.sidePanel?.open) {
    return
  }

  try {
    if (typeof tab.id === 'number') {
      await chromeApi.sidePanel.open({ tabId: tab.id })
    } else if (typeof tab.windowId === 'number') {
      await chromeApi.sidePanel.open({ windowId: tab.windowId })
    }
  } catch (_error) {}
}

function extensionMessageLogSummary(message: ExtensionMessage): Record<string, unknown> {
  return {
    type: message.type || 'unknown',
    method: message.method,
    settings: message.settings
      ? {
          ...message.settings,
          token: message.settings.token ? '<redacted>' : ''
        }
      : undefined,
    params_count: Array.isArray(message.params) ? message.params.length : undefined,
    has_text: typeof message.text === 'string' ? message.text.length > 0 : undefined,
    language: message.language,
    mime_type: message.mimeType,
    has_page_element_request: Boolean(message.pageElementRequest),
    has_page_element_info: Boolean(message.pageElementInfo)
  }
}

function registerPageElementContextMenuListener(): void {
  const contextMenus = chromeApi.contextMenus
  if (!contextMenus) {
    return
  }
  contextMenus.onClicked.addListener((info, tab) => {
    if (info.menuItemId !== pageElementContextMenuId) {
      return
    }
    if (tab) {
      void openSidePanel(tab)
    }
    handlePageElementContextMenuClick(info, tab).catch((error) => {
      console.warn('Failed to send page element to side panel', error)
    })
  })
}

async function injectPageElementContentScriptIntoOpenTabs(): Promise<void> {
  const tabs = await chromeApi.tabs.query({})
  await Promise.all(tabs.map((tab) => injectPageElementContentScript(tab)))
}

async function injectPageElementContentScript(tab: ChromeTabInfo): Promise<void> {
  if (typeof tab.id !== 'number' || !isPageElementInjectableUrl(tab.url)) {
    return
  }
  await Promise.resolve(
    chromeApi.scripting.executeScript({
      target: { tabId: tab.id, allFrames: true },
      files: ['assets/page_element_content.js']
    })
  ).catch(() => undefined)
}

function isPageElementInjectableUrl(url: string | undefined): boolean {
  return /^(https?|file):\/\//i.test(url || '')
}

async function createPageElementContextMenu(): Promise<void> {
  const contextMenus = chromeApi.contextMenus
  if (!contextMenus) {
    return
  }

  await Promise.resolve(contextMenus.remove?.(pageElementContextMenuId)).catch(() => undefined)
  await Promise.resolve(
    contextMenus.create({
      id: pageElementContextMenuId,
      title: chromeApi.i18n.getMessage('sendPageElementToChat') || 'Send this content to Anda',
      contexts: ['all']
    })
  )
}

async function handlePageElementContextMenuClick(
  info: ChromeContextMenuClickInfo,
  tab?: ChromeTabInfo
): Promise<void> {
  const element = await loadLastRightClickedElement(info, tab)
  if (!element) {
    console.warn('No recent page element was captured for the context menu click.')
    return
  }

  const request: PageElementAttachmentRequest = {
    id: createRequestId(),
    createdAt: Date.now(),
    element
  }
  await flashCapturedPageElement(info, tab, element)
  await chromeApi.storage.session?.set({
    [pageElementAttachmentRequestStorageKey]: request
  })

  await chromeApi.runtime
    .sendMessage({
      type: pageElementAttachmentMessageType,
      pageElementRequest: request
    })
    .catch(() => ({ ok: false, error: 'side panel unavailable' }))
}

async function loadLastRightClickedElement(
  clickInfo: ChromeContextMenuClickInfo,
  tab?: ChromeTabInfo
): Promise<PageElementInfo | null> {
  const injectedElement = await readLastRightClickedElementFromTab(clickInfo, tab)
  if (injectedElement) {
    return injectedElement
  }

  const storage = chromeApi.storage.session
  if (!storage) {
    return null
  }

  const saved = await storage.get(pageElementStorageKey)
  const element = saved[pageElementStorageKey]
  if (!isPageElementInfo(element) || !isFreshPageElementForClick(element, clickInfo)) {
    return null
  }
  return element
}

async function readLastRightClickedElementFromTab(
  clickInfo: ChromeContextMenuClickInfo,
  tab?: ChromeTabInfo
): Promise<PageElementInfo | null> {
  if (typeof tab?.id !== 'number') {
    return null
  }

  let results: Array<{ result: unknown }> = []
  try {
    results =
      (await Promise.resolve(
        chromeApi.scripting.executeScript<unknown, { key: string }>({
          target: pageElementScriptTarget(tab.id, clickInfo),
          func: ({ key }) => (globalThis as Record<string, unknown>)[key] || null,
          args: [{ key: pageElementMemoryKey }]
        })
      ).catch(() => [])) || []
  } catch (_error) {
    results = []
  }

  for (const result of results) {
    const element = result.result
    if (isPageElementInfo(element) && isFreshPageElementForClick(element, clickInfo)) {
      return element
    }
  }
  return null
}

async function flashCapturedPageElement(
  clickInfo: ChromeContextMenuClickInfo,
  tab: ChromeTabInfo | undefined,
  element: PageElementInfo
): Promise<void> {
  if (typeof tab?.id !== 'number') {
    return
  }

  await Promise.resolve(
    chromeApi.scripting.executeScript<boolean, PageElementHighlightArgs>({
      target: pageElementScriptTarget(tab.id, clickInfo),
      func: flashPageElementInTab,
      args: [
        {
          domElementMemoryKey: pageElementDomMemoryKey,
          cssPath: element.cssPath,
          xpath: element.xpath
        }
      ]
    })
  ).catch(() => undefined)
}

type PageElementHighlightArgs = {
  domElementMemoryKey: string
  cssPath: string
  xpath: string
}

function flashPageElementInTab(args: PageElementHighlightArgs): boolean {
  const registry = globalThis as Record<string, unknown>
  const remembered = registry[args.domElementMemoryKey]
  let target = remembered instanceof Element && remembered.isConnected ? remembered : null

  if (!target && args.cssPath) {
    try {
      target = document.querySelector(args.cssPath)
    } catch (_error) {}
  }

  if (!target && args.xpath) {
    try {
      const node = document.evaluate(
        args.xpath,
        document,
        null,
        XPathResult.FIRST_ORDERED_NODE_TYPE,
        null
      ).singleNodeValue
      target = node instanceof Element ? node : null
    } catch (_error) {}
  }

  if (!target) {
    return false
  }

  const rawRect = target.getBoundingClientRect()
  const viewportWidth = document.documentElement.clientWidth || window.innerWidth
  const viewportHeight = document.documentElement.clientHeight || window.innerHeight
  const left = Math.max(0, Math.min(viewportWidth, rawRect.left))
  const top = Math.max(0, Math.min(viewportHeight, rawRect.top))
  const right = Math.max(0, Math.min(viewportWidth, rawRect.right))
  const bottom = Math.max(0, Math.min(viewportHeight, rawRect.bottom))
  const width = right - left
  const height = bottom - top
  if (width <= 0 || height <= 0) {
    return false
  }

  document
    .querySelectorAll('[data-anda-page-element-highlight="true"]')
    .forEach((node) => node.remove())

  const margin = 4
  const overlay = document.createElement('div')
  overlay.setAttribute('data-anda-page-element-highlight', 'true')
  Object.assign(overlay.style, {
    position: 'fixed',
    left: `${Math.max(0, left - margin)}px`,
    top: `${Math.max(0, top - margin)}px`,
    width: `${width + margin * 2}px`,
    height: `${height + margin * 2}px`,
    border: '2px solid #10b981',
    borderRadius: `${Math.max(4, Math.min(12, Math.min(width, height) / 6))}px`,
    boxShadow: '0 0 0 4px rgba(16, 185, 129, 0.24), 0 0 28px rgba(16, 185, 129, 0.55)',
    boxSizing: 'border-box',
    background: 'rgba(16, 185, 129, 0.12)',
    opacity: '0',
    pointerEvents: 'none',
    transformOrigin: 'center',
    zIndex: '2147483647'
  })
  document.documentElement.append(overlay)

  const animation = overlay.animate(
    [
      { opacity: 0, transform: 'scale(0.985)' },
      { opacity: 1, transform: 'scale(1)', offset: 0.16 },
      { opacity: 0.78, transform: 'scale(1.01)', offset: 0.72 },
      { opacity: 0, transform: 'scale(1.015)' }
    ],
    {
      duration: 3200,
      easing: 'cubic-bezier(0.16, 1, 0.3, 1)',
      iterations: 1
    }
  )
  animation.onfinish = () => overlay.remove()
  setTimeout(() => overlay.remove(), 3800)
  return true
}

function pageElementScriptTarget(
  tabId: number,
  clickInfo: ChromeContextMenuClickInfo
): { tabId: number; frameIds?: number[]; allFrames?: boolean } {
  return typeof clickInfo.frameId === 'number' && clickInfo.frameId >= 0
    ? { tabId, frameIds: [clickInfo.frameId] }
    : { tabId, allFrames: true }
}

async function storeCapturedPageElement(value: unknown): Promise<void> {
  if (!isPageElementInfo(value)) {
    throw new Error('invalid page element capture')
  }
  await chromeApi.storage.session?.set({
    [pageElementStorageKey]: value
  })
}

function isFreshPageElementForClick(
  element: PageElementInfo,
  clickInfo: ChromeContextMenuClickInfo
): boolean {
  const ageMs = Date.now() - element.capturedAt
  if (!Number.isFinite(ageMs) || ageMs < 0 || ageMs > pageElementCaptureMaxAgeMs) {
    return false
  }

  const clickUrl = clickInfo.frameUrl || clickInfo.pageUrl || ''
  if (!clickUrl) {
    return true
  }
  return sameDocumentUrl(element.frameUrl, clickUrl) || sameDocumentUrl(element.pageUrl, clickUrl)
}

function sameDocumentUrl(left: string, right: string): boolean {
  return stripHash(left) === stripHash(right)
}

function stripHash(url: string): string {
  const index = url.indexOf('#')
  return index >= 0 ? url.slice(0, index) : url
}

function createRequestId(): string {
  const randomUUID = globalThis.crypto?.randomUUID
  if (randomUUID) {
    return randomUUID.call(globalThis.crypto)
  }
  return `${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 10)}`
}

function extensionResponseLogSummary(response: ExtensionResponse): Record<string, unknown> {
  return {
    ok: response.ok,
    status: response.status,
    has_result: response.ok ? response.result !== undefined : undefined,
    error: response.ok ? undefined : response.error
  }
}

async function sendRpc(
  method: string,
  params: unknown[],
  settings: SettingsState
): Promise<unknown> {
  await ensureSocket(settings)
  const activeSocket = socket
  if (!activeSocket || activeSocket.readyState !== WebSocket.OPEN) {
    throw new Error('WebSocket is not connected')
  }

  const id = nextMessageId++
  const payload = JSON.stringify({ id, method, params })

  return new Promise((resolve, reject) => {
    const timeout = setTimeout(() => {
      pending.delete(id)
      reject(new Error(`RPC ${method} timed out`))
    }, rpcTimeoutMs)

    pending.set(id, { resolve, reject, timeout })
    try {
      activeSocket.send(payload)
    } catch (error) {
      pending.delete(id)
      clearTimeout(timeout)
      reject(error instanceof Error ? error : new Error(String(error)))
    }
  })
}

async function ensureSocket(settings: SettingsState): Promise<void> {
  const normalized = normalizeSettings(settings)
  if (!normalized.token) {
    throw new Error('missing bearer token')
  }

  const key = connectionKey(normalized)
  if (socket && socket.readyState === WebSocket.OPEN && socketKey === key) {
    status = 'connected'
    return
  }
  if (opening && socketKey === key) {
    return opening
  }

  closeSocket('reconnecting')
  socketKey = key
  currentSettings = normalized
  status = 'connecting'

  opening = new Promise((resolve, reject) => {
    let settled = false
    const ws = new WebSocket(websocketUrl(normalized))
    socket = ws

    const fail = (error: Error) => {
      openingReject = null
      if (!settled) {
        settled = true
        reject(error)
      }
    }
    openingReject = fail

    const openTimeout = setTimeout(() => {
      fail(new Error('WebSocket connection timed out'))
      ws.close()
    }, 15_000)

    ws.onopen = () => {
      clearTimeout(openTimeout)
      settled = true
      openingReject = null
      opening = null
      status = 'connected'
      startKeepAlive()
      resolve()
      console.info('WebSocket connected')
    }

    ws.onmessage = (event) => {
      handleSocketMessage(event.data).catch((error) => {
        console.warn('Anda WebSocket message failed', error)
      })
    }

    ws.onerror = () => {
      status = 'connection failed'
    }

    ws.onclose = () => {
      clearTimeout(openTimeout)
      if (socket === ws) {
        socket = null
        opening = null
        stopKeepAlive()
        rejectPending('WebSocket connection closed')
        status = 'disconnected'
        scheduleReconnect(normalized)
      }
      fail(new Error('WebSocket connection closed'))
    }
  })

  return opening
}

function closeSocket(reason: string): void {
  if (reconnectTimer) {
    clearTimeout(reconnectTimer)
    reconnectTimer = null
  }
  stopKeepAlive()
  if (openingReject) {
    openingReject(new Error(reason))
    openingReject = null
  }
  if (socket) {
    const oldSocket = socket
    socket = null
    oldSocket.onopen = null
    oldSocket.onmessage = null
    oldSocket.onerror = null
    oldSocket.onclose = null
    oldSocket.close()
  }
  opening = null
  rejectPending(reason)
}

function scheduleReconnect(settings: SettingsState): void {
  if (!settings.token || reconnectTimer) {
    return
  }
  reconnectTimer = setTimeout(() => {
    reconnectTimer = null
    ensureSocket(settings)
      .then(() => registerBrowserSession(settings))
      .catch(() => scheduleReconnect(settings))
  }, reconnectDelayMs)
}

function startKeepAlive(): void {
  stopKeepAlive()
  keepAliveTimer = setInterval(() => {
    if (socket?.readyState === WebSocket.OPEN) {
      socket.send(JSON.stringify({ method: 'ping' }))
    }
  }, keepAliveIntervalMs)
}

function stopKeepAlive(): void {
  if (keepAliveTimer) {
    clearInterval(keepAliveTimer)
    keepAliveTimer = null
  }
}

function registerWebNavigationSessionRefreshListeners(): void {
  const webNavigation = chromeApi.webNavigation
  if (!webNavigation) {
    return
  }

  const refreshForMainFrame = (details: ChromeWebNavigationDetails) => {
    if (details.frameId === 0) {
      scheduleBrowserSessionRefresh()
    }
  }
  const refreshForTarget = (_details: ChromeWebNavigationTargetDetails) => {
    scheduleBrowserSessionRefresh()
  }
  const refreshForReplacement = (_details: ChromeWebNavigationTabReplacedDetails) => {
    scheduleBrowserSessionRefresh()
  }

  webNavigation.onBeforeNavigate?.addListener(refreshForMainFrame)
  webNavigation.onCommitted?.addListener(refreshForMainFrame)
  webNavigation.onDOMContentLoaded?.addListener(refreshForMainFrame)
  webNavigation.onCompleted?.addListener(refreshForMainFrame)
  webNavigation.onErrorOccurred?.addListener(refreshForMainFrame)
  webNavigation.onReferenceFragmentUpdated?.addListener(refreshForMainFrame)
  webNavigation.onHistoryStateUpdated?.addListener(refreshForMainFrame)
  webNavigation.onCreatedNavigationTarget?.addListener(refreshForTarget)
  webNavigation.onTabReplaced?.addListener(refreshForReplacement)
}

function rejectPending(reason: string): void {
  for (const [id, entry] of pending) {
    clearTimeout(entry.timeout)
    entry.reject(new Error(reason))
    pending.delete(id)
  }
}

async function handleSocketMessage(data: unknown): Promise<void> {
  if (typeof data !== 'string') {
    return
  }

  const message = JSON.parse(data) as RpcResponseMessage
  if (message.method === 'browser_action') {
    await queueBrowserActionRequest(message)
    return
  }

  if (typeof message.id !== 'number') {
    return
  }

  const entry = pending.get(message.id)
  if (!entry) {
    return
  }
  pending.delete(message.id)
  clearTimeout(entry.timeout)

  if (message.error) {
    entry.reject(new Error(message.error))
  } else {
    entry.resolve(message.result)
  }
}

function queueBrowserActionRequest(message: RpcResponseMessage): Promise<void> {
  const run = browserActionQueue
    .catch(() => undefined)
    .then(() => handleBrowserActionRequest(message))
  browserActionQueue = run.then(
    () => undefined,
    () => undefined
  )
  return run
}

async function handleBrowserActionRequest(message: RpcResponseMessage): Promise<void> {
  const command = message.params as BrowserCommand
  const id = typeof message.id === 'number' ? message.id : command.request_id
  let result: Record<string, unknown>

  try {
    const value = await executeBrowserAction(command, { chromeApi })
    result = { ok: true, value }
  } catch (error) {
    const errorCode = errorToCode(error)
    result = {
      ok: false,
      value: null,
      error: errorToMessage(error),
      ...(errorCode ? { error_code: errorCode } : {})
    }
  }

  if (socket?.readyState === WebSocket.OPEN) {
    socket.send(
      JSON.stringify({
        id,
        session: command.session,
        result
      })
    )
  }
}

async function registerBrowserSession(settings: SettingsState = currentSettings): Promise<string> {
  const session = await browserSession(chromeApi)
  if (!settings.token) {
    return session
  }
  const tab = await activeTab(chromeApi)

  await sendRpc(
    'browser_register',
    [
      {
        session,
        tab_id: tab?.id,
        url: tab?.url || '',
        title: tab?.title || ''
      }
    ],
    settings
  )

  return session
}

function scheduleBrowserSessionRefresh(): void {
  if (!currentSettings.token) {
    return
  }
  if (sessionRefreshTimer) {
    clearTimeout(sessionRefreshTimer)
  }
  sessionRefreshTimer = setTimeout(() => {
    sessionRefreshTimer = null
    registerBrowserSession(currentSettings).catch(() => undefined)
  }, 200)
}

export {}
