import { describe, expect, it } from 'vitest'
import { fileToAttachment, maxAttachmentBytes } from './attachments'

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
