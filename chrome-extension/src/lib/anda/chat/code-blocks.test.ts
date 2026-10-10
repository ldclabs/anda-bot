import { describe, expect, it } from 'vitest'
import { codeBlockLanguage } from './code-blocks'

describe('codeBlockLanguage', () => {
  it('reads the Prism language class', () => {
    expect(codeBlockLanguage('language-rust')).toBe('rust')
    expect(codeBlockLanguage('foo language-c++ bar')).toBe('c++')
    expect(codeBlockLanguage('language-csharp')).toBe('csharp')
  })

  it('is empty for plain blocks', () => {
    expect(codeBlockLanguage('')).toBe('')
    expect(codeBlockLanguage('katex-block')).toBe('')
  })
})
