import { afterEach, describe, expect, it, vi } from 'vitest'
import { PollConversation } from './poll-conversation'
import type { ChromeApi, ChromeTabInfo, QuickPrompt, SettingsState } from './types'

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

function bookmark(
  conversation = 1,
  messages: Array<{ index: number; role: 'assistant'; text: string }> = [
    { index: 0, role: 'assistant', text: 'hello' }
  ]
) {
  return {
    _id: conversation,
    user: 'alice',
    conversation,
    source: 'cli:/tmp/ws',
    folder_ids: [],
    messages,
    created_at: 1
  }
}

function createChromeApi(
  options: {
    settings?: Partial<SettingsState>
    activeTabs?: ChromeTabInfo[]
    browserSessionId?: string
    workspaceChannelSources?: string[]
    uiLanguage?: string
    chromeUiLanguage?: string
    quickPrompts?: QuickPrompt[]
    storageSetError?: Error
  } = {}
): MockChromeApi {
  const tabActivatedListeners: TabActivatedListener[] = []
  const tabUpdatedListeners: TabUpdatedListener[] = []
  const state = {
    baseUrl: 'http://127.0.0.1:8042',
    token: '',
    submitKeyMode: 'enter' as const,
    appearanceTheme: 'system' as const,
    browserSessionId: options.browserSessionId || '1700000000000',
    workspaceChannelSources: options.workspaceChannelSources || [],
    uiLanguage: options.uiLanguage,
    quickPrompts: options.quickPrompts || [],
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
      ),
      getUILanguage: vi.fn(() => options.chromeUiLanguage || 'en-US')
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
          if (options.storageSetError) {
            throw options.storageSetError
          }
          Object.assign(state, structuredClone(items))
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
  vi.useRealTimers()
  vi.restoreAllMocks()
  vi.unstubAllGlobals()
  vi.resetModules()
})

describe('AndaSidePanelClient.saveAppearanceTheme', () => {
  it('persists the appearance theme without saving unrelated draft fields', async () => {
    const chromeApi = createChromeApi({
      settings: { token: 'token', appearanceTheme: 'system' }
    })
    vi.stubGlobal('chrome', chromeApi)
    const { AndaSidePanelClient } = await importSidePanelModule()
    const client = new AndaSidePanelClient()

    client.settings = {
      baseUrl: 'http://127.0.0.1:8042',
      token: 'token',
      submitKeyMode: 'enter',
      appearanceTheme: 'system'
    }

    await client.saveAppearanceTheme('dark')

    expect(client.settings.appearanceTheme).toBe('dark')
    expect(chromeApi.storage.local.set).toHaveBeenCalledWith({ appearanceTheme: 'dark' })
    expect(chromeApi.runtime.sendMessage).toHaveBeenCalledWith(
      expect.objectContaining({
        type: 'anda_settings_changed',
        settings: expect.objectContaining({
          baseUrl: 'http://127.0.0.1:8042',
          token: 'token',
          submitKeyMode: 'enter',
          appearanceTheme: 'dark'
        })
      })
    )
  })
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
      submitKeyMode: 'enter',
      appearanceTheme: 'system'
    }
    client.activeChannel = {
      sendPrompt: vi.fn().mockResolvedValue(poller)
    } as any

    vi.spyOn(client as any, 'refreshActiveTab').mockResolvedValue(null)
    // Playback itself is covered in voice-session.test.ts; here we only assert
    // which messages the turn hands to it.
    const speak = vi.spyOn(client.voice, 'speak').mockResolvedValue('chrome')

    await client.sendVoiceTurn({ transcript: 'hello', ttsEnabled: true })

    expect(speak).toHaveBeenCalledTimes(1)
    expect(speak).toHaveBeenCalledWith('spoken reply', 'chrome')
    expect(client.status).toBe('idle')
  })
})

describe('AndaSidePanelClient.stopActiveTask', () => {
  it('sends /stop even while a normal prompt send is marked in progress', async () => {
    const chromeApi = createChromeApi({
      settings: { token: 'token' }
    })
    vi.stubGlobal('chrome', chromeApi)
    const { AndaSidePanelClient } = await importSidePanelModule()
    const client = new AndaSidePanelClient()
    const sendPrompt = vi.fn().mockResolvedValue(null)

    client.settings = {
      baseUrl: 'http://127.0.0.1:8042',
      token: 'token',
      submitKeyMode: 'enter',
      appearanceTheme: 'system'
    }
    client.sending = true
    client.activeChannel = {
      sendPrompt
    } as any

    await client.stopActiveTask()

    expect(sendPrompt).toHaveBeenCalledWith('/stop', [])
  })
})

describe('AndaSidePanelClient.requestExtra', () => {
  it('includes the synced launcher language in request metadata', async () => {
    const chromeApi = createChromeApi({
      activeTabs: [{ id: 1, url: 'https://example.com', title: 'Example', windowId: 2 }],
      uiLanguage: 'zh-Hans'
    })
    vi.stubGlobal('chrome', chromeApi)
    vi.stubGlobal('navigator', { language: 'fr-FR' })
    const { AndaSidePanelClient } = await importSidePanelModule()
    const client = new AndaSidePanelClient()

    const extra = await client.requestExtra()

    expect(extra).toMatchObject({
      browser_client: 'chrome_extension',
      language: 'zh_CN',
      tab: {
        id: 1,
        url: 'https://example.com',
        title: 'Example',
        window: 2
      }
    })
  })

  it('falls back to navigator.language when no launcher language is stored', async () => {
    const chromeApi = createChromeApi({ chromeUiLanguage: 'en-US' })
    vi.stubGlobal('chrome', chromeApi)
    vi.stubGlobal('navigator', { language: 'fr-FR' })
    const { AndaSidePanelClient } = await importSidePanelModule()
    const client = new AndaSidePanelClient()

    const extra = await client.requestExtra()

    expect(extra.language).toBe('fr-FR')
  })
})

