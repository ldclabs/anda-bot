import { SvelteSet } from 'svelte/reactivity'
import { apiResult, apiResultPage, type DaemonApi } from './daemon'
import type {
  Bookmark,
  BookmarkFolders,
  BookmarkedMessage,
  ChatMessage,
  Conversation
} from './types'
import { normalizeMessage } from './conversations'

/** What `BookmarksApi` needs from the surrounding side panel. */
export interface BookmarksContext {
  /** Channel source recorded on new bookmarks, e.g. `cli:/tmp/ws`. */
  activeSource(): string
  /** Request metadata routing a bookmark's conversation back to its workspace. */
  bookmarkRequestMeta(bookmark: BookmarkedMessage): Promise<Record<string, unknown>>
  /** Surfaces a failure the caller cannot handle, e.g. a rolled-back star. */
  reportError(error: unknown): void
}

export function emptyBookmarkFolders(): BookmarkFolders {
  return { version: 1, next_folder_id: 1, folders: {}, updated_at: 0 }
}

/**
 * Bookmarks, both as a daemon API and as the star state the chat transcript
 * renders.
 *
 * `bookmarkedIds` is the single source of truth for which messages show a
 * filled star. It is kept warm per conversation: `loadConversation` fetches a
 * conversation's marks once (deduplicating concurrent requests), and every
 * mutation refreshes the cached entry. `add` and `remove` update the star
 * optimistically and roll it back through `BookmarksContext.reportError` if the
 * daemon rejects, so the UI never waits on a round trip to feel responsive.
 *
 * Read verbs answer with empty results when no token is configured.
 */
export class BookmarksApi {
  #daemon: DaemonApi
  #context: BookmarksContext

  /** Message ids currently bookmarked; drives the star in the transcript. */
  readonly bookmarkedIds = new SvelteSet<string>()

  #generation = 0
  #cache = new Map<number, Bookmark | null>()
  #requests = new Map<number, Promise<Bookmark | null>>()

  constructor(daemon: DaemonApi, context: BookmarksContext) {
    this.#daemon = daemon
    this.#context = context
  }

  clear(): void {
    this.#generation++
    this.#cache.clear()
    this.#requests.clear()
    this.bookmarkedIds.clear()
  }

  isBookmarked(messageId: string): boolean {
    return this.bookmarkedIds.has(messageId)
  }

  /** Warms the star state for every visible conversation. */
  async loadConversations(
    conversations: number[],
    options: { force?: boolean } = {}
  ): Promise<void> {
    if (!this.#daemon.authorized) {
      return
    }
    const ids = Array.from(new Set(conversations.filter(isConversationId)))
    await Promise.all(ids.map((conversation) => this.loadConversation(conversation, options)))
  }

  /** Fetches one conversation's marks, reusing an in-flight or cached result. */
  async loadConversation(
    conversation: number,
    options: { force?: boolean } = {}
  ): Promise<Bookmark | null> {
    if (!this.#daemon.authorized || !isConversationId(conversation)) {
      return null
    }
    if (!options.force && this.#cache.has(conversation)) {
      return this.#cache.get(conversation) || null
    }

    const generation = this.#generation
    let request = this.#requests.get(conversation)
    if (!request) {
      request = apiResult<Bookmark | null>(this.#daemon, 'bookmarks_api', {
        type: 'GetConversationBookmark',
        conversation
      })
        .then((bookmark) => bookmark || null)
        .finally(() => {
          if (this.#requests.get(conversation) === request) this.#requests.delete(conversation)
        })
      this.#requests.set(conversation, request)
    }

    const bookmark = await request
    if (generation !== this.#generation) return null
    this.#updateCache(conversation, bookmark)
    return bookmark
  }

  async toggle(message: ChatMessage): Promise<void> {
    if (!this.#daemon.authorized || !message.id) {
      return
    }
    if (this.bookmarkedIds.has(message.id)) {
      await this.remove(message.id)
    } else {
      await this.add(message)
    }
  }

  /** Stars a message immediately, rolling back if the daemon rejects. */
  async add(message: ChatMessage): Promise<void> {
    const messageId = message.id
    if (!this.#daemon.authorized || !messageId || this.bookmarkedIds.has(messageId)) {
      return
    }
    const generation = this.#generation
    this.bookmarkedIds.add(messageId)
    try {
      const bookmark = await apiResult<Bookmark>(this.#daemon, 'bookmarks_api', {
        type: 'AddBookmark',
        message_id: messageId,
        conversation: message.conversation,
        source: this.#context.activeSource(),
        role: message.role,
        text: message.text,
        folder_ids: []
      })
      if (generation !== this.#generation) return
      this.#updateCache(bookmark.conversation, bookmark)
    } catch (error) {
      if (generation !== this.#generation) return
      this.bookmarkedIds.delete(messageId)
      this.#context.reportError(error)
    }
  }

  /** Unstars a message. Returns false when the daemon rejected the removal. */
  async remove(messageId: string): Promise<boolean> {
    if (!this.#daemon.authorized || !messageId) {
      return false
    }
    const generation = this.#generation
    const wasBookmarked = this.bookmarkedIds.delete(messageId)
    try {
      const result = await apiResult<{
        removed: boolean
        conversation?: number
        bookmark?: Bookmark | null
      }>(this.#daemon, 'bookmarks_api', { type: 'RemoveBookmark', message_id: messageId })
      if (generation !== this.#generation) return false
      const conversation = result.conversation || conversationFromMessageId(messageId)
      if (conversation > 0) {
        this.#updateCache(conversation, result.bookmark || null)
      }
      return true
    } catch (error) {
      if (generation !== this.#generation) return false
      if (wasBookmarked) {
        this.bookmarkedIds.add(messageId)
      }
      this.#context.reportError(error)
      return false
    }
  }

  /** One newest-first page of every bookmark. */
  async list(
    cursor?: string,
    limit?: number
  ): Promise<{ items: Bookmark[]; nextCursor: string | null }> {
    if (!this.#daemon.authorized) {
      return { items: [], nextCursor: null }
    }
    return apiResultPage<Bookmark>(this.#daemon, 'bookmarks_api', {
      type: 'ListBookmarks',
      ...pageArgs(cursor, limit)
    })
  }

  /** One newest-first page of a folder; folder 0 means unfiled. */
  async listInFolder(
    folderId: number,
    cursor?: string,
    limit?: number
  ): Promise<{ items: Bookmark[]; nextCursor: string | null }> {
    if (!this.#daemon.authorized) {
      return { items: [], nextCursor: null }
    }
    return apiResultPage<Bookmark>(this.#daemon, 'bookmarks_api', {
      type: 'ListBookmarksInFolder',
      folder_id: folderId,
      ...pageArgs(cursor, limit)
    })
  }

  async listFolders(): Promise<BookmarkFolders> {
    if (!this.#daemon.authorized) {
      return emptyBookmarkFolders()
    }
    const folders = await apiResult<BookmarkFolders | null>(this.#daemon, 'bookmarks_api', {
      type: 'ListBookmarkFolders'
    })
    return folders || emptyBookmarkFolders()
  }

  async createFolder(name: string, parentId: number | null = null): Promise<BookmarkFolders> {
    return this.#folders({ type: 'CreateBookmarkFolder', name, parent_id: parentId })
  }

  async renameFolder(folderId: number, name: string): Promise<BookmarkFolders> {
    return this.#folders({ type: 'RenameBookmarkFolder', folder_id: folderId, name })
  }

  async deleteFolder(folderId: number): Promise<BookmarkFolders> {
    return this.#folders({ type: 'DeleteBookmarkFolder', folder_id: folderId })
  }

  async moveFolder(
    folderId: number,
    parentId: number | null = null,
    order: number | null = null
  ): Promise<BookmarkFolders> {
    return this.#folders({
      type: 'MoveBookmarkFolder',
      folder_id: folderId,
      parent_id: parentId,
      order
    })
  }

