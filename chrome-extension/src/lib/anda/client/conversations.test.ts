import { describe, expect, it } from 'vitest'

import { normalizeMessage } from './conversations'
import type { Message, Resource } from './types'

function resource(overrides: Partial<Resource> = {}): Resource {
  return {
    _id: 101,
    tags: [],
    name: 'report.pdf',
    mime_type: 'application/pdf',
    size: 4096,
    ...overrides
  }
}

describe('normalizeMessage', () => {
  it('maps message resources to chat attachments', () => {
    const message: Message = {
      role: 'user',
      content: [
        { type: 'Text', text: 'please read this' },
        { type: 'Resource', ...resource() }
      ],
      timestamp: 1234
    }

    const normalized = normalizeMessage(message, {
      conversation: 55,
      index: 0,
      fallbackTimestamp: 999
    })

    expect(normalized?.text).toBe('please read this')
    expect(normalized?.attachments).toHaveLength(1)
    expect(normalized?.attachments?.[0]).toMatchObject({
      id: 'resource-101',
      name: 'report.pdf',
      type: 'application/pdf',
      size: 4096
    })
  })

  it('keeps attachment-only system runtime messages renderable', () => {
    const message: Message = {
      role: 'assistant',
      name: '$system',
      content: [
        { type: 'Text', text: '[$system:media-understanding] hidden runtime text' },
        {
          type: 'Resource',
          ...resource({ _id: 202, name: 'generated.png', mime_type: 'image/png' })
        }
      ]
    }

    const normalized = normalizeMessage(message, {
      conversation: 55,
      index: 1,
      fallbackTimestamp: 999
    })

    expect(normalized?.text).toBe('')
    expect(normalized?.attachments?.[0]).toMatchObject({
      id: 'resource-202',
      name: 'generated.png',
      type: 'image/png'
    })
  })

  it('moves embedded runtime system text out of a user message body', () => {
    const message: Message = {
      role: 'user',
      content: [
        { type: 'Text', text: '是的，一个交互优化' },
        {
          type: 'Text',
          text: '[$system: kind="background shell"]\nThis message is from the Anda runtime.\n\n"tool output"'
        },
        { type: 'Text', text: '后面的提交变更就没必要跑测试了' }
      ],
      timestamp: 1234
    }

    const normalized = normalizeMessage(message, {
      conversation: 55,
      index: 2,
      fallbackTimestamp: 999
    })

    expect(normalized?.role).toBe('user')
    expect(normalized?.text).toBe('是的，一个交互优化\n\n后面的提交变更就没必要跑测试了')
    expect(normalized?.thinkingText).toContain('background shell')
    expect(normalized?.thinkingText).toContain('tool output')
  })
})
