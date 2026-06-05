import { afterEach, describe, expect, it, vi } from 'vitest'
import type {
  ChromeApi,
  ChromeRuntimeOnInstalledDetails,
  ExtensionMessage,
  ExtensionResponse
} from '$lib/service-worker/types'

type InstalledListener = (details: ChromeRuntimeOnInstalledDetails) => void
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
  const onTabActivated =
    createChromeEvent<(activeInfo: { tabId: number; windowId: number }) => void>()
  const onTabUpdated =
    createChromeEvent<
      (tabId: number, changeInfo: { title?: string; url?: string }, tab: { id?: number }) => void
    >()
  const onMessageListeners: MessageListener[] = []

  const chromeApi = {
    runtime: {
      onInstalled,
      onStartup,
      sendMessage: vi.fn(),
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
          submitKeyMode: 'enter'
        })),
        set: vi.fn(async () => undefined)
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
      executeScript: vi.fn()
    },
    __onInstalledListeners: onInstalled.listeners,
    __onMessageListeners: onMessageListeners
  } as unknown as ChromeApi & {
    __onInstalledListeners: InstalledListener[]
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
          submitKeyMode: 'enter'
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
