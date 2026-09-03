import type { Bookmark, BookmarkFolder, BookmarkFolders, BookmarkedMessage } from '../client/types'
import { emptyBookmarkFolders } from '../client/bookmarks.svelte'
import { errorToMessage } from '$lib/service-worker/settings'

export { emptyBookmarkFolders }

/** 'all' and 'unfiled' are pseudo-folders; a number is a real folder id. */
export type ActiveFolder = 'all' | 'unfiled' | number

export interface BookmarkPage {
  items: Bookmark[]
  nextCursor: string | null
}

/**
 * Everything `BookmarkBrowser` needs from the daemon. `andaClient` satisfies it
 * in production; tests pass a plain object, which is why the browser never
 * touches `chrome.*` itself.
 */
export interface BookmarkStore {
  list(cursor?: string, limit?: number): Promise<BookmarkPage>
  listInFolder(folderId: number, cursor?: string, limit?: number): Promise<BookmarkPage>
  listFolders(): Promise<BookmarkFolders>
  createFolder(name: string, parentId?: number | null): Promise<BookmarkFolders>
  deleteFolder(folderId: number): Promise<BookmarkFolders>
  addToFolder(messageId: string, folderId: number): Promise<Bookmark>
  removeFromFolder(messageId: string, folderId: number): Promise<Bookmark>
  remove(messageId: string): Promise<boolean>
}

/**
 * The bookmark list a view renders: cursor paging, folder filtering, folder
 * CRUD, and the per-item in-flight flags that keep buttons from double-firing.
 *
 * Views own presentation only — they read the reactive fields, call the verbs,
 * and never re-derive paging or filtering themselves. Every verb resolves after
 * the state has settled and records failures in `error` instead of throwing, so
 * a view can `void browser.createFolder(name)` without a try/catch.
 */
export class BookmarkBrowser {
  #store: BookmarkStore

  items = $state<BookmarkedMessage[]>([])
  folders = $state<BookmarkFolders>(emptyBookmarkFolders())
  activeFolder = $state<ActiveFolder>('all')
  error = $state('')
  loading = $state(false)
  loadingMore = $state(false)
  creatingFolder = $state(false)

  #cursor = $state<string | null>(null)
  #removingIds = $state(new Set<string>())
  #assigningIds = $state(new Set<string>())
  #deletingFolderIds = $state(new Set<number>())

  constructor(store: BookmarkStore) {
    this.#store = store
  }

  /** True while another page is available for `loadMore()`. */
  get hasMore(): boolean {
    return Boolean(this.#cursor)
  }

  /** Folders in display order. */
  get folderList(): BookmarkFolder[] {
    return Object.values(this.folders.folders).sort(
      (left, right) => left.order - right.order || left._id - right._id
    )
  }

  folderName(folderId: number): string {
    return this.folders.folders[String(folderId)]?.name || ''
  }

  /** How many loaded items sit in `folder`; drives the folder-list counters. */
  folderCount(folder: ActiveFolder): number {
    if (folder === 'all') {
      return this.items.length
    }
    return this.items.filter((item) => this.#matchesFolder(item, folder)).length
  }

  isRemoving(messageId: string): boolean {
    return this.#removingIds.has(messageId)
  }

  isAssigning(messageId: string): boolean {
    return this.#assigningIds.has(messageId)
  }

  isDeletingFolder(folderId: number): boolean {
    return this.#deletingFolderIds.has(folderId)
  }

  /** Reloads folders plus the first page of the active folder. */
  async load(): Promise<void> {
    this.loading = true
    this.error = ''
    try {
      this.folders = await this.#store.listFolders()
      const { items, nextCursor } = await this.#listActive()
      this.items = items.flatMap(bookmarkMessageItems)
      this.#cursor = nextCursor
    } catch (error) {
      this.error = errorToMessage(error)
    } finally {
      this.loading = false
    }
  }

  /** Appends the next page; a no-op when exhausted or already loading. */
  async loadMore(): Promise<void> {
    if (!this.#cursor || this.loadingMore) {
      return
    }
    this.loadingMore = true
    try {
      const { items, nextCursor } = await this.#listActive(this.#cursor)
      this.items = [...this.items, ...items.flatMap(bookmarkMessageItems)]
      this.#cursor = nextCursor
    } catch (error) {
      this.error = errorToMessage(error)
    } finally {
      this.loadingMore = false
    }
  }

  /** Switches the filter and reloads; a no-op when already on `folder`. */
  async selectFolder(folder: ActiveFolder): Promise<void> {
    if (this.activeFolder === folder) {
      return
    }
    this.activeFolder = folder
    await this.load()
  }

  /** Creates a folder. Returns true when one was created. */
  async createFolder(name: string): Promise<boolean> {
    const trimmed = name.trim()
    if (!trimmed || this.creatingFolder) {
      return false
    }
    this.creatingFolder = true
    this.error = ''
    try {
      this.folders = await this.#store.createFolder(trimmed)
      return true
    } catch (error) {
      this.error = errorToMessage(error)
      return false
    } finally {
      this.creatingFolder = false
    }
  }

  /** Deletes a folder, falling back to 'all' when it was the active one. */
  async deleteFolder(folderId: number): Promise<void> {
    if (this.#deletingFolderIds.has(folderId)) {
      return
    }
    this.#deletingFolderIds = new Set([...this.#deletingFolderIds, folderId])
    this.error = ''
    try {
      this.folders = await this.#store.deleteFolder(folderId)
      if (this.activeFolder === folderId) {
        this.activeFolder = 'all'
      }
      await this.load()
    } catch (error) {
      this.error = errorToMessage(error)
    } finally {
      this.#deletingFolderIds = withoutValue(this.#deletingFolderIds, folderId)
    }
  }

  async addToFolder(messageId: string, folderId: number): Promise<void> {
    await this.#assign(messageId, () => this.#store.addToFolder(messageId, folderId))
  }

  async removeFromFolder(messageId: string, folderId: number): Promise<void> {
    await this.#assign(messageId, () => this.#store.removeFromFolder(messageId, folderId))
  }

  /** Deletes a bookmark and drops it from the list once the daemon confirms. */
  async remove(messageId: string): Promise<void> {
    if (this.#removingIds.has(messageId)) {
      return
    }
    this.#removingIds = new Set([...this.#removingIds, messageId])
    try {
      if (await this.#store.remove(messageId)) {
        this.items = this.items.filter((item) => item.message_id !== messageId)
      }
    } finally {
      this.#removingIds = withoutValue(this.#removingIds, messageId)
    }
  }

  async #assign(messageId: string, update: () => Promise<Bookmark>): Promise<void> {
    if (this.#assigningIds.has(messageId)) {
      return
    }
    this.#assigningIds = new Set([...this.#assigningIds, messageId])
    this.error = ''
    try {
      this.#replaceBookmark(await update())
    } catch (error) {
      this.error = errorToMessage(error)
    } finally {
      this.#assigningIds = withoutValue(this.#assigningIds, messageId)
    }
  }

  #listActive(cursor?: string): Promise<BookmarkPage> {
    if (this.activeFolder === 'all') {
      return this.#store.list(cursor)
    }
    return this.#store.listInFolder(this.activeFolder === 'unfiled' ? 0 : this.activeFolder, cursor)
  }

