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

  it('renders closed mermaid fences as diagrams', () => {
    const html = renderMarkdown('Flow:\n\n```mermaid\nflowchart LR\n  A[Start] --> B[Done]\n```\n')
    expect(html).toContain('<div class="md-diagram"><svg')
    expect(html).toContain('class="pele pele-flowchart"')
    expect(html).not.toContain('<pre>')

    const quoted = renderMarkdown('> ```mermaid\n> sequenceDiagram\n>   A->>B: Hi\n> ```')
    expect(quoted).toContain('<blockquote>\n<div class="md-diagram"><svg')
  })

  it('accepts a capitalized mermaid tag', () => {
    const html = renderMarkdown('```Mermaid\nflowchart LR\n  A --> B\n```\n')
    expect(html).toContain('<div class="md-diagram"><svg')
    expect(html).not.toContain('<pre>')
  })

  it('renders every diagram in a message, not just the first', () => {
    const html = renderMarkdown(
      [
        'One:',
        '',
        '```mermaid',
        'flowchart LR',
        '  A --> B',
        '```',
        '',
        'Two:',
        '',
        '```mermaid',
        'sequenceDiagram',
        '  A->>B: Hi',
        '```'
      ].join('\n')
    )
    expect(html.match(/<svg/g)).toHaveLength(2)
    expect(html).toContain('pele-flowchart')
    expect(html).toContain('pele-sequence')
  })

  it('keeps a streaming mermaid fence as code until it closes', () => {
    for (const partial of [
      '```mermaid\nflowchart LR\n  A --> B',
      '```mermaid\nflowchart LR\n  A --> B\n',
      '````mermaid\nflowchart LR\n  A --> B\n```'
    ]) {
      const html = renderMarkdown(partial)
      expect(html).not.toContain('<svg')
      expect(html).toContain('A --&gt; B')
    }
  })

  it('falls back to escaped code for invalid diagrams', () => {
    const html = renderMarkdown('```mermaid\nflowchart LR\n  A["<img src=x>"] -->\n```')
    expect(html).toBe(
      '<pre><code>flowchart LR\n  A[&quot;&lt;img src=x&gt;&quot;] --&gt;\n</code></pre>'
    )
  })

  it('applies the markdown link policy inside diagrams', () => {
    const html = renderMarkdown(
      [
        '```mermaid',
        'flowchart LR',
        '  A --> B --> C',
        '  click A href "https://example.com" _self',
        '  click B "./notes.md"',
        '  click C "javascript:alert(1)"',
        '```'
      ].join('\n')
    )
    expect(html).toContain(
      '<a href="https://example.com/" rel="noopener noreferrer" target="_blank">'
    )
    expect(html).not.toContain('_self')
    expect(html).not.toContain('notes.md')
    expect(html).not.toContain('javascript:')
  })
})
