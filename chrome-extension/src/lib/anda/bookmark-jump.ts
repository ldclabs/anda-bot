import type { BookmarkedMessage } from '$lib/anda/client/types'

export const bookmarkJumpRequestStorageKey = 'andaBookmarkJumpRequest'
export const bookmarkJumpRequestMaxAgeMs = 5 * 60 * 1000

export interface BookmarkJumpRequest {
  id: string
  createdAt: number
  bookmark: BookmarkedMessage
}

export function createBookmarkJumpRequest(bookmark: BookmarkedMessage): BookmarkJumpRequest {
  return {
    id: `${bookmark.message_id}:${globalThis.crypto?.randomUUID?.() || `${Date.now()}-${Math.random().toString(36).slice(2)}`}`,
    createdAt: Date.now(),
    bookmark
  }
}

export function isBookmarkJumpRequest(value: unknown): value is BookmarkJumpRequest {
  if (!value || typeof value !== 'object') {
    return false
  }

  const request = value as Partial<BookmarkJumpRequest>
  const bookmark = request.bookmark as Partial<BookmarkedMessage> | undefined

  return (
    typeof request.id === 'string' &&
    typeof request.createdAt === 'number' &&
    Boolean(bookmark) &&
    typeof bookmark?.message_id === 'string' &&
    typeof bookmark.source === 'string' &&
    typeof bookmark.conversation === 'number'
  )
}