  /**
   * Swaps in a conversation's bookmark after a folder change, dropping the
   * messages that no longer match the active filter.
   */
  #replaceBookmark(updated: Bookmark | null): void {
    if (!updated) {
      return
    }
    const kept = bookmarkMessageItems(updated).filter((item) =>
      this.#matchesFolder(item, this.activeFolder)
    )
    this.items = [
      ...this.items.filter((item) => item.conversation !== updated.conversation),
      ...kept
    ].sort(compareBookmarkItems)
  }

  #matchesFolder(item: BookmarkedMessage, folder: ActiveFolder): boolean {
    const ids = bookmarkFolderIds(item)
    if (folder === 'all') {
      return true
    }
    if (folder === 'unfiled') {
      return ids.length === 0
    }
    return ids.includes(folder)
  }
}

export function bookmarkFolderIds(bookmark: Bookmark | BookmarkedMessage): number[] {
  return Array.isArray(bookmark.folder_ids) ? bookmark.folder_ids : []
}

/** Flattens a bookmark into its marked messages, newest message first. */
export function bookmarkMessageItems(bookmark: Bookmark): BookmarkedMessage[] {
  return (bookmark.messages || [])
    .map((message) => {
      const messageIndex = Number(message.index)
      if (!Number.isInteger(messageIndex) || messageIndex < 0) {
        return null
      }
      return {
        bookmark,
        message_id: `m-${bookmark.conversation}-${messageIndex}`,
        message_index: messageIndex,
        conversation: bookmark.conversation,
        source: bookmark.source,
        role: message.role,
        folder_ids: bookmarkFolderIds(bookmark),
        text: message.text,
        created_at: bookmark.created_at
      } satisfies BookmarkedMessage
    })
    .filter((item): item is BookmarkedMessage => Boolean(item))
    .sort((left, right) => right.message_index - left.message_index)
}

export function compareBookmarkItems(left: BookmarkedMessage, right: BookmarkedMessage): number {
  return (
    right.bookmark._id - left.bookmark._id ||
    right.message_index - left.message_index ||
    left.message_id.localeCompare(right.message_id)
  )
}

/** Collapses whitespace so a bookmark reads as one line in a list. */
export function bookmarkPreviewText(bookmark: BookmarkedMessage): string {
  return bookmark.text.trim().replace(/\s+/g, ' ')
}

function withoutValue<T>(values: Set<T>, value: T): Set<T> {
  const next = new Set(values)
  next.delete(value)
  return next
}
