import { describe, expect, it } from 'vitest'
import { fileToAttachment, maxAttachmentBytes, mcpResourceToAttachment } from './attachments'

describe('fileToAttachment', () => {
  it('encodes a file as an inline resource', async () => {
    const attachment = await fileToAttachment(
      new File(['hello'], 'note.md', { type: 'text/markdown', lastModified: 1 })
    )
    expect(attachment).toMatchObject({
      id: 'note.md-5-1',
      name: 'note.md',
      size: 5,
      resource: { _id: 0, blob: 'aGVsbG8=', tags: ['text', 'md'] }
    })
  })

  it('refuses a file larger than one message can carry, before reading it', async () => {
    const file = new File([], 'video.mp4', { type: 'video/mp4' })
    Object.defineProperty(file, 'size', { value: maxAttachmentBytes + 1 })
    file.arrayBuffer = () => Promise.reject(new Error('read'))
    await expect(fileToAttachment(file)).rejects.toThrow('video.mp4')
  })
})

describe('mcpResourceToAttachment', () => {
  it('attaches a resource read like a picked file, naming its server', () => {
    const attachment = mcpResourceToAttachment('docs', {
      name: 'notes.md',
      uri: 'file:///notes.md',
      mime_type: 'text/markdown',
      size: 5,
      text: true,
      blob: 'aGVsbG8='
    })
    expect(attachment).toEqual({
      id: 'mcp-docs-file:///notes.md',
      name: 'notes.md',
      type: 'text/markdown',
      size: 5,
      resource: {
        _id: 0,
        tags: ['text', 'md'],
        name: 'notes.md',
        uri: 'file:///notes.md',
        mime_type: 'text/markdown',
        blob: 'aGVsbG8=',
        size: 5,
        metadata: { source: 'mcp:docs' }
      }
    })
  })
})
