import { type ChromeApi } from './types'

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
