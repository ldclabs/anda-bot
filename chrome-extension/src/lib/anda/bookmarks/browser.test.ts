import { describe, expect, it, vi } from 'vitest'
import {
  BookmarkBrowser,
  bookmarkMessageItems,
  bookmarkPreviewText,
  compareBookmarkItems,
  emptyBookmarkFolders,
  type BookmarkPage,
  type BookmarkStore
} from './browser.svelte'
import type { Bookmark, BookmarkFolders } from '../client/types'

function bookmark(
  conversation: number,
  messages: Array<{ index: number; text: string }> = [{ index: 0, text: 'hello' }],
  folderIds: number[] = []
): Bookmark {
  return {
    _id: conversation,
    user: 'alice',
    conversation,
    source: 'cli:/tmp/ws',
    folder_ids: folderIds,
    messages: messages.map((message) => ({
      index: message.index,
      role: 'assistant' as const,
      text: message.text
    })),
    created_at: 1000 + conversation
  }
}

function folders(names: Record<number, string>): BookmarkFolders {
  const state = emptyBookmarkFolders()
  for (const [id, name] of Object.entries(names)) {
    state.folders[id] = {
      _id: Number(id),
      name,
      parent_id: null,
      order: Number(id),
      created_at: 0,
      updated_at: 0
    }
  }
  state.next_folder_id = Object.keys(names).length + 1
  return state
}

function page(items: Bookmark[], nextCursor: string | null = null): BookmarkPage {
  return { items, nextCursor }
}

function createStore(overrides: Partial<BookmarkStore> = {}): BookmarkStore {
  return {
    list: vi.fn(async () => page([])),
    listInFolder: vi.fn(async () => page([])),
    listFolders: vi.fn(async () => emptyBookmarkFolders()),
    createFolder: vi.fn(async () => emptyBookmarkFolders()),
    deleteFolder: vi.fn(async () => emptyBookmarkFolders()),
    addToFolder: vi.fn(async () => bookmark(1)),
    removeFromFolder: vi.fn(async () => bookmark(1)),
    remove: vi.fn(async () => true),
    ...overrides
  }
}

describe('bookmarkMessageItems', () => {
  it('expands a bookmark into one item per marked message, newest first', () => {
    const items = bookmarkMessageItems(
      bookmark(7, [
        { index: 0, text: 'first' },
        { index: 3, text: 'later' }
      ])
    )

    expect(items.map((item) => item.message_id)).toEqual(['m-7-3', 'm-7-0'])
    expect(items[0].conversation).toBe(7)
    expect(items[0].source).toBe('cli:/tmp/ws')
  })

  it('drops messages without a usable index', () => {
    const raw = bookmark(1, [{ index: 0, text: 'kept' }])
    raw.messages.push({ index: -1, role: 'assistant', text: 'dropped' })
    raw.messages.push({ index: Number.NaN, role: 'assistant', text: 'dropped' })

    expect(bookmarkMessageItems(raw).map((item) => item.text)).toEqual(['kept'])
  })
})

describe('compareBookmarkItems', () => {
  it('orders newer conversations, then later messages, first', () => {
    const items = [
      ...bookmarkMessageItems(bookmark(1, [{ index: 0, text: 'old' }])),
      ...bookmarkMessageItems(
        bookmark(2, [
          { index: 0, text: 'newer first message' },
          { index: 5, text: 'newer last message' }
        ])
      )
    ].sort(compareBookmarkItems)

    expect(items.map((item) => item.message_id)).toEqual(['m-2-5', 'm-2-0', 'm-1-0'])
  })
})

describe('bookmarkPreviewText', () => {
  it('collapses whitespace into a single line', () => {
    const [item] = bookmarkMessageItems(bookmark(1, [{ index: 0, text: '  a\n\n  b\t c ' }]))
    expect(bookmarkPreviewText(item)).toBe('a b c')
  })
})

describe('BookmarkBrowser.load', () => {
  it('loads folders and the first page of every bookmark', async () => {
    const store = createStore({
      listFolders: vi.fn(async () => folders({ 1: 'Reading' })),
      list: vi.fn(async () => page([bookmark(2), bookmark(1)], 'cursor-1'))
    })
    const browser = new BookmarkBrowser(store)

    await browser.load()

    expect(browser.items.map((item) => item.message_id)).toEqual(['m-2-0', 'm-1-0'])
    expect(browser.folderList.map((folder) => folder.name)).toEqual(['Reading'])
    expect(browser.folderName(1)).toBe('Reading')
    expect(browser.hasMore).toBe(true)
    expect(browser.loading).toBe(false)
    expect(browser.error).toBe('')
  })

  it('records the failure instead of throwing', async () => {
    const browser = new BookmarkBrowser(
      createStore({
        listFolders: vi.fn(async () => {
          throw new Error('daemon offline')
        })
      })
    )

    await expect(browser.load()).resolves.toBeUndefined()
    expect(browser.error).toContain('daemon offline')
    expect(browser.loading).toBe(false)
  })
})

