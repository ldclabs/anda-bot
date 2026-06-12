import { describe, expect, it } from 'vitest'

import { formatUiMessage, getMessage, normalizeUiLanguage } from './i18n'

describe('normalizeUiLanguage', () => {
  it('maps Chinese variants to the bundled zh_CN locale', () => {
    expect(normalizeUiLanguage('zh-Hans')).toBe('zh_CN')
    expect(normalizeUiLanguage('zh_CN')).toBe('zh_CN')
    expect(normalizeUiLanguage('zh-TW')).toBe('zh_CN')
    expect(normalizeUiLanguage('ZH')).toBe('zh_CN')
  })

  it('maps supported language tags to locale directories', () => {
    expect(normalizeUiLanguage('en')).toBe('en')
    expect(normalizeUiLanguage('en-US')).toBe('en')
    expect(normalizeUiLanguage('ru-RU')).toBe('ru')
    expect(normalizeUiLanguage('ar')).toBe('ar')
    expect(normalizeUiLanguage('fr-FR')).toBe('fr')
    expect(normalizeUiLanguage('es-419')).toBe('es')
  })

  it('rejects unsupported or empty tags', () => {
    expect(normalizeUiLanguage('de-DE')).toBe('')
    expect(normalizeUiLanguage('')).toBe('')
    expect(normalizeUiLanguage(null)).toBe('')
    expect(normalizeUiLanguage(undefined)).toBe('')
    expect(normalizeUiLanguage('estonian-but-not-es')).toBe('')
  })
})

describe('formatUiMessage', () => {
  it('expands positional substitutions like chrome.i18n', () => {
    expect(formatUiMessage('Install $1 and restart now?', 'v1.2.3')).toBe(
      'Install v1.2.3 and restart now?'
    )
    expect(formatUiMessage("Format '.$1' rejected. Accepted: $2.", ['ogg', 'wav, mp3'])).toBe(
      "Format '.ogg' rejected. Accepted: wav, mp3."
    )
  })

  it('replaces missing substitutions with empty text and keeps escaped dollars', () => {
    expect(formatUiMessage('Failed: $1')).toBe('Failed: ')
    expect(formatUiMessage('Costs $$5 for $1', 'now')).toBe('Costs $5 for now')
  })
})

describe('getMessage', () => {
  it('returns empty text when no chrome API or override is available', () => {
    expect(getMessage('settings')).toBe('')
  })
})
