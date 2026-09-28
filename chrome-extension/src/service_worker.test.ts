import { afterEach, describe, expect, it, vi } from 'vitest'
import {
  pageElementAttachmentMessageType,
  pageElementAttachmentRequestStorageKey,
  pageElementContextMenuId,
  pageElementDomMemoryKey,
  pageElementSerializerKey,
  type PageElementInfo
} from '$lib/anda/page-element'
import type {
  ChromeApi,
  ChromeContextMenuClickInfo,
  ChromeRuntimeOnInstalledDetails,
  ChromeTabInfo,
  ExtensionMessage,
  ExtensionResponse
} from '$lib/service-worker/types'

type InstalledListener = (details: ChromeRuntimeOnInstalledDetails) => void
type ContextMenuClickListener = (
  info: ChromeContextMenuClickInfo,
  tab?: ChromeTabInfo | undefined
) => void
type MessageListener = (
  message: ExtensionMessage,
  sender: { url?: string },
  sendResponse: (response: ExtensionResponse) => void
) => boolean | void
type TabActivatedListener = (activeInfo: { tabId: number; windowId: number }) => void
type TabUpdatedListener = (
  tabId: number,
  changeInfo: { title?: string; url?: string },
  tab: ChromeTabInfo
) => void

const extensionOrigin = 'chrome-extension://anda/'
const extensionPage = { url: `${extensionOrigin}index.html` }

function createChromeEvent<Listener extends (...args: any[]) => void>() {
  const listeners: Listener[] = []
  return {
    addListener: vi.fn((listener: Listener) => {
      listeners.push(listener)
    }),
    removeListener: vi.fn((listener: Listener) => {
      const index = listeners.indexOf(listener)
      if (index >= 0) {
        listeners.splice(index, 1)
      }
    }),
    listeners
  }
}

function createChromeApi(options: { development?: boolean; token?: string } = {}) {
  const onInstalled = createChromeEvent<InstalledListener>()
  const onStartup = createChromeEvent<() => void>()
  const onActionClicked = createChromeEvent<(tab: { id?: number; windowId?: number }) => void>()
  const onContextMenuClicked = createChromeEvent<ContextMenuClickListener>()
  const onTabActivated = createChromeEvent<TabActivatedListener>()
  const onTabUpdated = createChromeEvent<TabUpdatedListener>()
  const onWindowFocusChanged = createChromeEvent<(windowId: number) => void>()
  const onMessageListeners: MessageListener[] = []
  const sessionState: Record<string, unknown> = {}

  const storageGet = async (keys: string[] | string) => {
    const list = Array.isArray(keys) ? keys : [keys]
    const result: Record<string, unknown> = {}
    for (const key of list) {
      if (key in sessionState) {
        result[key] = sessionState[key]
      }
    }
    return result
  }

  const chromeApi = {
    runtime: {
      onInstalled,
      onStartup,
      getURL: vi.fn((path: string) => `${extensionOrigin}${path}`),
      sendMessage: vi.fn(async () => ({ ok: true })),
      onMessage: {
        addListener: vi.fn((listener: MessageListener) => {
          onMessageListeners.push(listener)
        })
      }
    },
    management: options.development
      ? {
          getSelf: vi.fn(async () => ({ installType: 'development' }))
        }
      : undefined,
    action: {
      onClicked: onActionClicked
    },
    contextMenus: {
      create: vi.fn(),
      remove: vi.fn(),
      onClicked: onContextMenuClicked
    },
    sidePanel: {
      setPanelBehavior: vi.fn(async () => undefined),
      open: vi.fn(async () => undefined)
    },
    i18n: {
      getMessage: vi.fn((key: string) => key)
    },
    storage: {
      local: {
        get: vi.fn(async () => ({
          baseUrl: 'http://127.0.0.1:8042',
          token: options.token || '',
          submitKeyMode: 'enter',
          appearanceTheme: 'system'
        })),
        set: vi.fn(async () => undefined)
      },
      session: {
        get: vi.fn(storageGet),
        set: vi.fn(async (items: Record<string, unknown>) => {
          Object.assign(sessionState, structuredClone(items))
        }),
        remove: vi.fn(async (keys: string[] | string) => {
          for (const key of Array.isArray(keys) ? keys : [keys]) {
            delete sessionState[key]
          }
        })
      }
    },
    tabs: {
      query: vi.fn(async () => []),
      get: vi.fn(),
      create: vi.fn(async (properties: { url?: string }) => ({ id: 1, ...properties })),
      remove: vi.fn(),
      update: vi.fn(),
      reload: vi.fn(),
      captureVisibleTab: vi.fn(),
      onActivated: onTabActivated,
      onUpdated: onTabUpdated
    },
    windows: {
      onFocusChanged: onWindowFocusChanged
    },
    scripting: {
      executeScript: vi.fn(async () => [])
    },
    __onInstalledListeners: onInstalled.listeners,
    __onStartupListeners: onStartup.listeners,
    __onTabActivatedListeners: onTabActivated.listeners,
    __onTabUpdatedListeners: onTabUpdated.listeners,
    __onWindowFocusChangedListeners: onWindowFocusChanged.listeners,
    __contextMenuClickedListeners: onContextMenuClicked.listeners,
    __sessionState: sessionState,
    __onMessageListeners: onMessageListeners
  } as unknown as ChromeApi & {
    __onInstalledListeners: InstalledListener[]
    __onStartupListeners: Array<() => void>
    __onTabActivatedListeners: TabActivatedListener[]
    __onTabUpdatedListeners: TabUpdatedListener[]
    __onWindowFocusChangedListeners: Array<(windowId: number) => void>
    __contextMenuClickedListeners: ContextMenuClickListener[]
    __sessionState: Record<string, unknown>
    __onMessageListeners: MessageListener[]
  }

  return chromeApi
}

