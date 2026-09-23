import { formatFileSize } from '$lib/utils/format'
import type { ChatAttachment } from '../client/types'

/**
 * How a message attachment reads and where its bytes come from.
 *
 * An attachment arrives one of three ways: inline base64 on the resource, a
 * daemon resource id the transcript fetches on demand, or a plain URI. The
 * caches a view holds for the first two are passed in rather than read from
 * module state, which keeps every function here pure and testable.
 */

/** Blobs fetched by resource id, and object URLs minted from them. */
export interface AttachmentCaches {
  resourceBlobs: ReadonlyMap<number, string>
  objectUrls: ReadonlyMap<string, string>
}

export function attachmentMimeType(attachment: ChatAttachment): string {
  return attachment.type || attachment.resource.mime_type || ''
}

/** 0 when the attachment is inline-only and has no daemon resource. */
export function attachmentResourceId(attachment: ChatAttachment): number {
  return attachment.resource._id || 0
}

/** Keyed by resource id when there is one, so copies share a cached blob. */
export function attachmentCacheKey(attachment: ChatAttachment): string {
  const id = attachmentResourceId(attachment)
  return id ? `resource:${id}` : attachment.id
}

/** Inline base64 if present, else whatever was fetched for its resource id. */
export function attachmentResourceBlob(
  attachment: ChatAttachment,
  caches: AttachmentCaches
): string {
  const inline = attachment.resource.blob?.trim()
  if (inline) {
    return inline
  }
  const id = attachmentResourceId(attachment)
  return id ? (caches.resourceBlobs.get(id) || '').trim() : ''
}

export function attachmentObjectUrl(attachment: ChatAttachment, caches: AttachmentCaches): string {
  return caches.objectUrls.get(attachmentCacheKey(attachment)) || ''
}

export function attachmentMetaLabel(attachment: ChatAttachment): string {
  return [attachmentMimeType(attachment), formatFileSize(attachment.size)]
    .filter(Boolean)
    .join(' / ')
}

/** The resource description, minus the `[$system:…]` marker the daemon adds. */
export function attachmentDescription(attachment: ChatAttachment): string {
  return (attachment.resource.description || '')
    .trim()
    .replace(/^\[\$system:[^\]]+\]\s*/i, '')
    .trim()
}

/**
 * A URL the browser can fetch: the minted object URL, else the resource URI when
 * it uses a scheme the page may load. Relative and unknown schemes yield ''.
 */
export function attachmentDownloadUrl(
  attachment: ChatAttachment,
  caches: AttachmentCaches
): string {
  const objectUrl = attachmentObjectUrl(attachment, caches)
  if (objectUrl) {
    return objectUrl
  }
  if (attachmentResourceBlob(attachment, caches)) return ''
  const uri = attachment.resource.uri?.trim() || ''
  return /^(https?:|file:|data:|blob:)/i.test(uri) ? uri : ''
}

/** True when the attachment can be saved now or fetched to be saved. */
export function attachmentHasDownloadData(
  attachment: ChatAttachment,
  caches: AttachmentCaches
): boolean {
  return Boolean(
    attachmentDownloadUrl(attachment, caches) ||
    attachmentResourceBlob(attachment, caches) ||
    attachmentResourceId(attachment)
  )
}

/** Strips path separators and other characters filesystems reject. */
export function safeDownloadName(name: string): string {
  return name.replace(/[\\/:*?"<>|]+/g, '-').trim() || 'attachment'
}

export { normalizeBase64, base64ToBytes } from '$lib/utils/base64'

export function bytesToArrayBuffer(bytes: Uint8Array): ArrayBuffer {
  const buffer = new ArrayBuffer(bytes.byteLength)
  new Uint8Array(buffer).set(bytes)
  return buffer
}
