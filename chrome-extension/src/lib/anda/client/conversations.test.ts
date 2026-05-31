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
})
