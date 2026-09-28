import { type ChromeApi } from './types'

type ChromeWithManagement = ChromeApi & {
  management?: {
    getSelf?(): Promise<{ installType?: string }>
  }
}

type NavigatorBrand = {
  brand: string
  version: string
}

type NavigatorWithBrowserHints = Navigator & {
  userAgentData?: {
    brands?: NavigatorBrand[]
  }
  brave?: {
    isBrave(): Promise<boolean>
  }
}

/**
 * Returns the extension APIs, asserting the ones every Anda context relies on.
 * Shared by the service worker, the side panel, and the dashboard pages so a
 * missing API fails loudly at startup rather than as an undefined-property
 * error deep inside a handler.
 */
export function getChromeApi(): ChromeApi {
  const chromeApi = (globalThis as typeof globalThis & { chrome?: ChromeApi }).chrome
  if (
    !chromeApi?.runtime ||
    !chromeApi.storage?.local ||
    !chromeApi.tabs ||
    !chromeApi.scripting ||
    !chromeApi.i18n
  ) {
    throw new Error('Chrome extension APIs are unavailable. Load the built extension in Chrome.')
  }
  return chromeApi
}

export async function isDevelopmentMode(
  chromeApi: ChromeWithManagement = getChromeApi()
): Promise<boolean> {
  const getSelf = chromeApi.management?.getSelf
  if (!getSelf) {
    return false
  }

  try {
    const self = await getSelf.call(chromeApi.management)
    return self.installType === 'development'
  } catch (_error) {
    return false
  }
}

/**
 * Names the Chromium browser this extension runs in, e.g. 'chrome', 'edge' or
 * 'brave'. The value is part of the browser session id, so the detection
 * order must stay stable.
 */
export async function getCurrentBrowser(): Promise<string> {
  const browserNavigator = navigator as NavigatorWithBrowserHints
  const ua = browserNavigator.userAgent

  // Brand hints are low-entropy, so they are available without a request.
  for (const { brand } of browserNavigator.userAgentData?.brands || []) {
    if (brand.includes('Brave')) return 'brave'
    if (brand.includes('Microsoft') || brand.includes('Edge')) return 'edge'
    if (brand.includes('Opera') || brand === 'OPR') return 'opera'
    if (brand.includes('Google Chrome')) return 'chrome'
  }

  // Brave hides itself from the user agent but exposes this probe.
  if (typeof browserNavigator.brave !== 'undefined') {
    try {
      if (await browserNavigator.brave.isBrave()) return 'brave'
    } catch (_error) {}
  }

  if (ua.includes('Edg')) return 'edge'
  if (ua.includes('OPR') || ua.includes('Opera')) return 'opera'
  if (ua.includes('Brave')) return 'brave'
  if (ua.includes('Vivaldi')) return 'vivaldi'
  if (ua.includes('Arc')) return 'arc'

  if (ua.includes('Chrome')) return 'chrome'

  return 'chromium'
}
