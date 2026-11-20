import { describe, expect, it } from 'vitest'
import type { ChatMessage } from '../client/types'
import { transcriptAnchor, chatTranscript, isProcessStep } from './message-display'

function message(id: string, fields: Partial<ChatMessage>): ChatMessage {
  return { id, conversation: 1, role: 'assistant', text: '', ...fields }
}

describe('chatTranscript', () => {
  it('puts consecutive reasoning, paired calls and background updates in one disclosure before the answer', () => {
    const source = [
      message('prompt', { role: 'user', text: 'Review the docs' }),
      message('step', {
        thinkingText: 'Check examples',
        tools: [{ name: 'read_file', callId: 'a' }]
      }),
      message('output', {
        role: 'tool',
        tools: [{ name: 'read_file', callId: 'a', output: 'README' }]
      }),
      message('notice', {
        role: 'tool',
        thinkingText: '[$system: kind="background shell"]\nRuntime.\n\n"Done"'
      }),
      message('answer', { thinkingText: 'Double check', text: 'The examples match.' })
    ]
    const { messages, rows } = chatTranscript(source)
    expect(rows.map((row) => row.kind)).toEqual(['message', 'activity', 'message'])
    expect(rows[1]).toMatchObject({
      id: 'step:thinking',
      details: [
        { kind: 'thought', text: 'Check examples' },
        { kind: 'tool', tool: { name: 'read_file', output: 'README' } },
        { kind: 'notice', name: 'background shell', text: 'Done' },
        { kind: 'thought', text: 'Double check' }
      ]
    })
    expect(rows[2]).toMatchObject({ id: 'answer', lastMessageId: 'answer' })
    expect(transcriptAnchor([chatTranscript(source)], 'notice')).toBe(rows[1]?.id)
    expect(transcriptAnchor([chatTranscript(source)], 'answer')).toBe('answer')
    expect(messages[1]).not.toBe(source[1])
    expect(source[1]?.tools?.[0]?.output).toBeUndefined()
    expect(chatTranscript(source).rows).toBe(rows)
    expect(chatTranscript(source.slice(0, 2)).rows[1]?.id).toBe(rows[1]?.id)
  })

  it('preserves public narration, important notices, attachments, approvals and conversation failures', () => {
    const { rows } = chatTranscript([
      message('step', {
        thinkingText: 'Check',
        text: 'I will read the report.',
        tools: [{ name: 'read_file' }]
      }),
      message('recover', {
        role: 'tool',
        thinkingText: '[$system: kind="Code Mode recovery"]\nRuntime.\n\n"Check the outcome"'
      }),
      message('file', {
        role: 'tool',
        attachments: [{ id: 'r', name: 'report', resource: { name: 'report', tags: [] } }]
      }),
      message('act', {
        actions: [{ id: 'approve', name: 'anda.tool_approval', status: 'pending', payload: {} }]
      }),
      message('fail', { role: 'system', text: 'Connection lost' })
    ])
    expect(rows.map((row) => row.id)).toEqual([
      'step:thinking',
      'step',
      'step:tool:0',
      'recover:notice:0',
      'file',
      'act',
      'fail'
    ])
    expect(rows[2]?.lastMessageId).toBe('step')
  })

  it('pairs by name when ids are missing, within the same user turn and conversation', () => {
    const call = { name: 'read_file' }
    const source = [
      message('old', { tools: [call] }),
      message('external', { role: 'external_user', text: 'Another request' }),
      message('new', { tools: [call, call] }),
      message('result', {
        role: 'tool',
        tools: [
          { ...call, output: 'a' },
          { ...call, output: 'b' }
        ]
      }),
      message('open', { tools: [call] }),
      message('late', { role: 'tool', conversation: 2, tools: [{ ...call, output: 'orphan' }] })
    ]
    const { messages, rows, lastPromptId } = chatTranscript(source)
    expect(messages[0]?.tools?.[0]?.output).toBeUndefined()
    expect(messages[2]?.tools?.map((tool) => tool.output)).toEqual(['a', 'b'])
    expect(messages.at(-1)?.tools?.[0]?.output).toBe('orphan')
    expect(rows.map((row) => row.kind)).toEqual(['activity', 'message', 'activity', 'activity'])
    expect(lastPromptId).toBe('external')
    expect(isProcessStep(messages[2]!)).toBe(true)
    expect(isProcessStep(messages.at(-1)!)).toBe(false)
  })
  it('keeps a late result attached by call ID while preserving its visible text', () => {
    const call = message('old', { tools: [{ callId: 'id-a', name: 'shell' }] })
    const prompt = message('steer', { role: 'user', text: 'One more thing' })
    const { messages, rows } = chatTranscript([
      call,
      prompt,
      message('late', {
        role: 'tool',
        text: 'File available',
        tools: [{ callId: 'id-a', name: 'shell', output: 'ok' }]
      })
    ])
    expect(messages[0]?.tools?.[0]?.output).toBe('ok')
    expect(messages[1]).toBe(prompt)
    expect(rows.map((row) => row.id)).toEqual(['old:tool:0', 'steer', 'late'])
  })
})
