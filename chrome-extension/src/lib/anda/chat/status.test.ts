import { afterEach, describe, expect, it, vi } from 'vitest'

afterEach(() => {
  vi.unstubAllGlobals()
  vi.resetModules()
})

async function loadStatusLabel() {
  vi.resetModules()
  vi.stubGlobal('chrome', { i18n: { getMessage: (key: string) => `<${key}>` } })
  return (await import('./status')).statusLabel
}

describe('statusLabel', () => {
  it('folds status codes into a few localized labels', async () => {
    const statusLabel = await loadStatusLabel()
    expect(statusLabel('submitted')).toBe('<working>')
    expect(statusLabel('reconnecting')).toBe('<statusConnecting>')
    expect(statusLabel('idle')).toBe('<ready>')
    expect(statusLabel('completed')).toBe('<statusCompleted>')
    expect(statusLabel('send failed')).toBe('<statusFailed>')
    expect(statusLabel('extension unavailable')).toBe('<statusUnavailable>')
  })

  it('shows an unknown code as sent', async () => {
    const statusLabel = await loadStatusLabel()
    expect(statusLabel('paused')).toBe('paused')
  })
})