async function importServiceWorker(chromeApi: ChromeApi): Promise<void> {
  vi.resetModules()
  vi.stubGlobal('chrome', chromeApi)
  await import('./service_worker')
}

afterEach(() => {
  vi.restoreAllMocks()
  vi.unstubAllGlobals()
  vi.resetModules()
})

describe('service worker install handling', () => {
  it('opens the side panel page on first install', async () => {
    const chromeApi = createChromeApi()
    await importServiceWorker(chromeApi)

    chromeApi.__onInstalledListeners[0]({ reason: 'install' })

    expect(chromeApi.sidePanel?.setPanelBehavior).toHaveBeenCalledWith({
      openPanelOnActionClick: true
    })
    expect(chromeApi.tabs.create).toHaveBeenCalledWith({ url: 'index.html' })
  })

  it('does not open the side panel page for extension updates', async () => {
    const chromeApi = createChromeApi()
    await importServiceWorker(chromeApi)

    chromeApi.__onInstalledListeners[0]({ reason: 'update', previousVersion: '0.8.11' })

    expect(chromeApi.tabs.create).not.toHaveBeenCalled()
  })

  it('creates the page element context menu on install', async () => {
    const chromeApi = createChromeApi()
    await importServiceWorker(chromeApi)

    chromeApi.__onInstalledListeners[0]({ reason: 'install' })

    await vi.waitFor(() =>
      expect(chromeApi.contextMenus?.create).toHaveBeenCalledWith({
        id: pageElementContextMenuId,
        title: 'sendPageElementToChat',
        contexts: ['all']
      })
    )
  })
})

