import { describe, expect, it, vi } from 'vitest'
import { BookmarksApi, type BookmarksContext } from './bookmarks.svelte'
import type { DaemonApi } from './daemon'
import type { Bookmark, BookmarkedMessage, ChatMessage } from './types'

/**
 * These tests exercise `BookmarksApi` through the `DaemonApi` seam, so none of
 * them needs a fake `chrome` global.
 */
function createDaemon(output: unknown, overrides: Partial<DaemonApi> = {}) {
  const toolCall = vi.fn(async () => ({ output, usage: {} }) as never)
  const daemon: DaemonApi = {
    authorized: true,
    rpc: vi.fn(async () => undefined as never),
    toolCall,
    ...overrides
  }
  return { daemon, toolCall }
}

function createContext(overrides: Partial<BookmarksContext> = {}): BookmarksContext {
  return {
    activeSource: () => 'cli:/tmp/ws',
    bookmarkRequestMeta: async (item) => ({
      source: item.source,
      workspace: '/tmp/ws',
      conversation: item.conversation,
      browser_client: 'chrome_extension'
    }),
    reportError: vi.fn(),
    ...overrides
  }
}

function bookmark(
  conversation = 1,
  messages: Array<{ index: number; role: 'assistant'; text: string }> = [
    { index: 0, role: 'assistant', text: 'hello' }
  ]
): Bookmark {
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

function message(id: string, text: string): ChatMessage {
  return { id, conversation: 1, role: 'assistant', text, timestamp: 1 }
}

function bookmarkedMessage(overrides: Partial<BookmarkedMessage> = {}): BookmarkedMessage {
  return {
    bookmark: bookmark(7, [{ index: 1, role: 'assistant', text: 'snapshot text' }]),
    message_id: 'm-7-1',
    message_index: 1,
    conversation: 7,
    source: 'cli:/tmp/ws/',
    role: 'assistant',
    folder_ids: [],
    text: 'snapshot text',
    created_at: 1,
    ...overrides
  }
}

describe('BookmarksApi starring', () => {
  it('adds a bookmark optimistically and calls the daemon', async () => {
    const { daemon, toolCall } = createDaemon({ result: bookmark(), next_cursor: null })
    const api = new BookmarksApi(daemon, createContext())

    await api.toggle(message('m-1-0', 'hello'))

    expect(api.isBookmarked('m-1-0')).toBe(true)
    expect(toolCall).toHaveBeenCalledWith(
      'bookmarks_api',
      expect.objectContaining({
        type: 'AddBookmark',
        message_id: 'm-1-0',
        source: 'cli:/tmp/ws',
        text: 'hello',
        folder_ids: []
      })
    )
  })

  it('rolls back the optimistic star and reports when the daemon rejects the add', async () => {
    const { daemon } = createDaemon(null, {
      toolCall: vi.fn(async () => {
        throw new Error('boom')
      })
    })
    const context = createContext()
    const api = new BookmarksApi(daemon, context)

    await api.toggle(message('m-1-0', 'hello'))

    expect(api.isBookmarked('m-1-0')).toBe(false)
    expect(context.reportError).toHaveBeenCalled()
  })

  it('removes an existing bookmark', async () => {
    const { daemon, toolCall } = createDaemon({
      result: { removed: true, conversation: 1, bookmark: null }
    })
    const api = new BookmarksApi(daemon, createContext())
    api.bookmarkedIds.add('m-1-0')

    expect(await api.remove('m-1-0')).toBe(true)
    expect(api.isBookmarked('m-1-0')).toBe(false)
    expect(toolCall).toHaveBeenCalledWith('bookmarks_api', {
      type: 'RemoveBookmark',
      message_id: 'm-1-0'
    })
  })

  it('keeps the star when removing a bookmark fails', async () => {
    const { daemon } = createDaemon(null, {
      toolCall: vi.fn(async () => {
        throw new Error('boom')
      })
    })
    const api = new BookmarksApi(daemon, createContext())
    api.bookmarkedIds.add('m-1-0')

    expect(await api.remove('m-1-0')).toBe(false)
    expect(api.isBookmarked('m-1-0')).toBe(true)
  })

  it('does nothing at all without a token', async () => {
    const { daemon, toolCall } = createDaemon({ result: bookmark() }, { authorized: false })
    const api = new BookmarksApi(daemon, createContext())

    await api.toggle(message('m-1-0', 'hello'))
    await api.loadConversations([1])

    expect(toolCall).not.toHaveBeenCalled()
    expect(api.isBookmarked('m-1-0')).toBe(false)
  })
})

describe('BookmarksApi.loadConversations', () => {
  it('loads one conversation into the star set and caches it', async () => {
    const { daemon, toolCall } = createDaemon({
      result: bookmark(1, [
        { index: 0, role: 'assistant', text: 'first' },
        { index: 2, role: 'assistant', text: 'third' }
      ])
    })
    const api = new BookmarksApi(daemon, createContext())

    await api.loadConversations([1])
    await api.loadConversations([1])

    expect(api.isBookmarked('m-1-0')).toBe(true)
    expect(api.isBookmarked('m-1-1')).toBe(false)
    expect(api.isBookmarked('m-1-2')).toBe(true)
    expect(toolCall).toHaveBeenCalledTimes(1)
    expect(toolCall).toHaveBeenCalledWith('bookmarks_api', {
      type: 'GetConversationBookmark',
      conversation: 1
    })
  })

  it('refetches when forced', async () => {
    const { daemon, toolCall } = createDaemon({ result: bookmark(1) })
    const api = new BookmarksApi(daemon, createContext())

    await api.loadConversation(1)
    await api.loadConversation(1, { force: true })

    expect(toolCall).toHaveBeenCalledTimes(2)
  })

  it('skips ids that are not conversations', async () => {
    const { daemon, toolCall } = createDaemon({ result: null })
    const api = new BookmarksApi(daemon, createContext())

    await api.loadConversations([0, -3, Number.NaN])

    expect(toolCall).not.toHaveBeenCalled()
  })
})

describe('BookmarksApi listing', () => {
  it('returns a paginated bookmark page with its next cursor', async () => {
    const { daemon } = createDaemon({ result: [bookmark()], next_cursor: 'cursor-1' })
    const api = new BookmarksApi(daemon, createContext())

    const page = await api.list()

    expect(page.items).toHaveLength(1)
    expect(page.nextCursor).toBe('cursor-1')
  })

  it('lists a folder page and omits absent paging arguments', async () => {
    const { daemon, toolCall } = createDaemon({
      result: [{ ...bookmark(), folder_ids: [1] }],
      next_cursor: 'cursor-2'
    })
    const api = new BookmarksApi(daemon, createContext())

    const page = await api.listInFolder(1)

    expect(page.nextCursor).toBe('cursor-2')
    expect(toolCall).toHaveBeenLastCalledWith('bookmarks_api', {
      type: 'ListBookmarksInFolder',
      folder_id: 1
    })
  })

  it('passes the cursor and limit through when given', async () => {
    const { daemon, toolCall } = createDaemon({ result: [], next_cursor: null })
    const api = new BookmarksApi(daemon, createContext())

    await api.list('cursor-1', 20)

    expect(toolCall).toHaveBeenLastCalledWith('bookmarks_api', {
      type: 'ListBookmarks',
      cursor: 'cursor-1',
      limit: 20
    })
  })

  it('answers with empty results without a token', async () => {
    const { daemon, toolCall } = createDaemon({ result: [] }, { authorized: false })
    const api = new BookmarksApi(daemon, createContext())

    expect(await api.list()).toEqual({ items: [], nextCursor: null })
    expect((await api.listFolders()).folders).toEqual({})
    expect(toolCall).not.toHaveBeenCalled()
  })
})

describe('BookmarksApi folder operations', () => {
  const foldersOutput = {
    result: {
      version: 1,
      next_folder_id: 2,
      folders: {
        '1': { _id: 1, name: 'Work', parent_id: null, order: 1, created_at: 1, updated_at: 1 }
      },
      updated_at: 1
    }
  }

  it('calls each folder tool variant', async () => {
    const { daemon, toolCall } = createDaemon(foldersOutput)
    const api = new BookmarksApi(daemon, createContext())

    const folders = await api.createFolder('Work')
    expect(folders.folders['1'].name).toBe('Work')
    expect(toolCall).toHaveBeenLastCalledWith('bookmarks_api', {
      type: 'CreateBookmarkFolder',
      name: 'Work',
      parent_id: null
    })

    await api.renameFolder(1, 'Reading')
    expect(toolCall).toHaveBeenLastCalledWith('bookmarks_api', {
      type: 'RenameBookmarkFolder',
      folder_id: 1,
      name: 'Reading'
    })

    await api.moveFolder(1, null, 10)
    expect(toolCall).toHaveBeenLastCalledWith('bookmarks_api', {
      type: 'MoveBookmarkFolder',
      folder_id: 1,
      parent_id: null,
      order: 10
    })

    await api.deleteFolder(1)
    expect(toolCall).toHaveBeenLastCalledWith('bookmarks_api', {
      type: 'DeleteBookmarkFolder',
      folder_id: 1
    })
  })

  it('falls back to empty folders when the daemon returns null', async () => {
    const { daemon } = createDaemon({ result: null })
    const api = new BookmarksApi(daemon, createContext())

    expect((await api.createFolder('Work')).folders).toEqual({})
  })

  it('updates folder membership and refreshes the star set', async () => {
    const { daemon, toolCall } = createDaemon({
      result: { ...bookmark(), folder_ids: [1, 2] }
    })
    const api = new BookmarksApi(daemon, createContext())

    await api.addToFolder('m-1-0', 2)
    expect(toolCall).toHaveBeenLastCalledWith('bookmarks_api', {
      type: 'AddBookmarkToFolder',
      message_id: 'm-1-0',
      folder_id: 2
    })
    expect(api.isBookmarked('m-1-0')).toBe(true)

    await api.setFolders('m-1-0', [2])
    expect(toolCall).toHaveBeenLastCalledWith('bookmarks_api', {
      type: 'SetBookmarkFolders',
      message_id: 'm-1-0',
      folder_ids: [2]
    })

    await api.removeFromFolder('m-1-0', 1)
    expect(toolCall).toHaveBeenLastCalledWith('bookmarks_api', {
      type: 'RemoveBookmarkFromFolder',
      message_id: 'm-1-0',
      folder_id: 1
    })
  })
})

describe('BookmarksApi.conversationMarkdown', () => {
  it('loads the markdown from the source conversation message', async () => {
    const { daemon, toolCall } = createDaemon({
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
    })
    const api = new BookmarksApi(daemon, createContext())

    const markdown = await api.conversationMarkdown(bookmarkedMessage())

    expect(markdown).toBe('**conversation markdown**')
    expect(toolCall).toHaveBeenCalledWith(
      'conversations_api',
      { type: 'GetConversation', _id: 7 },
      [],
      expect.objectContaining({
        source: 'cli:/tmp/ws/',
        workspace: '/tmp/ws',
        conversation: 7,
        browser_client: 'chrome_extension'
      })
    )
  })

  it('returns empty text when the conversation no longer holds the message', async () => {
    const { daemon } = createDaemon({
      result: {
        _id: 7,
        user: 'alice',
        messages: [],
        status: 'completed',
        usage: { input_tokens: 0, output_tokens: 0, cached_tokens: 0, requests: 0 },
        created_at: 1,
        updated_at: 2
      }
    })
    const api = new BookmarksApi(daemon, createContext())

    expect(await api.conversationMarkdown(bookmarkedMessage())).toBe('')
  })

  it('does not call the daemon for a malformed bookmark', async () => {
    const { daemon, toolCall } = createDaemon({ result: null })
    const api = new BookmarksApi(daemon, createContext())

    expect(await api.conversationMarkdown(bookmarkedMessage({ message_index: -1 }))).toBe('')
    expect(await api.conversationMarkdown(bookmarkedMessage({ conversation: 0 }))).toBe('')
    expect(toolCall).not.toHaveBeenCalled()
  })
})
