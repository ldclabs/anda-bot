import type { ChatAttachment, Resource } from './client/types'

export const pageElementContextMenuId = 'anda-send-page-element-to-chat'
export const pageElementMemoryKey = '__andaLastRightClickedElement'
export const pageElementDomMemoryKey = '__andaLastRightClickedDomElement'
export const pageElementStorageKey = 'andaLastRightClickedElement'
export const pageElementAttachmentRequestStorageKey = 'andaPageElementAttachmentRequest'
export const pageElementAttachmentMessageType = 'anda_page_element_attachment_request'
export const pageElementCaptureMessageType = 'anda_page_element_captured'

const maxAttachmentTextChars = 100_000
const maxSemanticAttributes = 12
const maxSemanticAttributeValueChars = 500
const semanticAttributeNames = new Set([
  'alt',
  'aria-description',
  'aria-label',
  'content',
  'datetime',
  'href',
  'name',
  'placeholder',
  'src',
  'title',
  'type',
  'value'
])

export interface PageElementRect {
  x: number
  y: number
  width: number
  height: number
  top: number
  right: number
  bottom: number
  left: number
}

export interface PageElementInfo {
  tagName: string
  id?: string | null
  className?: string | null
  role?: string | null
  innerText: string
  textContent: string
  outerHTML: string
  attributes: Record<string, string>
  xpath: string
  cssPath: string
  pageUrl: string
  pageTitle: string
  frameUrl: string
  selectedText?: string
  rect?: PageElementRect | null
  capturedAt: number
}

export interface PageElementAttachmentRequest {
  id: string
  createdAt: number
  element: PageElementInfo
}

export function isPageElementInfo(value: unknown): value is PageElementInfo {
  const item = recordValue(value)
  if (!item) {
    return false
  }
  return (
    typeof item.tagName === 'string' &&
    typeof item.innerText === 'string' &&
    typeof item.textContent === 'string' &&
    typeof item.outerHTML === 'string' &&
    typeof item.xpath === 'string' &&
    typeof item.cssPath === 'string' &&
    typeof item.pageUrl === 'string' &&
    typeof item.pageTitle === 'string' &&
    typeof item.frameUrl === 'string' &&
    typeof item.capturedAt === 'number' &&
    Boolean(recordValue(item.attributes))
  )
}

export function isPageElementAttachmentRequest(
  value: unknown
): value is PageElementAttachmentRequest {
  const item = recordValue(value)
  if (!item) {
    return false
  }
  return (
    typeof item.id === 'string' &&
    typeof item.createdAt === 'number' &&
    isPageElementInfo(item.element)
  )
}

export function pageElementInfoToAttachment(request: PageElementAttachmentRequest): ChatAttachment {
  const name = pageElementAttachmentName(request.element)
  const content = JSON.stringify(pageElementAttachmentPayload(request), null, 2)
  const size = new TextEncoder().encode(content).length
  const resource: Resource = {
    _id: 0,
    tags: Array.from(new Set(['webpage', 'dom', 'element'].filter(Boolean))),
    name,
    description: pageElementDescription(request.element),
    uri: request.element.pageUrl || request.element.frameUrl || undefined,
    mime_type: 'application/json',
    blob: utf8ToBase64(content),
    size,
    metadata: {
      source: 'chrome_extension_context_menu',
      request_id: request.id
    }
  }
  return {
    id: `page-element-${request.id}`,
    name,
    type: resource.mime_type,
    size,
    resource
  }
}

function pageElementAttachmentPayload(request: PageElementAttachmentRequest) {
  const element = request.element
  const text = pageElementText(element)
  const selectedText = cleanPageElementText(element.selectedText || '')
  const attributes = semanticElementAttributes(element, text)
  return {
    type: 'anda.page_element',
    captured_at: new Date(element.capturedAt || request.createdAt).toISOString(),
    source: {
      title: element.pageTitle || undefined,
      url: element.pageUrl || undefined,
      frame_url:
        element.frameUrl && element.frameUrl !== element.pageUrl ? element.frameUrl : undefined
    },
    element: {
      tag: element.tagName.toLowerCase(),
      id: element.id || undefined,
      role: element.role || undefined,
      text: text || undefined,
      selected_text: selectedText && selectedText !== text ? selectedText : undefined,
      attributes: Object.keys(attributes).length ? attributes : undefined
    }
  }
}

function pageElementAttachmentName(element: PageElementInfo): string {
  const source = element.pageTitle.trim() || hostnameFromUrl(element.pageUrl) || 'content'
  const label = sanitizeFilePart(`page-content-${source}`) || 'page-content'
  return `${label}.json`
}

function pageElementDescription(element: PageElementInfo): string {
  return `Web page content from ${element.pageTitle || element.pageUrl || 'current page'}`
}

function pageElementText(element: PageElementInfo): string {
  return trimString(
    cleanPageElementText(element.innerText || element.textContent),
    maxAttachmentTextChars
  )
}

function cleanPageElementText(value: string): string {
  return value
    .replace(/\r\n?/g, '\n')
    .split('\n')
    .map((line) => line.replace(/[^\S\n]+/g, ' ').trim())
    .join('\n')
    .replace(/\n{3,}/g, '\n\n')
    .trim()
}

function semanticElementAttributes(element: PageElementInfo, text: string): Record<string, string> {
  const attributes: Record<string, string> = {}
  for (const [name, rawValue] of Object.entries(element.attributes)) {
    const normalizedName = name.toLowerCase()
    if (!semanticAttributeNames.has(normalizedName)) {
      continue
    }

    const value = trimString(cleanPageElementText(rawValue), maxSemanticAttributeValueChars)
    if (!value || value === text) {
      continue
    }

    attributes[normalizedName] = value
    if (Object.keys(attributes).length >= maxSemanticAttributes) {
      break
    }
  }
  return attributes
}

function sanitizeFilePart(value: string): string {
  return value
    .trim()
    .replace(/[^\w#.-]+/g, '-')
    .replace(/-+/g, '-')
    .replace(/^-|-$/g, '')
    .slice(0, 80)
}

function hostnameFromUrl(value: string): string {
  try {
    return new URL(value).hostname
  } catch (_error) {
    return ''
  }
}

function trimString(value: string, maxChars: number): string {
  return value.length > maxChars ? `${value.slice(0, maxChars)}...` : value
}

function utf8ToBase64(value: string): string {
  const bytes = new TextEncoder().encode(value)
  const chunkSize = 0x8000
  let binary = ''
  for (let index = 0; index < bytes.length; index += chunkSize) {
    binary += String.fromCharCode(...bytes.subarray(index, index + chunkSize))
  }
  return btoa(binary)
}

function recordValue(value: unknown): Record<string, unknown> | null {
  return value && typeof value === 'object' && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : null
}