describe('service worker page element context menu', () => {
  it('opens the side panel and forwards the captured element as an attachment request', async () => {
    const chromeApi = createChromeApi()
    await importServiceWorker(chromeApi)
    const element: PageElementInfo = {
      tagName: 'BUTTON',
      id: 'submit',
      role: 'button',
      innerText: 'Submit',
      attributes: { id: 'submit', type: 'button' },
      xpath: '//*[@id="submit"]',
      cssPath: '#submit',
      pageUrl: 'https://example.com/form',
      pageTitle: 'Example form',
      frameUrl: 'https://example.com/form',
      selectedText: '',
      capturedAt: Date.now()
    }
    const openSidePanel = vi.mocked(chromeApi.sidePanel?.open)
    const executeScript = vi.fn(async () => [{ result: element }])
    chromeApi.scripting.executeScript = executeScript

    chromeApi.__contextMenuClickedListeners[0](
      { menuItemId: pageElementContextMenuId, pageUrl: 'https://example.com/form', frameId: 0 },
      { id: 7, windowId: 3 }
    )

    await vi.waitFor(() => expect(chromeApi.sidePanel?.open).toHaveBeenCalledWith({ tabId: 7 }))
    expect(openSidePanel?.mock.invocationCallOrder[0]).toBeLessThan(
      executeScript.mock.invocationCallOrder[0]
    )

    await vi.waitFor(() =>
      expect(chromeApi.__sessionState[pageElementAttachmentRequestStorageKey]).toMatchObject({
        element: {
          tagName: 'BUTTON',
          innerText: 'Submit'
        }
      })
    )
    const request = chromeApi.__sessionState[pageElementAttachmentRequestStorageKey]
    expect(chromeApi.scripting.executeScript).toHaveBeenCalledTimes(2)
    expect(chromeApi.scripting.executeScript).toHaveBeenNthCalledWith(
      1,
      expect.objectContaining({
        target: { tabId: 7, frameIds: [0] },
        args: [{ key: pageElementSerializerKey }]
      })
    )
    expect(chromeApi.scripting.executeScript).toHaveBeenNthCalledWith(
      2,
      expect.objectContaining({
        target: { tabId: 7, frameIds: [0] },
        args: [
          {
            domElementMemoryKey: pageElementDomMemoryKey,
            cssPath: '#submit',
            xpath: '//*[@id="submit"]'
          }
        ]
      })
    )
    expect(chromeApi.runtime.sendMessage).toHaveBeenCalledWith({
      type: pageElementAttachmentMessageType,
      pageElementRequest: request
    })
  })
})

describe('service worker development logging', () => {
  it('rejects stale RPC credentials instead of replacing persisted settings', async () => {
    const chromeApi = createChromeApi()
    await importServiceWorker(chromeApi)
    const sendResponse = vi.fn()
    chromeApi.__onMessageListeners[0](
      {
        type: 'anda_rpc',
        method: 'information',
        params: [],
        settings: {
          baseUrl: 'http://old-daemon',
          token: 'old-token',
          submitKeyMode: 'enter',
          appearanceTheme: 'system'
        }
      },
      extensionPage,
      sendResponse
    )
    await vi.waitFor(() =>
      expect(sendResponse).toHaveBeenCalledWith(
        expect.objectContaining({
          ok: false,
          error: expect.stringContaining('Connection settings changed')
        })
      )
    )
    expect(chromeApi.storage.local.set).not.toHaveBeenCalled()
  })

  it('redacts settings and omits payload bodies from development logs', async () => {
    const chromeApi = createChromeApi({ development: true })
    const consoleLog = vi.spyOn(console, 'log').mockImplementation(() => undefined)
    await importServiceWorker(chromeApi)
    const sendResponse = vi.fn()

    chromeApi.__onMessageListeners[0](
      {
        type: 'anda_status',
        settings: {
          baseUrl: 'http://127.0.0.1:8042',
          token: 'secret-token',
          submitKeyMode: 'enter',
          appearanceTheme: 'system'
        },
        text: 'private prompt'
      },
      extensionPage,
      sendResponse
    )

    await vi.waitFor(() => expect(sendResponse).toHaveBeenCalled())
    await vi.waitFor(() => expect(consoleLog).toHaveBeenCalled())

    const serializedLog = JSON.stringify(consoleLog.mock.calls[0])
    expect(serializedLog).toContain('<redacted>')
    expect(serializedLog).not.toContain('secret-token')
    expect(serializedLog).not.toContain('private prompt')
  })
})

