import { describe, expect, it } from 'vitest'
import type { ChatMessage } from '../client/types'
import { displayMessages, isProcessStep } from './message-display'

function message(id: string, fields: Partial<ChatMessage>): ChatMessage {
  return { id, conversation: 1, role: 'assistant', text: '', ...fields }
}

describe('displayMessages', () => {
  it('pairs tool results with their calls and drops result-only messages', () => {
    const source = [
      message('m-1-0', { role: 'user', text: 'look at the repo' }),
      message('m-1-1', {
        text: 'Checking the tree.',
        tools: [{ callId: 'a', name: 'shell', args: { command: 'ls' } }]
      }),
      message('m-1-2', { role: 'tool', tools: [{ callId: 'a', name: 'shell', output: 'README' }] }),
      message('m-1-3', { text: 'It has a README.' })
    ]

    const shown = displayMessages(source)

    expect(shown.map((item) => item.id)).toEqual(['m-1-0', 'm-1-1', 'm-1-3'])
    expect(shown[1]?.tools).toEqual([
      { callId: 'a', name: 'shell', args: { command: 'ls' }, output: 'README' }
    ])
    // Source messages stay untouched; the fold is a copy.
    expect(source[1]?.tools?.[0]?.output).toBeUndefined()
    expect(isProcessStep(shown[1]!)).toBe(true)
    expect(isProcessStep(shown[2]!)).toBe(false)
  })

  it('folds steps without prose into the step before them', () => {
    const shown = displayMessages([
      message('m-1-1', {
        text: 'Reading the docs.',
        tools: [{ callId: 'a', name: 'read_file', args: { path: 'a.md' } }]
      }),
      message('m-1-2', { role: 'tool', tools: [{ callId: 'a', name: 'read_file', output: 'A' }] }),
      message('m-1-3', {
        thinkingText: 'Need the second file too.',
        tools: [{ callId: 'b', name: 'read_file', args: { path: 'b.md' } }]
      }),
      message('m-1-4', { role: 'tool', tools: [{ callId: 'b', name: 'read_file', output: 'B' }] }),
      message('m-1-5', { text: 'Both files read.' })
    ])

    expect(shown.map((item) => item.id)).toEqual(['m-1-1', 'm-1-5'])
    expect(shown[0]?.text).toBe('Reading the docs.')
    expect(shown[0]?.thinkingText).toBe('Need the second file too.')
    expect(shown[0]?.tools?.map((tool) => tool.output)).toEqual(['A', 'B'])
  })

  it('pairs by tool name when a provider omits call ids', () => {
    const shown = displayMessages([
      message('m-1-1', {
        tools: [
          { name: 'shell', args: { command: 'pwd' } },
          { name: 'recall_memory', args: { query: 'x' } }
        ]
      }),
      message('m-1-2', {
        role: 'tool',
        tools: [
          { name: 'recall_memory', output: { error: 'down' } },
          { name: 'shell', output: '/tmp' }
        ]
      })
    ])

    expect(shown).toHaveLength(1)
    expect(shown[0]?.tools?.map((tool) => tool.output)).toEqual(['/tmp', { error: 'down' }])
  })

  it('keeps unmatched results and runtime notices visible', () => {
    const shown = displayMessages([
      message('m-1-0', { role: 'user', text: 'go on' }),
      message('m-1-1', { role: 'tool', tools: [{ callId: 'z', name: 'shell', output: 'late' }] }),
      message('m-1-2-tool', { role: 'tool', thinkingText: '[$system: kind="notice"]\n\n"hi"' }),
      message('m-1-3', { thinkingText: 'Only thinking.' })
    ])

    expect(shown.map((item) => item.id)).toEqual(['m-1-0', 'm-1-1', 'm-1-2-tool', 'm-1-3'])
    expect(shown[1]?.tools?.[0]?.output).toBe('late')
  })

  it('returns the same objects when nothing folds', () => {
    const source = [
      message('m-1-0', { role: 'user', text: 'hi' }),
      message('m-1-1', { text: 'hello' })
    ]

    const shown = displayMessages(source)

    expect(shown[0]).toBe(source[0])
    expect(shown[1]).toBe(source[1])
  })

  it('folds an unchanged message array once and refolds a replaced one', () => {
    const source = [
      message('m-1-0', { text: 'Run it.', tools: [{ callId: 'a', name: 'shell' }] }),
      message('m-1-1', { role: 'tool', tools: [{ callId: 'a', name: 'shell', output: 'ok' }] })
    ]

    const first = displayMessages(source)
    expect(displayMessages(source)).toBe(first)
    expect(displayMessages(source)[0]).toBe(first[0])

    const replaced = [...source, message('m-1-2', { text: 'Done.' })]
    expect(displayMessages(replaced)).not.toBe(first)
    expect(displayMessages(replaced).map((item) => item.id)).toEqual(['m-1-0', 'm-1-2'])
  })
})
