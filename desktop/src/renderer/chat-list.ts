import { workspaceFromCliSource } from '$lib/anda/client/workspace'
import type { ChatEntry, SidebarGroup } from '../shared/contract'

/** What a sidebar row's leading icon says about its chat, most urgent first. */
export type ChatState = 'approval' | 'running' | 'failed' | 'unread' | 'idle'

/**
 * A chat reads as unread once activity lands after it was last seen. Chats
 * from before the app recorded `readAt` (and ones it only discovered) count
 * as read until their next update.
 */
export function isUnread(chat: ChatEntry): boolean {
  return chat.readAt !== undefined && chat.updatedAt > chat.readAt
}

export function chatWorkspace(chat: ChatEntry): string {
  return chat.workspace || workspaceFromCliSource(chat.source)
}

export function folderName(path: string): string {
  return path.split(/[\\/]/).filter(Boolean).at(-1) || path
}

export type DateBucket = 'today' | 'yesterday' | 'week' | 'month' | 'older'
export interface ChatSection {
  key: string
  /** A date bucket, a project's folder path, `''` for chats without a project, or `null` when ungrouped. */
  bucket: DateBucket | null
  workspace: string | null
  chats: ChatEntry[]
}

function dateBucket(time: number, now: Date): DateBucket {
  const startOfToday = new Date(now.getFullYear(), now.getMonth(), now.getDate()).getTime()
  const day = 24 * 60 * 60 * 1000
  if (time >= startOfToday) return 'today'
  if (time >= startOfToday - day) return 'yesterday'
  if (time >= startOfToday - 6 * day) return 'week'
  if (time >= startOfToday - 29 * day) return 'month'
  return 'older'
}

/**
 * Splits an already ordered chat list into the sidebar's sections. Date
 * sections keep their natural order; project sections follow each project's
 * most recent chat, with chats outside any project last.
 */
export function groupChats(
  chats: ChatEntry[],
  mode: SidebarGroup,
  now = new Date()
): ChatSection[] {
  if (mode === 'none' || !chats.length)
    return [{ key: 'all', bucket: null, workspace: null, chats }]
  const sections = new Map<string, ChatSection>()
  for (const chat of chats) {
    const key = mode === 'date' ? dateBucket(chat.updatedAt, now) : chatWorkspace(chat)
    let section = sections.get(key)
    if (!section) {
      section =
        mode === 'date'
          ? { key, bucket: key as DateBucket, workspace: null, chats: [] }
          : { key: `project:${key}`, bucket: null, workspace: key, chats: [] }
      sections.set(key, section)
    }
    section.chats.push(chat)
  }
  const list = [...sections.values()]
  if (mode === 'project') {
    // Chats outside any project come last.
    list.sort((a, b) => Number(a.workspace === '') - Number(b.workspace === ''))
  }
  return list
}
