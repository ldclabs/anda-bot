import { afterEach, describe, expect, it, vi } from 'vitest'
import {
  pageElementAttachmentMessageType,
  pageElementAttachmentRequestStorageKey,
  pageElementContextMenuId,
  pageElementDomMemoryKey,
  pageElementMemoryKey,
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
  sender: unknown,
  sendResponse: (response: ExtensionResponse) => void
) => boolean | void

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

function createChromeApi(options: { development?: boolean } = {}) {
  const onInstalled = createChromeEvent<InstalledListener>()
  const onStartup = createChromeEvent<() => void>()
  const onActionClicked = createChromeEvent<(tab: { id?: number; windowId?: number }) => void>()
  const onContextMenuClicked = createChromeEvent<ContextMenuClickListener>()
  const onTabActivated =
    createChromeEvent<(activeInfo: { tabId: number; windowId: number }) => void>()
  const onTabUpdated =
    createChromeEvent<
      (tabId: number, changeInfo: { title?: string; url?: string }, tab: { id?: number }) => void
    >()
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
          token: '',
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
    scripting: {
      executeScript: vi.fn(async () => [])
    },
    __onInstalledListeners: onInstalled.listeners,
    __contextMenuClickedListeners: onContextMenuClicked.listeners,
    __sessionState: sessionState,
    __onMessageListeners: onMessageListeners
  } as unknown as ChromeApi & {
    __onInstalledListeners: InstalledListener[]
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
      className: 'primary',
      role: 'button',
      innerText: 'Submit',
      textContent: 'Submit',
      outerHTML: '<button id="submit">Submit</button>',
      attributes: { id: 'submit', type: 'button' },
      xpath: '//*[@id="submit"]',
      cssPath: '#submit',
      pageUrl: 'https://example.com/form',
      pageTitle: 'Example form',
      frameUrl: 'https://example.com/form',
      selectedText: '',
      rect: null,
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
          outerHTML: '<button id="submit">Submit</button>'
        }
      })
    )
    const request = chromeApi.__sessionState[pageElementAttachmentRequestStorageKey]
    expect(chromeApi.scripting.executeScript).toHaveBeenCalledTimes(2)
    expect(chromeApi.scripting.executeScript).toHaveBeenNthCalledWith(
      1,
      expect.objectContaining({
        target: { tabId: 7, frameIds: [0] },
        args: [{ key: pageElementMemoryKey }]
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
      {},
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
