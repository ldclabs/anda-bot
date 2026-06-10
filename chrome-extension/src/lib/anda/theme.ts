import type { AppearanceTheme } from '$lib/anda/client/types'
import { normalizeAppearanceTheme } from '$lib/service-worker/settings'

const darkSchemeQuery = '(prefers-color-scheme: dark)'

export type ResolvedAppearanceTheme = 'light' | 'dark'

export function resolveAppearanceTheme(
  theme: AppearanceTheme,
  systemPrefersDark = currentSystemPrefersDark()
): ResolvedAppearanceTheme {
  const normalized = normalizeAppearanceTheme(theme)
  if (normalized === 'system') {
    return systemPrefersDark ? 'dark' : 'light'
  }
  return normalized
}

export function applyAppearanceTheme(theme: AppearanceTheme): () => void {
  if (typeof document === 'undefined') {
    return () => undefined
  }

  const normalized = normalizeAppearanceTheme(theme)
  const mediaQuery =
    typeof window !== 'undefined' && typeof window.matchMedia === 'function'
      ? window.matchMedia(darkSchemeQuery)
      : null

  const update = () => {
    const resolved = resolveAppearanceTheme(normalized, mediaQuery?.matches || false)
    const root = document.documentElement
    root.classList.toggle('dark', resolved === 'dark')
    root.classList.toggle('light', resolved === 'light')
    root.dataset.appearanceTheme = normalized
    root.style.colorScheme = resolved
  }

  update()

  if (normalized !== 'system' || !mediaQuery) {
    return () => undefined
  }

  mediaQuery.addEventListener('change', update)
  return () => {
    mediaQuery.removeEventListener('change', update)
  }
}

function currentSystemPrefersDark(): boolean {
  return (
    typeof window !== 'undefined' &&
    typeof window.matchMedia === 'function' &&
    window.matchMedia(darkSchemeQuery).matches
  )
}
