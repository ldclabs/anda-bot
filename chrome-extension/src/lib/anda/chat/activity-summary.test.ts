import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import en from '../../../../public/_locales/en/messages.json'
import zh from '../../../../public/_locales/zh_CN/messages.json'
import type { ChatToolCall } from '../client/types'
import { activityCounts, activityLabel } from './activity-summary'
import type { ActivityDetail } from './message-display'

type Messages = Record<string, { message: string }>

function useLocale(language: string, messages: Messages) {
  vi.stubGlobal('chrome', {
    i18n: {
      getUILanguage: () => language,
      getMessage: (key: string, substitutions?: string | string[]) =>
        (messages[key]?.message || '').replace(/\$1/g, String([substitutions].flat()[0] ?? ''))
    }
  })
}

function tools(...calls: ChatToolCall[]): ActivityDetail[] {
  return calls.map((tool, index) => ({ kind: 'tool', id: `t${index}`, messageId: 'm', tool }))
}

beforeEach(() => useLocale('en', en))
afterEach(() => vi.unstubAllGlobals())

describe('activityCounts', () => {
  it('sorts calls by what they did and counts each file once', () => {
    const counts = activityCounts([
      { kind: 'thought', id: 'th', messageId: 'm', text: 'Look around' },
      ...tools(
        { name: 'shell', args: { command: 'git status' } },
        { name: 'shell', args: { command: 'cargo test' } },
        { name: 'read_file', args: { path: 'src/main.rs' } },
        { name: 'read_file', args: { path: 'src/main.rs' } },
        { name: 'search_file', args: { pattern: 'TODO' } },
        { name: 'edit_file', args: { path: 'src/main.rs' } },
        { name: 'recall_memory', args: { query: 'release' } },
        { name: 'fetch_web_resources', args: { url: 'https://example.com' } },
        { name: 'subagent', args: { task: 'Review' } },
        { name: 'update_plan', args: { advance: true } }
      )
    ])
    expect(counts).toEqual({
      commands: 2,
      filesRead: 2,
      filesEdited: 1,
      memory: 1,
      web: 1,
      tools: 1,
      plan: true
    })
  })
})

describe('activityLabel', () => {
  it('reads as one sentence with singular and plural phrases', () => {
    const label = activityLabel(
      activityCounts(
        tools(
          { name: 'shell', args: { command: 'ls' } },
          { name: 'shell', args: { command: 'pwd' } },
          { name: 'read_file', args: { path: 'a.md' } },
          { name: 'update_plan' }
        )
      )
    )
    expect(label).toBe('Ran 2 commands, read 1 file, and updated the plan')
  })
  it('follows the UI language', () => {
    useLocale('zh-CN', zh)
    const label = activityLabel(activityCounts(tools({ name: 'shell' }, { name: 'recall_memory' })))
    expect(label).toBe('运行了 1 个命令和调取了 1 次记忆')
  })
  it('is empty when the run called no tools', () => {
    expect(activityLabel(activityCounts([]))).toBe('')
  })
})
