import type { ChatAttachment, Resource } from './client/types'

export const pageElementContextMenuId = 'anda-send-page-element-to-chat'
export const pageElementMemoryKey = '__andaLastRightClickedElement'
export const pageElementStorageKey = 'andaLastRightClickedElement'
export const pageElementAttachmentRequestStorageKey = 'andaPageElementAttachmentRequest'
export const pageElementAttachmentMessageType = 'anda_page_element_attachment_request'
export const pageElementCaptureMessageType = 'anda_page_element_captured'

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
  const content = JSON.stringify(
    {
      type: 'anda.page_element',
      captured_at: new Date(request.element.capturedAt || request.createdAt).toISOString(),
      element: request.element
    },
    null,
    2
  )
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
      request_id: request.id,
      page_url: request.element.pageUrl,
      page_title: request.element.pageTitle,
      frame_url: request.element.frameUrl,
      xpath: request.element.xpath,
      css_path: request.element.cssPath,
      tag_name: request.element.tagName,
      element_id: request.element.id || undefined,
      class_name: request.element.className || undefined,
      selected_text: request.element.selectedText || undefined,
      captured_at: request.element.capturedAt
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

function pageElementAttachmentName(element: PageElementInfo): string {
  const source = element.pageTitle.trim() || hostnameFromUrl(element.pageUrl) || 'content'
  const label = sanitizeFilePart(`page-content-${source}`) || 'page-content'
  return `${label}.json`
}

function pageElementDescription(element: PageElementInfo): string {
  const text = element.innerText || element.textContent
  const summary = text.replace(/\s+/g, ' ').trim().slice(0, 160)
  return summary
    ? `Web page content from ${element.pageTitle || element.pageUrl}: ${summary}`
    : `Web page content from ${element.pageTitle || element.pageUrl}`
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
