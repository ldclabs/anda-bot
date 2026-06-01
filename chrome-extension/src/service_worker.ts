import {
  activeTab,
  executeBrowserAction,
  rememberActiveTab
} from '$lib/service-worker/browser-actions'
import { getChromeApi, isDevelopmentMode } from '$lib/service-worker/chrome'
import { handlePageAudioCapture, handlePageSpeechRecognition } from '$lib/service-worker/page-voice'
import {
  browserSession,
  connectionKey,
  defaultSettings,
  errorToMessage,
  loadSettings,
  normalizeSettings,
  websocketUrl
} from '$lib/service-worker/settings'
import { chromeTtsAvailable, speakWithChromeTts } from '$lib/service-worker/tts'
import type {
  BrowserCommand,
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

chromeApi.runtime.onInstalled.addListener((reason: string) => {
  if (chromeApi.sidePanel?.setPanelBehavior) {
    chromeApi.sidePanel.setPanelBehavior({ openPanelOnActionClick: true }).catch(() => {})
  }
  loadSettingsAndConnect()
  if (reason === 'install') {
    chromeApi.tabs.create({
      url: 'index.html'
    })
  }
})

chromeApi.runtime.onStartup.addListener(() => {
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

chromeApi.runtime.onMessage.addListener((message, _sender, sendResponse) => {
  handleExtensionMessage(message)
    .then((res) => {
      sendResponse(res)
      isDevelopmentModePromise.then((dev) => {
        if (dev) {
          console.log(`onMessage: ${message.type}`, message, res)
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
    if (tab.id) {
      await chromeApi.sidePanel.open({ tabId: tab.id })
    } else if (tab.windowId) {
      await chromeApi.sidePanel.open({ windowId: tab.windowId })
    }
  } catch (_error) {}
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
    result = { ok: false, value: null, error: errorToMessage(error) }
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
