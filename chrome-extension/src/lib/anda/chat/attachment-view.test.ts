import { describe, expect, it } from 'vitest'
import {
  attachmentCacheKey,
  attachmentDescription,
  attachmentDownloadUrl,
  attachmentHasDownloadData,
  attachmentMetaLabel,
  attachmentMimeType,
  attachmentObjectUrl,
  attachmentResourceBlob,
  attachmentResourceId,
  base64ToBytes,
  bytesToArrayBuffer,
  normalizeBase64,
  safeDownloadName,
  type AttachmentCaches
} from './attachment-view'
import type { ChatAttachment } from '../client/types'

function attachment(overrides: Partial<ChatAttachment> = {}): ChatAttachment {
  return {
    id: 'att-1',
    name: 'notes.txt',
    type: 'text/plain',
    size: 2048,
    resource: { _id: 0, tags: [], name: 'notes.txt' },
    ...overrides
  }
}

function caches(
  resourceBlobs: Record<number, string> = {},
  objectUrls: Record<string, string> = {}
): AttachmentCaches {
  return {
    resourceBlobs: new Map(Object.entries(resourceBlobs).map(([id, blob]) => [Number(id), blob])),
    objectUrls: new Map(Object.entries(objectUrls))
  }
}

describe('attachment identity', () => {
  it('prefers the declared type over the resource mime type', () => {
    expect(attachmentMimeType(attachment())).toBe('text/plain')
    expect(
      attachmentMimeType(
        attachment({ type: '', resource: { _id: 0, tags: [], name: 'x', mime_type: 'image/png' } })
      )
    ).toBe('image/png')
    expect(
      attachmentMimeType(attachment({ type: '', resource: { _id: 0, tags: [], name: 'x' } }))
    ).toBe('')
  })

  it('keys by resource id so copies share one cache entry', () => {
    expect(attachmentCacheKey(attachment({ resource: { _id: 42, tags: [], name: 'x' } }))).toBe(
      'resource:42'
    )
    expect(attachmentCacheKey(attachment())).toBe('att-1')
    expect(attachmentResourceId(attachment())).toBe(0)
  })
})

describe('attachment bytes', () => {
  it('prefers inline base64 over the fetched cache', () => {
    const inline = attachment({ resource: { _id: 42, tags: [], name: 'x', blob: '  AAA  ' } })
    expect(attachmentResourceBlob(inline, caches({ 42: 'BBB' }))).toBe('AAA')
  })

  it('falls back to the cache entry for its resource id', () => {
    const remote = attachment({ resource: { _id: 42, tags: [], name: 'x' } })
    expect(attachmentResourceBlob(remote, caches({ 42: ' BBB ' }))).toBe('BBB')
    expect(attachmentResourceBlob(remote, caches())).toBe('')
  })

  it('has no blob for an inline-only attachment with nothing inline', () => {
    expect(attachmentResourceBlob(attachment(), caches({ 42: 'BBB' }))).toBe('')
  })

  it('reads the minted object url by cache key', () => {
    const remote = attachment({ resource: { _id: 42, tags: [], name: 'x' } })
    expect(attachmentObjectUrl(remote, caches({}, { 'resource:42': 'blob:x' }))).toBe('blob:x')
    expect(attachmentObjectUrl(remote, caches())).toBe('')
  })
})

describe('attachment download', () => {
  it('prefers the object url over the resource uri', () => {
    const item = attachment({
      resource: { _id: 42, tags: [], name: 'x', uri: 'https://example.com/a.txt' }
    })
    expect(attachmentDownloadUrl(item, caches({}, { 'resource:42': 'blob:x' }))).toBe('blob:x')
    expect(attachmentDownloadUrl(item, caches())).toBe('https://example.com/a.txt')
  })

  it('rejects a uri whose scheme the page cannot load', () => {
    const relative = attachment({ resource: { _id: 0, tags: [], name: 'x', uri: '/local/a.txt' } })
    const custom = attachment({ resource: { _id: 0, tags: [], name: 'x', uri: 'anda://a.txt' } })
    expect(attachmentDownloadUrl(relative, caches())).toBe('')
    expect(attachmentDownloadUrl(custom, caches())).toBe('')
  })

  it('is downloadable from a url, an inline blob, or a resource id', () => {
    expect(
      attachmentHasDownloadData(
        attachment({ resource: { _id: 0, tags: [], name: 'x', uri: 'data:,hi' } }),
        caches()
      )
    ).toBe(true)
    expect(
      attachmentHasDownloadData(
        attachment({ resource: { _id: 0, tags: [], name: 'x', blob: 'AAA' } }),
        caches()
      )
    ).toBe(true)
    expect(
      attachmentHasDownloadData(
        attachment({ resource: { _id: 42, tags: [], name: 'x' } }),
        caches()
      )
    ).toBe(true)
    expect(attachmentHasDownloadData(attachment(), caches())).toBe(false)
  })

  it('strips characters filesystems reject', () => {
    expect(safeDownloadName('a/b:c*d?e"f<g>h|i')).toBe('a-b-c-d-e-f-g-h-i')
    expect(safeDownloadName('   ')).toBe('attachment')
    // A run of rejected characters collapses to one dash, which is a legal name.
    expect(safeDownloadName('///')).toBe('-')
  })
})

describe('attachment labels', () => {
  it('joins the mime type and size, dropping either when absent', () => {
    expect(attachmentMetaLabel(attachment())).toBe('text/plain / 2.0 KB')
    expect(attachmentMetaLabel(attachment({ size: undefined }))).toBe('text/plain')
    expect(
      attachmentMetaLabel(attachment({ type: '', resource: { _id: 0, tags: [], name: 'x' } }))
    ).toBe('2.0 KB')
  })

  it('strips the daemon system marker from the description', () => {
    expect(
      attachmentDescription(
        attachment({
          resource: { _id: 0, tags: [], name: 'x', description: '[$system:page]  A web page ' }
        })
      )
    ).toBe('A web page')
    expect(attachmentDescription(attachment())).toBe('')
  })
})

describe('base64 decoding', () => {
  it('accepts data urls, url-safe alphabets, whitespace, and missing padding', () => {
    expect(normalizeBase64('data:text/plain;base64,QUJD')).toBe('QUJD')
    expect(normalizeBase64('QU\nJD')).toBe('QUJD')
    expect(normalizeBase64('-_8')).toBe('+/8=')
    expect(normalizeBase64('QUJDRA==')).toBe('QUJDRA==')
  })

  it('decodes to bytes and to a standalone ArrayBuffer', () => {
    const bytes = base64ToBytes('QUJD')
    expect(Array.from(bytes)).toEqual([65, 66, 67])

    const buffer = bytesToArrayBuffer(bytes)
    expect(buffer.byteLength).toBe(3)
    expect(Array.from(new Uint8Array(buffer))).toEqual([65, 66, 67])
  })
})
