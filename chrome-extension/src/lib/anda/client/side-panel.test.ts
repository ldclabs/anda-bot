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

describe('AndaSidePanelClient quick prompts', () => {
  it('loads, uses, removes, and clears local quick prompts', async () => {
    vi.useFakeTimers()
    vi.setSystemTime(new Date('2026-06-19T00:00:00Z'))
    const chromeApi = createChromeApi({
      quickPrompts: [
        {
          id: 'old',
          text: '  提交变更  ',
          createdAt: 1,
          updatedAt: 2,
          usedAt: 3,
          useCount: 4
        },
        {
          id: 'object',
          text: { bad: true },
          createdAt: 1,
          updatedAt: 1,
          usedAt: 0,
          useCount: 0
        } as unknown as QuickPrompt,
        { id: 'blank', text: '   ', createdAt: 1, updatedAt: 1, usedAt: 0, useCount: 0 }
      ]
    })
    vi.stubGlobal('chrome', chromeApi)
    const { AndaSidePanelClient } = await importSidePanelModule()
    const client = new AndaSidePanelClient()

    await client.loadQuickPrompts()

    expect(client.quickPrompts).toHaveLength(1)
    expect(client.quickPrompts[0]).toMatchObject({ text: '提交变更', useCount: 4 })
    expect(client.isQuickPrompt('提交变更')).toBe(true)

    await client.addQuickPrompt('写测试')
    expect(client.isQuickPrompt('写测试')).toBe(true)
    expect(chromeApi.storage.local.set).toHaveBeenLastCalledWith({
      quickPrompts: expect.arrayContaining([expect.objectContaining({ text: '写测试' })])
    })

    const reloaded = new AndaSidePanelClient()
    await reloaded.loadQuickPrompts()
    expect(reloaded.isQuickPrompt('提交变更')).toBe(true)
    expect(reloaded.isQuickPrompt('写测试')).toBe(true)

    await client.useQuickPrompt('提交变更')

    expect(client.quickPrompts.find((prompt) => prompt.text === '提交变更')).toMatchObject({
      text: '提交变更',
      useCount: 5
    })

    await client.removeQuickPrompt('写测试')
    expect(client.isQuickPrompt('写测试')).toBe(false)

    await client.clearQuickPrompts()
    expect(client.quickPrompts).toEqual([])
    expect(chromeApi.storage.local.set).toHaveBeenLastCalledWith({ quickPrompts: [] })
  })

  it('evicts the least-used quick prompt when the limit is exceeded', async () => {
    vi.useFakeTimers()
    vi.setSystemTime(new Date('2026-06-19T00:00:00Z'))
    const chromeApi = createChromeApi()
    vi.stubGlobal('chrome', chromeApi)
    const { AndaSidePanelClient, quickPromptsMaxItems } = await importSidePanelModule()
    const client = new AndaSidePanelClient()
    client.quickPrompts = Array.from({ length: quickPromptsMaxItems }, (_, index) => ({
      id: `prompt-${index}`,
      text: `prompt ${index}`,
      createdAt: index,
      updatedAt: index,
      usedAt: index,
      useCount: index === 0 ? 0 : 1
    }))

    await client.addQuickPrompt('new prompt')

    expect(client.quickPrompts).toHaveLength(quickPromptsMaxItems)
    expect(client.quickPrompts.some((prompt) => prompt.text === 'prompt 0')).toBe(false)
    expect(client.quickPrompts.some((prompt) => prompt.text === 'new prompt')).toBe(true)
  })

  it('does not show a quick prompt when local storage rejects the write', async () => {
    const chromeApi = createChromeApi({ storageSetError: new Error('storage unavailable') })
    vi.stubGlobal('chrome', chromeApi)
    const { AndaSidePanelClient } = await importSidePanelModule()
    const client = new AndaSidePanelClient()

    await client.addQuickPrompt('提交变更')

    expect(client.quickPrompts).toEqual([])
    expect(client.isQuickPrompt('提交变更')).toBe(false)
    expect(client.systemMessage).toEqual({
      kind: 'error',
      text: 'quickPromptsUpdateFailed:storage unavailable'
    })
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
    const speakAssistantText = vi
      .spyOn(client as any, 'speakAssistantText')
      .mockResolvedValue('chrome')

    await client.sendVoiceTurn({ transcript: 'hello', ttsEnabled: true })

    expect(speakAssistantText).toHaveBeenCalledTimes(1)
    expect(speakAssistantText).toHaveBeenCalledWith('spoken reply', 'chrome')
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

describe('AndaSidePanelClient bookmarks', () => {
  const tokenSettings: SettingsState = {
    baseUrl: 'http://127.0.0.1:8042',
    token: 'token',
    submitKeyMode: 'enter',
    appearanceTheme: 'system'
  }

  it('adds a bookmark optimistically and calls the daemon', async () => {
    const chromeApi = createChromeApi({ settings: { token: 'token' } })
    vi.stubGlobal('chrome', chromeApi)
    const { AndaSidePanelClient } = await importSidePanelModule()
    const client = new AndaSidePanelClient()
    client.settings = { ...tokenSettings }
    client.activeChannel = { source: 'cli:/tmp/ws' } as any
    const rpc = vi
      .spyOn(client, 'rpc')
      .mockResolvedValue({ output: { result: bookmark(), next_cursor: null } } as any)

    await client.toggleBookmark(message('m-1-0', 'hello'))

    expect(client.isBookmarked('m-1-0')).toBe(true)
    expect(rpc).toHaveBeenCalledWith('tool_call', [
      expect.objectContaining({
        name: 'bookmarks_api',
        args: expect.objectContaining({
          type: 'AddBookmark',
          message_id: 'm-1-0',
          source: 'cli:/tmp/ws',
          text: 'hello',
          folder_ids: []
        })
      })
    ])
  })

  it('rolls back the optimistic star when the daemon rejects the add', async () => {
    const chromeApi = createChromeApi({ settings: { token: 'token' } })
    vi.stubGlobal('chrome', chromeApi)
    const { AndaSidePanelClient } = await importSidePanelModule()
    const client = new AndaSidePanelClient()
    client.settings = { ...tokenSettings }
    client.activeChannel = { source: 'cli:/tmp/ws' } as any
    vi.spyOn(client, 'rpc').mockRejectedValue(new Error('boom'))

    await client.toggleBookmark(message('m-1-0', 'hello'))

    expect(client.isBookmarked('m-1-0')).toBe(false)
  })

  it('removes an existing bookmark', async () => {
    const chromeApi = createChromeApi({ settings: { token: 'token' } })
    vi.stubGlobal('chrome', chromeApi)
    const { AndaSidePanelClient } = await importSidePanelModule()
    const client = new AndaSidePanelClient()
    client.settings = { ...tokenSettings }
    client.bookmarkedIds.add('m-1-0')
    const rpc = vi.spyOn(client, 'rpc').mockResolvedValue({
      output: { result: { removed: true, conversation: 1, bookmark: null } }
    } as any)

    const removed = await client.removeBookmark('m-1-0')

    expect(removed).toBe(true)
    expect(client.isBookmarked('m-1-0')).toBe(false)
    expect(rpc).toHaveBeenCalledWith('tool_call', [
      expect.objectContaining({
        args: expect.objectContaining({ type: 'RemoveBookmark', message_id: 'm-1-0' })
      })
    ])
  })

  it('keeps the star when removing a bookmark fails', async () => {
    const chromeApi = createChromeApi({ settings: { token: 'token' } })
    vi.stubGlobal('chrome', chromeApi)
    const { AndaSidePanelClient } = await importSidePanelModule()
    const client = new AndaSidePanelClient()
    client.settings = { ...tokenSettings }
    client.bookmarkedIds.add('m-1-0')
    vi.spyOn(client, 'rpc').mockRejectedValue(new Error('boom'))

    const removed = await client.removeBookmark('m-1-0')

    expect(removed).toBe(false)
    expect(client.isBookmarked('m-1-0')).toBe(true)
  })

  it('loads one conversation bookmark into the star set and caches it', async () => {
    const chromeApi = createChromeApi({ settings: { token: 'token' } })
    vi.stubGlobal('chrome', chromeApi)
    const { AndaSidePanelClient } = await importSidePanelModule()
    const client = new AndaSidePanelClient()
    client.settings = { ...tokenSettings }
    const rpc = vi.spyOn(client, 'rpc').mockResolvedValue({
      output: {
        result: bookmark(1, [
          { index: 0, role: 'assistant', text: 'first' },
          { index: 2, role: 'assistant', text: 'third' }
        ])
      }
    } as any)

    await client.loadConversationBookmarks([1])
    await client.loadConversationBookmarks([1])

    expect(client.isBookmarked('m-1-0')).toBe(true)
    expect(client.isBookmarked('m-1-1')).toBe(false)
    expect(client.isBookmarked('m-1-2')).toBe(true)
    expect(rpc).toHaveBeenCalledTimes(1)
    expect(rpc).toHaveBeenCalledWith('tool_call', [
      expect.objectContaining({
        name: 'bookmarks_api',
        args: { type: 'GetConversationBookmark', conversation: 1 }
      })
    ])
  })

  it('returns a paginated bookmark page with its next cursor', async () => {
    const chromeApi = createChromeApi({ settings: { token: 'token' } })
    vi.stubGlobal('chrome', chromeApi)
    const { AndaSidePanelClient } = await importSidePanelModule()
    const client = new AndaSidePanelClient()
    client.settings = { ...tokenSettings }
    vi.spyOn(client, 'rpc').mockResolvedValue({
      output: { result: [bookmark()], next_cursor: 'cursor-1' }
    } as any)

    const page = await client.listBookmarks()

    expect(page.items).toHaveLength(1)
    expect(page.nextCursor).toBe('cursor-1')
  })

  it('loads bookmark markdown from the source conversation message', async () => {
    const chromeApi = createChromeApi({
      settings: { token: 'token' },
      activeTabs: [{ id: 1, url: 'https://example.com', title: 'Example', windowId: 1 }]
    })
    vi.stubGlobal('chrome', chromeApi)
    const { AndaSidePanelClient } = await importSidePanelModule()
    const client = new AndaSidePanelClient()
    client.settings = { ...tokenSettings }
    const rpc = vi.spyOn(client, 'rpc').mockResolvedValue({
      output: {
        result: {
          _id: 7,
          user: 'alice',
          messages: [
            { role: 'user', content: [{ type: 'Text', text: 'prompt' }] },
            { role: 'assistant', content: [{ type: 'Text', text: '**conversation markdown**' }] }
          ],
          status: 'completed',
          usage: { input_tokens: 0, output_tokens: 0, cached_tokens: 0, requests: 0 },
          created_at: 1,
          updated_at: 2
        }
      }
    } as any)

    const markdown = await client.getConversationMarkdownForBookmark({
      bookmark: bookmark(7, [{ index: 1, role: 'assistant', text: 'snapshot text' }]),
      message_id: 'm-7-1',
      message_index: 1,
      conversation: 7,
      source: 'cli:/tmp/ws/',
      role: 'assistant',
      folder_ids: [],
      text: 'snapshot text',
      created_at: 1
    })

    expect(markdown).toBe('**conversation markdown**')
    expect(rpc).toHaveBeenCalledWith('tool_call', [
      expect.objectContaining({
        name: 'conversations_api',
        args: { type: 'GetConversation', _id: 7 },
        meta: expect.objectContaining({
          source: 'cli:/tmp/ws/',
          workspace: '/tmp/ws',
          conversation: 7,
          browser_client: 'chrome_extension'
        })
      })
    ])
  })

  it('calls bookmark folder operations with daemon tool variants', async () => {
    const chromeApi = createChromeApi({ settings: { token: 'token' } })
    vi.stubGlobal('chrome', chromeApi)
    const { AndaSidePanelClient } = await importSidePanelModule()
    const client = new AndaSidePanelClient()
    client.settings = { ...tokenSettings }
    const rpc = vi.spyOn(client, 'rpc').mockResolvedValue({
      output: {
        result: {
          version: 1,
          next_folder_id: 2,
          folders: {
            '1': {
              id: 1,
              name: 'Work',
              parent_id: null,
              order: 1,
              created_at: 1,
              updated_at: 1
            }
          },
          updated_at: 1
        }
      }
    } as any)

    const folders = await client.createBookmarkFolder('Work')

    expect(folders.folders['1'].name).toBe('Work')
    expect(rpc).toHaveBeenLastCalledWith('tool_call', [
      expect.objectContaining({
        name: 'bookmarks_api',
        args: {
          type: 'CreateBookmarkFolder',
          name: 'Work',
          parent_id: null
        }
      })
    ])

    await client.renameBookmarkFolder(1, 'Reading')

    expect(rpc).toHaveBeenLastCalledWith('tool_call', [
      expect.objectContaining({
        name: 'bookmarks_api',
        args: {
          type: 'RenameBookmarkFolder',
          folder_id: 1,
          name: 'Reading'
        }
      })
    ])

    await client.moveBookmarkFolder(1, null, 10)

    expect(rpc).toHaveBeenLastCalledWith('tool_call', [
      expect.objectContaining({
        name: 'bookmarks_api',
        args: {
          type: 'MoveBookmarkFolder',
          folder_id: 1,
          parent_id: null,
          order: 10
        }
      })
    ])

    await client.deleteBookmarkFolder(1)

    expect(rpc).toHaveBeenLastCalledWith('tool_call', [
      expect.objectContaining({
        name: 'bookmarks_api',
        args: {
          type: 'DeleteBookmarkFolder',
          folder_id: 1
        }
      })
    ])
  })

  it('updates bookmark folder membership and lists a folder page', async () => {
    const chromeApi = createChromeApi({ settings: { token: 'token' } })
    vi.stubGlobal('chrome', chromeApi)
    const { AndaSidePanelClient } = await importSidePanelModule()
    const client = new AndaSidePanelClient()
    client.settings = { ...tokenSettings }
    const rpc = vi.spyOn(client, 'rpc').mockResolvedValue({
      output: {
        result: [{ ...bookmark(), folder_ids: [1] }],
        next_cursor: 'cursor-2'
      }
    } as any)

    const page = await client.listBookmarksInFolder(1)

    expect(page.nextCursor).toBe('cursor-2')
    expect(rpc).toHaveBeenLastCalledWith('tool_call', [
      expect.objectContaining({
        name: 'bookmarks_api',
        args: {
          type: 'ListBookmarksInFolder',
          folder_id: 1
        }
      })
    ])

    vi.mocked(rpc).mockResolvedValue({
      output: { result: { ...bookmark(), folder_ids: [1, 2] } }
    } as any)

    await client.addBookmarkToFolder('m-1-0', 2)

    expect(rpc).toHaveBeenLastCalledWith('tool_call', [
      expect.objectContaining({
        args: {
          type: 'AddBookmarkToFolder',
          message_id: 'm-1-0',
          folder_id: 2
        }
      })
    ])

    await client.setBookmarkFolders('m-1-0', [2])

    expect(rpc).toHaveBeenLastCalledWith('tool_call', [
      expect.objectContaining({
        args: {
          type: 'SetBookmarkFolders',
          message_id: 'm-1-0',
          folder_ids: [2]
        }
      })
    ])

    await client.removeBookmarkFromFolder('m-1-0', 1)

    expect(rpc).toHaveBeenLastCalledWith('tool_call', [
      expect.objectContaining({
        args: {
          type: 'RemoveBookmarkFromFolder',
          message_id: 'm-1-0',
          folder_id: 1
        }
      })
    ])
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

    vi.spyOn(client, 'rpc').mockImplementation(async (method) => {
      if (method === 'pick_workspace') {
        return { path: '/tmp/anda/workspace/' } as any
      }
      throw new Error(`unexpected RPC method: ${method}`)
    })
    const switchChannel = vi.spyOn(client, 'switchChannel').mockResolvedValue()

    await client.openWorkspaceChannel()

    expect(chromeApi.storage.local.set).toHaveBeenCalledWith(
      expect.objectContaining({
        workspaceChannelSources: ['cli:/tmp/anda/workspace']
      })
    )
    expect(switchChannel).toHaveBeenCalledWith('cli:/tmp/anda/workspace')
  })
})
