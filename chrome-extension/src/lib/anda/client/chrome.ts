import type { ChromeApi } from './types'

export function getChromeApi(): ChromeApi {
	const chromeApi = (globalThis as typeof globalThis & { chrome?: ChromeApi }).chrome
	if (!chromeApi?.runtime || !chromeApi.storage?.local || !chromeApi.tabs || !chromeApi.i18n) {
		throw new Error('Chrome extension APIs are unavailable. Load the built extension in Chrome.')
	}
	return chromeApi
}