  async setFolders(messageId: string, folderIds: number[]): Promise<Bookmark> {
    return this.#assign({
      type: 'SetBookmarkFolders',
      message_id: messageId,
      folder_ids: folderIds
    })
  }

  async addToFolder(messageId: string, folderId: number): Promise<Bookmark> {
    return this.#assign({
      type: 'AddBookmarkToFolder',
      message_id: messageId,
      folder_id: folderId
    })
  }

  async removeFromFolder(messageId: string, folderId: number): Promise<Bookmark> {
    return this.#assign({
      type: 'RemoveBookmarkFromFolder',
      message_id: messageId,
      folder_id: folderId
    })
  }

  /**
   * Re-fetches the bookmarked message's full markdown from its conversation.
   * Returns '' when the conversation no longer holds that message.
   */
  async conversationMarkdown(bookmark: BookmarkedMessage): Promise<string> {
    if (
      !this.#daemon.authorized ||
      !isConversationId(bookmark.conversation) ||
      !Number.isInteger(bookmark.message_index) ||
      bookmark.message_index < 0
    ) {
      return ''
    }

    const meta = await this.#context.bookmarkRequestMeta(bookmark)
    const {
      output: { result }
    } = await this.#daemon.toolCall<{ result: Conversation }>(
      'conversations_api',
      { type: 'GetConversation', _id: bookmark.conversation },
      [],
      meta
    )
    const rawMessage = result.messages?.[bookmark.message_index]
    if (!rawMessage) {
      return ''
    }

    return (
      normalizeMessage(rawMessage, {
        conversation: result._id,
        index: bookmark.message_index,
        fallbackTimestamp: result.updated_at
      })?.text || ''
    )
  }

  async #folders(args: Record<string, unknown>): Promise<BookmarkFolders> {
    const folders = await apiResult<BookmarkFolders | null>(this.#daemon, 'bookmarks_api', args)
    return folders || emptyBookmarkFolders()
  }

  async #assign(args: Record<string, unknown>): Promise<Bookmark> {
    const generation = this.#generation
    const bookmark = await apiResult<Bookmark>(this.#daemon, 'bookmarks_api', args)
    if (generation !== this.#generation) throw new Error('Connection settings changed')
    this.#updateCache(bookmark.conversation, bookmark)
    return bookmark
  }

  /**
   * Replaces every star belonging to `conversation` with the marks in
   * `bookmark`, so a removal server-side clears the star client-side too.
   */
  #updateCache(conversation: number, bookmark: Bookmark | null): void {
    for (const id of Array.from(this.bookmarkedIds)) {
      if (conversationFromMessageId(id) === conversation) {
        this.bookmarkedIds.delete(id)
      }
    }
    this.#cache.set(conversation, bookmark)
    if (!bookmark) {
      return
    }
    for (const message of bookmark.messages || []) {
      if (Number.isInteger(message.index) && message.index >= 0) {
        this.bookmarkedIds.add(`m-${bookmark.conversation}-${message.index}`)
      }
    }
  }
}

function pageArgs(cursor?: string, limit?: number): Record<string, unknown> {
  const args: Record<string, unknown> = {}
  if (cursor) {
    args.cursor = cursor
  }
  if (limit) {
    args.limit = limit
  }
  return args
}

function isConversationId(value: number): boolean {
  return Number.isFinite(value) && value > 0
}

/** Message ids are `m-<conversation>-<index>`; anything else yields 0. */
function conversationFromMessageId(messageId: string): number {
  const match = /^m-(\d+)-\d+$/.exec(messageId)
  return match ? Number(match[1]) : 0
}
