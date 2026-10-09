import { describe, expect, it } from 'vitest'
import { base64ToBytes, bytesToBase64, normalizeBase64 } from './base64'

describe('base64 decoding', () => {
  it('accepts data urls, url-safe alphabets, whitespace, and missing padding', () => {
    expect(normalizeBase64('data:text/plain;base64,QUJD')).toBe('QUJD')
    expect(normalizeBase64('QU\nJD')).toBe('QUJD')
    expect(normalizeBase64('-_8')).toBe('+/8=')
    expect(normalizeBase64('QUJDRA==')).toBe('QUJDRA==')
  })

  it('decodes the daemon b64: base64url form of resource blobs', () => {
    expect(Array.from(base64ToBytes('b64:-_8='))).toEqual([251, 255])
  })

  it('round-trips bytes', () => {
    const bytes = base64ToBytes('QUJD')
    expect(Array.from(bytes)).toEqual([65, 66, 67])
    expect(bytesToBase64(bytes)).toBe('QUJD')
  })
})
