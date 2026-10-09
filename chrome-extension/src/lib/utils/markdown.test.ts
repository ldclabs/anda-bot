import { describe, expect, it } from 'vitest'
import { renderMarkdown } from './markdown'

describe('renderMarkdown', () => {
  it('keeps enumerations and plus-minus text as written', () => {
    const html = renderMarkdown('Pick (a), (b) or (c). Range +-5, (tm) (r).')
    expect(html).toContain('(c)')
    expect(html).toContain('+-5')
    expect(html).toContain('(tm) (r)')
    expect(html).not.toMatch(/[©±™®]/)
  })

  it('renders relative links as plain text and opens absolute ones in a new tab', () => {
    expect(renderMarkdown('[doc](./a.md)')).toBe('<p>doc</p>\n')
    expect(renderMarkdown('[site](https://example.com)')).toContain(
      '<a href="https://example.com" target="_blank" rel="noopener noreferrer">site</a>'
    )
  })
})
