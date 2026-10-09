import { describe, expect, it } from 'vitest'
import { groupChats, isUnread } from '../src/renderer/chat-list'
import {
  conversationMarkdown,
  editedFiles,
  fileTarget,
  workspaceRelative
} from '../src/renderer/transcript'
import { diffLineKind, formatElapsed, formatTokens, modelLabel } from '../src/renderer/presentation'
import { accelerator, shortcutLabel } from '../src/shared/shortcuts'
import type { ChatEntry } from '../src/shared/contract'
import type { ChatMessage } from '$lib/anda/client/types'

const chat = (source: string, updatedAt: number, extra: Partial<ChatEntry> = {}): ChatEntry => ({
  source,
  title: source,
  updatedAt,
  ...extra
})

describe('chat list', () => {
  it('treats chats without a read time as read', () => {
    expect(isUnread(chat('a', 10))).toBe(false)
    expect(isUnread(chat('a', 10, { readAt: 5 }))).toBe(true)
    expect(isUnread(chat('a', 10, { readAt: 10 }))).toBe(false)
  })

  it('groups by date bucket and by project with unassigned chats last', () => {
    const now = new Date(2026, 9, 9, 15)
    const today = new Date(2026, 9, 9, 9).getTime()
    const yesterday = new Date(2026, 9, 8, 9).getTime()
    const old = new Date(2026, 5, 1).getTime()
    const chats = [
      chat('desktop:1', today),
      chat('desktop:2', yesterday, { workspace: '/repo/app' }),
      chat('cli:/repo/app', old),
      chat('desktop:3', old)
    ]
    expect(groupChats(chats, 'date', now).map((s) => [s.bucket, s.chats.length])).toEqual([
      ['today', 1],
      ['yesterday', 1],
      ['older', 2]
    ])
    expect(groupChats(chats, 'project', now).map((s) => [s.workspace, s.chats.length])).toEqual([
      ['/repo/app', 2],
      ['', 2]
    ])
    expect(groupChats(chats, 'none', now)).toHaveLength(1)
  })
})

describe('transcript helpers', () => {
  const tools = (calls: Array<[string, unknown, unknown?]>): ChatMessage => ({
    id: 'm',
    role: 'assistant',
    text: '',
    tools: calls.map(([name, args, output]) => ({
      name,
      args: JSON.stringify(args),
      output: output === undefined ? '{"ok":true}' : JSON.stringify(output)
    }))
  })

  it('collects edited files with the lines each edit changed', () => {
    const files = editedFiles([
      tools([
        ['edit_file', { path: 'src/a.ts', old_string: 'a\nb\nc', new_string: 'a\nB\nB2\nc' }],
        ['write_file', { path: 'notes.md', content: 'one\ntwo\n' }],
        ['edit_file', { path: 'src/a.ts', old_string: 'x', new_string: '' }],
        [
          'edit_file',
          { path: 'broken.ts', old_string: 'a', new_string: 'b' },
          { error: 'no match' }
        ],
        ['read_file', { path: 'src/b.ts' }]
      ])
    ])
    expect(files).toEqual([
      { path: 'src/a.ts', additions: 2, deletions: 2 },
      { path: 'notes.md', additions: 2, deletions: 0 }
    ])
  })

  it('recognizes file paths but not prose, calls or web links', () => {
    expect(fileTarget('src/app.ts')).toEqual({ path: 'src/app.ts' })
    expect(fileTarget('./a/b.rs:42')).toEqual({ path: './a/b.rs', line: 42 })
    expect(fileTarget('anda_bot/src/main.rs#L10-L20')).toEqual({
      path: 'anda_bot/src/main.rs',
      line: 10
    })
    expect(fileTarget('README.md')).toEqual({ path: 'README.md' })
    expect(fileTarget('C:\\repo\\x.ts')).toEqual({ path: 'C:\\repo\\x.ts' })
    expect(fileTarget('file:///tmp/a%20b/c.txt')).toBeNull()
    expect(fileTarget('file:///tmp/c.txt')).toEqual({ path: '/tmp/c.txt' })
    for (const text of [
      'https://example.com/a.ts',
      'obj.method',
      'foo()',
      'std::fs',
      'a b.ts',
      'v1.2'
    ])
      expect(fileTarget(text)).toBeNull()
  })

  it('shortens paths inside the workspace', () => {
    expect(workspaceRelative('/repo/app/src/a.ts', '/repo/app')).toBe('src/a.ts')
    expect(workspaceRelative('/elsewhere/a.ts', '/repo/app')).toBe('/elsewhere/a.ts')
    expect(workspaceRelative('C:\\repo\\a.ts', 'C:\\repo')).toBe('a.ts')
  })

  it('exports a conversation as Markdown', () => {
    const text = conversationMarkdown(
      'Title',
      [
        { id: '1', role: 'user', text: 'Hi' },
        { id: '2', role: 'tool', text: 'ignored' },
        { id: '3', role: 'assistant', text: 'Hello' }
      ],
      { user: 'You', assistant: 'Anda' }
    )
    expect(text).toBe('# Title\n\n---\n\n**You:**\n\nHi\n\n---\n\n**Anda:**\n\nHello\n')
  })
})

describe('presentation', () => {
  it('names models, colours diffs and formats counts', () => {
    expect(modelLabel('chatgpt:acct:gpt-5.1')).toBe('gpt-5.1')
    expect(modelLabel('claude-opus')).toBe('claude-opus')
    expect(['+++ b/a', '@@ -1 +1 @@', '+x', '-y', ' z'].map(diffLineKind)).toEqual([
      'meta',
      'hunk',
      'add',
      'del',
      'context'
    ])
    expect([950, 12_345, 4_100_000].map(formatTokens)).toEqual(['950', '12k', '4.1M'])
    expect(formatTokens(1234)).toBe('1.2k')
    expect([7_000, 750_000, 3_725_000].map(formatElapsed)).toEqual(['0:07', '12:30', '1:02:05'])
  })

  it('maps shortcuts per platform', () => {
    expect(accelerator('back', 'darwin')).toBe('Cmd+[')
    expect(accelerator('back', 'win32')).toBe('Alt+Left')
    expect(shortcutLabel('panel:terminal', 'darwin')).toBe('⌘⇧T')
    expect(shortcutLabel('panel:terminal', 'linux')).toBe('Ctrl+Shift+T')
  })
})