describe('BookmarkBrowser.loadMore', () => {
  it('appends the next page and clears hasMore at the end', async () => {
    const list = vi
      .fn<BookmarkStore['list']>()
      .mockResolvedValueOnce(page([bookmark(2)], 'cursor-1'))
      .mockResolvedValueOnce(page([bookmark(1)], null))
    const browser = new BookmarkBrowser(createStore({ list }))

    await browser.load()
    await browser.loadMore()

    expect(list).toHaveBeenNthCalledWith(2, 'cursor-1')
    expect(browser.items.map((item) => item.message_id)).toEqual(['m-2-0', 'm-1-0'])
    expect(browser.hasMore).toBe(false)
  })

  it('does nothing once the cursor is exhausted', async () => {
    const list = vi.fn(async () => page([bookmark(1)], null))
    const browser = new BookmarkBrowser(createStore({ list }))

    await browser.load()
    await browser.loadMore()

    expect(list).toHaveBeenCalledTimes(1)
  })
})

describe('BookmarkBrowser.selectFolder', () => {
  it('queries the folder endpoint and maps unfiled to folder 0', async () => {
    const listInFolder = vi.fn(async () => page([bookmark(1)]))
    const browser = new BookmarkBrowser(createStore({ listInFolder }))

    await browser.selectFolder('unfiled')
    expect(listInFolder).toHaveBeenCalledWith(0, undefined)

    await browser.selectFolder(4)
    expect(listInFolder).toHaveBeenLastCalledWith(4, undefined)
  })

  it('ignores a reselect of the active folder', async () => {
    const list = vi.fn(async () => page([]))
    const browser = new BookmarkBrowser(createStore({ list }))

    await browser.selectFolder('all')

    expect(list).not.toHaveBeenCalled()
  })
})

describe('BookmarkBrowser folder assignment', () => {
  it('re-filters the conversation out of the list when it leaves the active folder', async () => {
    const browser = new BookmarkBrowser(
      createStore({
        listInFolder: vi.fn(async () => page([bookmark(1, undefined, [4])])),
        removeFromFolder: vi.fn(async () => bookmark(1, undefined, []))
      })
    )

    await browser.selectFolder(4)
    expect(browser.items).toHaveLength(1)

    await browser.removeFromFolder('m-1-0', 4)

    expect(browser.items).toHaveLength(0)
    expect(browser.isAssigning('m-1-0')).toBe(false)
  })

  it('keeps the conversation when the update still matches the filter', async () => {
    const browser = new BookmarkBrowser(
      createStore({
        list: vi.fn(async () => page([bookmark(1)])),
        addToFolder: vi.fn(async () => bookmark(1, undefined, [4]))
      })
    )

    await browser.load()
    await browser.addToFolder('m-1-0', 4)

    expect(browser.items.map((item) => item.folder_ids)).toEqual([[4]])
    expect(browser.error).toBe('')
  })

  it('reports an assignment failure without dropping the item', async () => {
    const browser = new BookmarkBrowser(
      createStore({
        list: vi.fn(async () => page([bookmark(1)])),
        addToFolder: vi.fn(async () => {
          throw new Error('folder is gone')
        })
      })
    )

    await browser.load()
    await browser.addToFolder('m-1-0', 4)

    expect(browser.items).toHaveLength(1)
    expect(browser.error).toContain('folder is gone')
  })
})

describe('BookmarkBrowser.remove', () => {
  it('drops the message once the daemon confirms', async () => {
    const browser = new BookmarkBrowser(
      createStore({ list: vi.fn(async () => page([bookmark(1)])) })
    )

    await browser.load()
    await browser.remove('m-1-0')

    expect(browser.items).toHaveLength(0)
    expect(browser.isRemoving('m-1-0')).toBe(false)
  })

  it('keeps the message when the daemon reports nothing was removed', async () => {
    const browser = new BookmarkBrowser(
      createStore({
        list: vi.fn(async () => page([bookmark(1)])),
        remove: vi.fn(async () => false)
      })
    )

    await browser.load()
    await browser.remove('m-1-0')

    expect(browser.items).toHaveLength(1)
  })
})

describe('BookmarkBrowser folder CRUD', () => {
  it('trims the new folder name and refuses an empty one', async () => {
    const createFolder = vi.fn(async () => folders({ 1: 'Reading' }))
    const browser = new BookmarkBrowser(createStore({ createFolder }))

    expect(await browser.createFolder('   ')).toBe(false)
    expect(createFolder).not.toHaveBeenCalled()

    expect(await browser.createFolder('  Reading  ')).toBe(true)
    expect(createFolder).toHaveBeenCalledWith('Reading')
    expect(browser.folderName(1)).toBe('Reading')
  })

  it('falls back to all and reloads when the active folder is deleted', async () => {
    const list = vi.fn(async () => page([bookmark(1)]))
    const browser = new BookmarkBrowser(
      createStore({ list, listInFolder: vi.fn(async () => page([])) })
    )

    await browser.selectFolder(4)
    await browser.deleteFolder(4)

    expect(browser.activeFolder).toBe('all')
    expect(list).toHaveBeenCalledTimes(1)
    expect(browser.isDeletingFolder(4)).toBe(false)
  })
})

describe('BookmarkBrowser.folderCount', () => {
  it('counts loaded items per folder', async () => {
    const browser = new BookmarkBrowser(
      createStore({
        list: vi.fn(async () => page([bookmark(2, undefined, [4]), bookmark(1, undefined, [])]))
      })
    )

    await browser.load()

    expect(browser.folderCount('all')).toBe(2)
    expect(browser.folderCount('unfiled')).toBe(1)
    expect(browser.folderCount(4)).toBe(1)
    expect(browser.folderCount(9)).toBe(0)
  })
})
