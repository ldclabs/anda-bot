import { afterEach, describe, expect, it, vi } from 'vitest'
import { PollConversation } from './poll-conversation'
import type { ChromeApi, ChromeTabInfo, SettingsState } from './types'

type TabActivatedListener = (activeInfo: { tabId: number; windowId: number }) => void
type TabUpdatedListener = (
  tabId: number,
  changeInfo: { title?: string; url?: string },
  tab: ChromeTabInfo
) => void

type MockChromeApi = ChromeApi & {
  __tabActivatedListeners: TabActivatedListener[]
  __tabUpdatedListeners: TabUpdatedListener[]
}

function message(id: string, text: string) {
  return {
    id,
    conversation: 1,
    role: 'assistant' as const,
    text,
    timestamp: 1
  }
}

function createChromeApi(
  options: {
    settings?: Partial<SettingsState>
    activeTabs?: ChromeTabInfo[]
    browserSessionId?: string
  } = {}
): MockChromeApi {
  const tabActivatedListeners: TabActivatedListener[] = []
  const tabUpdatedListeners: TabUpdatedListener[] = []
  const state = {
    baseUrl: 'http://127.0.0.1:8042',
    token: '',
    submitKeyMode: 'enter' as const,
    browserSessionId: options.browserSessionId || '1700000000000',
    ...options.settings
  }

  const chromeApi = {
    runtime: {
      onInstalled: {
        addListener: vi.fn(),
        removeListener: vi.fn()
      },
      onStartup: {
        addListener: vi.fn(),
        removeListener: vi.fn()
      },
      sendMessage: vi.fn(async (message) => {
        switch (message.type) {
          case 'anda_chrome_tts_available':
            return { ok: true, result: { available: true } }
          default:
            return { ok: true, result: {} }
        }
      }),
      onMessage: {
        addListener: vi.fn()
      }
    },
    action: {
      onClicked: {
        addListener: vi.fn(),
        removeListener: vi.fn()
      }
    },
    extension: {
      inIncognitoContext: false
    },
    i18n: {
      getMessage: vi.fn((key: string, substitutions?: string[]) =>
        substitutions?.length ? `${key}:${substitutions.join(',')}` : key
      )
    },
    storage: {
      local: {
        get: vi.fn(async (keys: string[]) => {
          const result: Record<string, unknown> = {}
          for (const key of keys) {
            if (key in state) {
              result[key] = state[key as keyof typeof state]
            }
          }
          return result
        }),
        set: vi.fn(async (items: Record<string, unknown>) => {
          Object.assign(state, items)
        })
      }
    },
    tabs: {
      query: vi.fn(async () => options.activeTabs || []),
      get: vi.fn(),
      create: vi.fn(),
      remove: vi.fn(),
      update: vi.fn(),
      reload: vi.fn(),
      captureVisibleTab: vi.fn(),
      onActivated: {
        addListener: vi.fn((listener: TabActivatedListener) => {
          tabActivatedListeners.push(listener)
        }),
        removeListener: vi.fn((listener: TabActivatedListener) => {
          const index = tabActivatedListeners.indexOf(listener)
          if (index >= 0) {
            tabActivatedListeners.splice(index, 1)
          }
        })
      },
      onUpdated: {
        addListener: vi.fn((listener: TabUpdatedListener) => {
          tabUpdatedListeners.push(listener)
        }),
        removeListener: vi.fn((listener: TabUpdatedListener) => {
          const index = tabUpdatedListeners.indexOf(listener)
          if (index >= 0) {
            tabUpdatedListeners.splice(index, 1)
          }
        })
      }
    },
    scripting: {
      executeScript: vi.fn()
    },
    __tabActivatedListeners: tabActivatedListeners,
    __tabUpdatedListeners: tabUpdatedListeners
  }

  return chromeApi as unknown as MockChromeApi
}

async function importSidePanelModule() {
  vi.resetModules()
  return import('./side-panel.svelte')
}

afterEach(() => {
  vi.restoreAllMocks()
  vi.unstubAllGlobals()
  vi.resetModules()
})

describe('AndaSidePanelClient.sendVoiceTurn', () => {
  it('continues playback polling after non-spoken assistant messages', async () => {
    const chromeApi = createChromeApi({
      settings: { token: 'token' }
    })
    vi.stubGlobal('chrome', chromeApi)
    const { AndaSidePanelClient } = await importSidePanelModule()
    const client = new AndaSidePanelClient()

    const poller = new PollConversation()
    poller.push(message('m-1', '<thinking>draft</thinking>'), message('m-2', 'spoken reply'))
    poller.finish()

    client.settings = {
      baseUrl: 'http://127.0.0.1:8042',
      token: 'token',
      submitKeyMode: 'enter'
    }
    client.activeChannel = {
      sendPrompt: vi.fn().mockResolvedValue(poller)
    } as any

    vi.spyOn(client as any, 'refreshActiveTab').mockResolvedValue(null)
    const speakAssistantText = vi
      .spyOn(client as any, 'speakAssistantText')
      .mockResolvedValue('chrome')

    await client.sendVoiceTurn({ transcript: 'hello', ttsEnabled: true })

    expect(speakAssistantText).toHaveBeenCalledTimes(1)
    expect(speakAssistantText).toHaveBeenCalledWith('spoken reply', 'chrome')
    expect(client.status).toBe('idle')
  })
})

describe('AndaSidePanelClient.bindChromeEvents', () => {
  it('ignores tab update events until the active tab is known', async () => {
    const chromeApi = createChromeApi({ activeTabs: [] })
    vi.stubGlobal('chrome', chromeApi)
    const { AndaSidePanelClient } = await importSidePanelModule()
    const client = new AndaSidePanelClient()

    await client.init()

    expect(client.tab).toBeNull()
    expect(chromeApi.__tabUpdatedListeners).toHaveLength(1)

    chromeApi.__tabUpdatedListeners[0](
      42,
      { title: 'Updated title' },
      { id: 42, title: 'Updated title', url: 'https://example.com' }
    )

    expect(client.tab).toBeNull()
  })

  it('keeps the tracked active tab in sync when the current tab changes', async () => {
    const activeTab = { id: 7, title: 'Before', url: 'https://before.example' }
    const chromeApi = createChromeApi({ activeTabs: [activeTab] })
    vi.stubGlobal('chrome', chromeApi)
    const { AndaSidePanelClient } = await importSidePanelModule()
    const client = new AndaSidePanelClient()

    await client.init()

    chromeApi.__tabUpdatedListeners[0](
      7,
      { title: 'After' },
      { id: 7, title: 'After', url: 'https://after.example' }
    )

    expect(client.tab).toEqual({
      id: 7,
      title: 'After',
      url: 'https://after.example'
    })
  })
})