describe('AndaSidePanelClient.sendPrompt', () => {
  it.each([
    ['steer', '/steer correct course'],
    ['new', '/new fresh start'],
    ['stop', '/stop wrong branch'],
    ['cancel', '/cancel abandon session']
  ])('allows %s commands through the global sending lock', async (_name, prompt) => {
    const chromeApi = createChromeApi({
      settings: { token: 'token' }
    })
    vi.stubGlobal('chrome', chromeApi)
    const { AndaSidePanelClient } = await importSidePanelModule()
    const client = new AndaSidePanelClient()
    const sendPrompt = vi.fn().mockResolvedValue(null)

    client.settings = {
      baseUrl: 'http://127.0.0.1:8042',
      token: 'token',
      submitKeyMode: 'enter',
      appearanceTheme: 'system'
    }
    client.sending = true
    client.activeChannel = {
      sendPrompt
    } as any
    vi.spyOn(client as any, 'refreshActiveTab').mockResolvedValue(null)

    await client.sendPrompt(prompt)

    expect(sendPrompt).toHaveBeenCalledWith(prompt, [])
    expect(client.sending).toBe(true)
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

describe('AndaSidePanelClient.openWorkspaceChannel', () => {
  it('persists a CLI workspace channel source and switches to it', async () => {
    const chromeApi = createChromeApi({
      settings: { token: 'token' }
    })
    vi.stubGlobal('chrome', chromeApi)
    const { AndaSidePanelClient } = await importSidePanelModule()
    const client = new AndaSidePanelClient()

    client.settings = {
      baseUrl: 'http://127.0.0.1:8042',
      token: 'token',
      submitKeyMode: 'enter',
      appearanceTheme: 'system'
    }

    const rpc = vi.spyOn(client, 'rpc').mockImplementation(async (method) => {
      if (method === 'pick_workspace') {
        return { path: '/tmp/anda/workspace/' } as any
      }
      if (method === 'register_workspace') {
        return { workspace: '/tmp/anda/workspace' } as any
      }
      if (method === 'tool_call') {
        return { output: { result: {} } } as any
      }
      throw new Error(`unexpected RPC method: ${method}`)
    })

    await client.openWorkspaceChannel()

    expect(chromeApi.storage.local.set).toHaveBeenCalledWith(
      expect.objectContaining({
        workspaceChannelSources: ['cli:/tmp/anda/workspace']
      })
    )
    expect(rpc).toHaveBeenCalledWith('register_workspace', ['/tmp/anda/workspace'])
    expect(client.activeSource).toBe('cli:/tmp/anda/workspace')
  })

  it('registers an existing directory channel before activating it', async () => {
    const chromeApi = createChromeApi({ settings: { token: 'token' } })
    vi.stubGlobal('chrome', chromeApi)
    const { AndaSidePanelClient } = await importSidePanelModule()
    const client = new AndaSidePanelClient()
    client.settings = { ...client.settings, token: 'token' }
    const rpc = vi.spyOn(client, 'rpc').mockImplementation(async (method) => {
      if (method === 'register_workspace') {
        return { workspace: '/tmp/project' } as any
      }
      if (method === 'tool_call') {
        return { output: { result: {} } } as any
      }
      throw new Error(`unexpected RPC method: ${method}`)
    })

    await client.switchChannel('/tmp/project')

    expect(rpc).toHaveBeenCalledWith('register_workspace', ['/tmp/project'])
    expect(client.activeSource).toBe('/tmp/project')
  })

  it('keeps the previous channel when directory registration fails', async () => {
    const chromeApi = createChromeApi({ settings: { token: 'token' } })
    vi.stubGlobal('chrome', chromeApi)
    const { AndaSidePanelClient } = await importSidePanelModule()
    const client = new AndaSidePanelClient()
    client.settings = { ...client.settings, token: 'token' }
    vi.spyOn(client, 'rpc').mockImplementation(async (method) => {
      if (method === 'register_workspace') {
        throw new Error('registration denied')
      }
      if (method === 'tool_call') {
        return { output: { result: {} } } as any
      }
      throw new Error(`unexpected RPC method: ${method}`)
    })
    await client.switchChannel('browser:chrome:1')

    await client.switchChannel('cli:/tmp/project')

    expect(client.activeSource).toBe('browser:chrome:1')
    expect(client.systemMessage?.text).toContain('registration denied')
  })

  it('does not reactivate a folder after a newer channel switch', async () => {
    const chromeApi = createChromeApi({ settings: { token: 'token' } })
    vi.stubGlobal('chrome', chromeApi)
    const { AndaSidePanelClient } = await importSidePanelModule()
    const client = new AndaSidePanelClient()
    client.settings = { ...client.settings, token: 'token' }
    let finishRegistration!: (value: unknown) => void
    const registration = new Promise<unknown>((resolve) => {
      finishRegistration = resolve
    })
    vi.spyOn(client, 'rpc').mockImplementation(async (method) => {
      if (method === 'register_workspace') {
        return registration as any
      }
      if (method === 'tool_call') {
        return { output: { result: {} } } as any
      }
      throw new Error(`unexpected RPC method: ${method}`)
    })

    const folderSwitch = client.switchChannel('cli:/tmp/project')
    await client.switchChannel('browser:chrome:1')
    finishRegistration({ workspace: '/tmp/project' })
    await folderSwitch

    expect(client.activeSource).toBe('browser:chrome:1')
  })
})
