export const UI_LANGUAGES = ['en', 'zh_CN', 'ru', 'ar', 'fr', 'es'] as const
export type UiLanguage = (typeof UI_LANGUAGES)[number]

export const uiLanguageStorageKey = 'uiLanguage'

type LocaleMessages = Record<string, { message?: string }>

let overrideLanguage: UiLanguage | '' = ''
let overrideMessages: LocaleMessages | null = null

function chromeApi(): typeof chrome | null {
  return typeof chrome !== 'undefined' ? chrome : null
}

/**
 * Maps any language tag ("zh-Hans", "zh_CN", "fr-FR", ...) to a bundled
 * `_locales` directory, or '' when the tag is unsupported.
 */
export function normalizeUiLanguage(value: unknown): UiLanguage | '' {
  const tag = String(value ?? '')
    .trim()
    .replace(/_/g, '-')
    .toLowerCase()
  if (!tag) {
    return ''
  }
  if (tag.startsWith('zh')) {
    return 'zh_CN'
  }
  for (const language of UI_LANGUAGES) {
    if (language === 'zh_CN') {
      continue
    }
    if (tag === language || tag.startsWith(`${language}-`)) {
      return language
    }
  }
  return ''
}

/**
 * Drop-in replacement for chrome.i18n.getMessage that prefers the language
 * selected in the Anda launcher over the browser UI language.
 */
export function getMessage(key: string, substitutions?: string | string[]): string {
  const message = overrideMessages?.[key]?.message
  if (typeof message === 'string') {
    return formatUiMessage(message, substitutions)
  }
  try {
    return chromeApi()?.i18n?.getMessage?.(key, substitutions) || ''
  } catch {
    return ''
  }
}

/** Expands `$1`..`$9` like chrome.i18n.getMessage; `$$` is a literal `$`. */
export function formatUiMessage(message: string, substitutions?: string | string[]): string {
  const subs =
    substitutions === undefined
      ? []
      : Array.isArray(substitutions)
        ? substitutions
        : [substitutions]
  return message.replace(/\$([1-9$])/g, (_match, token: string) =>
    token === '$' ? '$' : String(subs[Number(token) - 1] ?? '')
  )
}

export function activeUiLanguage(): UiLanguage | '' {
  return overrideLanguage
}

/** Loads the persisted language override; call before rendering UI text. */
export async function initI18n(): Promise<void> {
  const api = chromeApi()
  if (!api?.storage?.local) {
    return
  }
  try {
    const saved = await api.storage.local.get([uiLanguageStorageKey])
    await applyUiLanguage(saved?.[uiLanguageStorageKey])
  } catch {
    // Keep browser-locale messages when storage is unavailable.
  }
}

/** Switches the in-memory message table; pass '' (or unsupported) to reset. */
export async function applyUiLanguage(value: unknown): Promise<void> {
  const language = normalizeUiLanguage(value)
  const api = chromeApi()
  if (!language || !api?.runtime?.getURL || language === browserUiLanguage()) {
    overrideLanguage = ''
    overrideMessages = null
    applyDocumentDirection()
    return
  }

  try {
    const response = await fetch(api.runtime.getURL(`_locales/${language}/messages.json`))
    overrideMessages = (await response.json()) as LocaleMessages
    overrideLanguage = language
  } catch {
    overrideLanguage = ''
    overrideMessages = null
  }
  applyDocumentDirection()
}

function browserUiLanguage(): UiLanguage | '' {
  try {
    return normalizeUiLanguage(chromeApi()?.i18n?.getUILanguage?.())
  } catch {
    return ''
  }
}

/** Keeps Arabic pages right-to-left when the override changes the language. */
function applyDocumentDirection(): void {
  if (typeof document === 'undefined') {
    return
  }
  let dir = ''
  if (overrideLanguage) {
    dir = overrideLanguage === 'ar' ? 'rtl' : 'ltr'
  } else {
    try {
      dir = chromeApi()?.i18n?.getMessage?.('@@bidi_dir') || ''
    } catch {
      dir = ''
    }
  }
  if (dir === 'rtl' || dir === 'ltr') {
    document.documentElement.dir = dir
  }
}
