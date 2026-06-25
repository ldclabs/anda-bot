import { describe, expect, it } from 'vitest'
import { pageElementInfoToAttachment, type PageElementAttachmentRequest } from './page-element'

describe('pageElementInfoToAttachment', () => {
  it('stores page content as base64-encoded JSON', () => {
    const request: PageElementAttachmentRequest = {
      id: 'request-1',
      createdAt: Date.parse('2026-06-22T00:00:00Z'),
      element: {
        tagName: 'ARTICLE',
        id: 'post',
        className: 'content',
        role: null,
        innerText: 'Visible page content',
        textContent: 'Visible page content',
        outerHTML: '<article id="post">Visible page content</article>',
        attributes: { id: 'post' },
        xpath: '//*[@id="post"]',
        cssPath: '#post',
        pageUrl: 'https://example.com/post',
        pageTitle: 'Example Post',
        frameUrl: 'https://example.com/post',
        selectedText: '',
        rect: null,
        capturedAt: Date.parse('2026-06-22T00:00:00Z')
      }
    }

    const attachment = pageElementInfoToAttachment(request)
    const raw = attachment.resource.blob || ''
    const decoded = base64ToUtf8(raw)
    const parsed = JSON.parse(decoded)

    expect(raw.trim().startsWith('{')).toBe(false)
    expect(() => JSON.parse(decoded)).not.toThrow()
    expect(parsed).toMatchObject({
      type: 'anda.page_element',
      source: {
        title: 'Example Post',
        url: 'https://example.com/post'
      },
      element: {
        tag: 'article',
        id: 'post',
        text: 'Visible page content'
      }
    })
    expect(parsed.element).not.toHaveProperty('innerText')
    expect(parsed.element).not.toHaveProperty('textContent')
    expect(parsed.element).not.toHaveProperty('outerHTML')
    expect(parsed.element).not.toHaveProperty('cssPath')
    expect(parsed.element).not.toHaveProperty('attributes')
    expect(attachment.resource.description).not.toContain('Visible page content')
    expect(attachment.resource.metadata).not.toHaveProperty('css_path')
    expect(attachment.resource.metadata).not.toHaveProperty('class_name')
    expect(attachment.resource.metadata).not.toHaveProperty('page_title')
    expect(attachment.resource.metadata).not.toHaveProperty('tag_name')
    expect(attachment.name).toBe('page-content-Example-Post.json')
  })
})

function base64ToUtf8(value: string): string {
  const binary = atob(value)
  const bytes = new Uint8Array(binary.length)
  for (let index = 0; index < binary.length; index += 1) {
    bytes[index] = binary.charCodeAt(index)
  }
  return new TextDecoder().decode(bytes)
}
