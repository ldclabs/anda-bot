import { type ChromeApi } from './types'

type NavigatorBrand = {
  brand: string
  version: string
}

type NavigatorWithBrowserHints = Navigator & {
  userAgentData?: {
    getHighEntropyValues(hints: string[]): Promise<{
      brands?: NavigatorBrand[]
    }>
  }
  brave?: {
    isBrave(): Promise<boolean>
  }
}

export function getChromeApi(): ChromeApi {
  const chromeApi = (globalThis as typeof globalThis & { chrome?: ChromeApi }).chrome
  if (!chromeApi?.runtime || !chromeApi.storage?.local || !chromeApi.tabs || !chromeApi.scripting) {
    throw new Error('Chrome extension APIs are unavailable.')
  }
  return chromeApi
}

export async function isDevelopmentMode() {
  const self = await chrome.management.getSelf()
  return self.installType === 'development'
}

/**
 * 增强版浏览器检测（异步）
 * 返回值示例：'Chrome' | 'Edge' | 'Brave' | 'Opera' | 'Vivaldi' | 'Arc' | 'Other Chromium'
 */
export async function getCurrentBrowser() {
  const browserNavigator = navigator as NavigatorWithBrowserHints
  const ua = browserNavigator.userAgent

  // === 1. 优先使用 User-Agent Client Hints（最现代、最可靠）===
  if (browserNavigator.userAgentData) {
    try {
      const uaData = await browserNavigator.userAgentData.getHighEntropyValues(['brands'])
      const brands = uaData.brands || []

      // 遍历 brands 数组，匹配已知品牌
      for (const { brand } of brands) {
        if (brand.includes('Brave')) return 'brave'
        if (brand.includes('Microsoft') || brand.includes('Edge')) return 'edge'
        if (brand.includes('Opera') || brand === 'OPR') return 'opera'
        if (brand.includes('Google Chrome') || brand === 'Google Chrome') return 'chrome'
      }
    } catch (e) {
      console.warn('userAgentData 获取失败，降级使用 UA')
    }
  }

  // === 2. 特定浏览器独有属性（高优先级）===
  // Brave 官方推荐方式
  if (typeof browserNavigator.brave !== 'undefined') {
    try {
      if (await browserNavigator.brave.isBrave()) return 'brave'
    } catch (e) {}
  }

  // === 3. UA 字符串精准匹配（兼容性最强）===
  if (ua.includes('Edg')) return 'edge'
  if (ua.includes('OPR') || ua.includes('Opera')) return 'opera'
  if (ua.includes('Brave')) return 'brave'
  if (ua.includes('Vivaldi')) return 'vivaldi'
  if (ua.includes('Arc')) return 'arc' // Arc 部分版本会带 Arc 标识

  if (ua.includes('Chrome')) return 'chrome'

  return 'chromium'
}