describe('service worker lifecycle and routing', () => {
  it('does not inject content scripts into every tab at browser startup', async () => {
    const chromeApi = createChromeApi()
    await importServiceWorker(chromeApi)

    chromeApi.__onStartupListeners[0]()

    expect(chromeApi.tabs.query).not.toHaveBeenCalledWith({})
    expect(chromeApi.scripting.executeScript).not.toHaveBeenCalled()
  })

  it('ignores messages from content scripts', async () => {
    const chromeApi = createChromeApi()
    await importServiceWorker(chromeApi)
    const sendResponse = vi.fn()

    const handled = chromeApi.__onMessageListeners[0](
      { type: 'anda_status' },
      { url: 'https://example.com/page' },
      sendResponse
    )

    expect(handled).toBe(false)
    expect(sendResponse).not.toHaveBeenCalled()
  })

  it('keeps the focused tab when another window updates its active tab', async () => {
    const chromeApi = createChromeApi()
    chromeApi.tabs.get = vi.fn(async (tabId: number) => ({
      id: tabId,
      active: true,
      windowId: tabId === 5 ? 1 : 2
    }))
    await importServiceWorker(chromeApi)
    const { activeTab } = await import('$lib/service-worker/browser-tabs')

    chromeApi.__onTabActivatedListeners[0]({ tabId: 5, windowId: 1 })
    chromeApi.__onTabUpdatedListeners[0](
      9,
      { title: 'Background video' },
      { id: 9, active: true, windowId: 2 }
    )

    await expect(activeTab(chromeApi)).resolves.toMatchObject({ id: 5 })
  })

  async function connectedWorker() {
    const sockets: FakeWebSocket[] = []
    class FakeWebSocket {
      static OPEN = 1
      readyState = 0
      sent: Array<{ id?: number; method?: string; params?: unknown[] }> = []
      onopen: (() => void) | null = null
      onclose: (() => void) | null = null
      onerror: (() => void) | null = null
      onmessage: ((event: { data: string }) => void) | null = null

      constructor(readonly url: string) {
        sockets.push(this)
        setTimeout(() => {
          this.readyState = FakeWebSocket.OPEN
          this.onopen?.()
        })
      }

      send(data: string) {
        const message = JSON.parse(data)
        this.sent.push(message)
        if (typeof message.id === 'number') {
          setTimeout(() => this.onmessage?.({ data: JSON.stringify({ id: message.id }) }))
        }
      }

      close() {
        this.readyState = 3
        this.onclose?.()
      }
    }
    vi.stubGlobal('WebSocket', FakeWebSocket)

    const tab = { id: 5, active: true, windowId: 1, url: 'https://a.example/', title: 'A' }
    const chromeApi = createChromeApi({ token: 'secret' })
    chromeApi.tabs.query = vi.fn(async () => [tab])
    chromeApi.tabs.get = vi.fn(async () => ({ ...tab }))
    await importServiceWorker(chromeApi)

    const registrations = () =>
      sockets.flatMap((socket) => socket.sent).filter((m) => m.method === 'browser_register')
    await vi.waitFor(() => expect(registrations()).toHaveLength(1))
    return { chromeApi, tab, registrations }
  }

  it('skips unchanged tab updates and registers changed metadata', async () => {
    const { chromeApi, tab, registrations } = await connectedWorker()

    chromeApi.__onTabUpdatedListeners[0](5, { title: 'A' }, tab)
    await new Promise((resolve) => setTimeout(resolve, 300))
    expect(registrations()).toHaveLength(1)

    tab.title = 'B'
    chromeApi.__onTabUpdatedListeners[0](5, { title: 'B' }, tab)
    await vi.waitFor(() => expect(registrations()).toHaveLength(2))
    expect(registrations()[1].params).toEqual([
      expect.objectContaining({ tab_id: 5, title: 'B', url: 'https://a.example/' })
    ])
  })

  it('refreshes an unchanged session when its window regains focus, even alongside a tab update', async () => {
    const { chromeApi, tab, registrations } = await connectedWorker()

    // Losing focus must not make this profile the daemon's default browser.
    chromeApi.__onWindowFocusChangedListeners[0](-1)
    await new Promise((resolve) => setTimeout(resolve, 300))
    expect(registrations()).toHaveLength(1)

    chromeApi.__onWindowFocusChangedListeners[0](tab.windowId)
    await vi.waitFor(() =>
      expect(chromeApi.tabs.query).toHaveBeenCalledWith({ active: true, windowId: tab.windowId })
    )
    // A normal tab update may arrive during the same refresh debounce.
    chromeApi.__onTabUpdatedListeners[0](tab.id, { title: tab.title }, tab)
    await vi.waitFor(() => expect(registrations()).toHaveLength(2))
    expect(registrations()[1].params).toEqual(registrations()[0].params)

    // The focus refresh must not disable deduplication for later tab updates.
    chromeApi.__onTabUpdatedListeners[0](tab.id, { title: tab.title }, tab)
    await new Promise((resolve) => setTimeout(resolve, 300))
    expect(registrations()).toHaveLength(2)
  })
})
