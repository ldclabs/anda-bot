import { afterEach, describe, expect, it, vi } from 'vitest'
import { applyAppearanceTheme, resolveAppearanceTheme } from './theme'

afterEach(() => {
  vi.unstubAllGlobals()
})

describe('resolveAppearanceTheme', () => {
  it('uses the system preference when the appearance theme follows system', () => {
    expect(resolveAppearanceTheme('system', false)).toBe('light')
    expect(resolveAppearanceTheme('system', true)).toBe('dark')
  })

  it('keeps explicit light and dark themes independent from the system preference', () => {
    expect(resolveAppearanceTheme('light', true)).toBe('light')
    expect(resolveAppearanceTheme('dark', false)).toBe('dark')
  })

  it('applies explicit themes to the document root immediately', () => {
    const root = createRootStub()
    vi.stubGlobal('document', { documentElement: root })
    vi.stubGlobal('window', { matchMedia: vi.fn() })

    applyAppearanceTheme('dark')
    expect(root.classes.has('dark')).toBe(true)
    expect(root.classes.has('light')).toBe(false)
    expect(root.dataset.appearanceTheme).toBe('dark')
    expect(root.style.colorScheme).toBe('dark')

    applyAppearanceTheme('light')
    expect(root.classes.has('dark')).toBe(false)
    expect(root.classes.has('light')).toBe(true)
    expect(root.dataset.appearanceTheme).toBe('light')
    expect(root.style.colorScheme).toBe('light')
  })

  it('updates system themes when the system color scheme changes', () => {
    const root = createRootStub()
    const mediaQuery = createMediaQueryStub(false)
    vi.stubGlobal('document', { documentElement: root })
    vi.stubGlobal('window', { matchMedia: vi.fn(() => mediaQuery) })

    const cleanup = applyAppearanceTheme('system')
    expect(root.classes.has('light')).toBe(true)
    expect(root.classes.has('dark')).toBe(false)
    expect(root.dataset.appearanceTheme).toBe('system')

    mediaQuery.matches = true
    mediaQuery.listener?.()
    expect(root.classes.has('dark')).toBe(true)
    expect(root.classes.has('light')).toBe(false)

    cleanup()
    expect(mediaQuery.listener).toBeNull()
  })
})

function createRootStub() {
  const classes = new Set<string>()
  return {
    classes,
    dataset: {} as Record<string, string>,
    style: {} as Record<string, string>,
    classList: {
      toggle(name: string, enabled: boolean) {
        if (enabled) {
          classes.add(name)
        } else {
          classes.delete(name)
        }
      }
    }
  }
}

function createMediaQueryStub(matches: boolean) {
  return {
    matches,
    listener: null as (() => void) | null,
    addEventListener(_event: 'change', listener: () => void) {
      this.listener = listener
    },
    removeEventListener(_event: 'change', listener: () => void) {
      if (this.listener === listener) {
        this.listener = null
      }
    }
  }
}
