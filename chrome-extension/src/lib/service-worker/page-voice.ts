import { activeTab } from './browser-actions'
import { pageAudioCaptureDispatcher } from './page-audio'
import { pageSpeechRecognitionDispatcher } from './page-speech'
import type {
  ChromeApi,
  PageAudioArgs,
  PageAudioResult,
  PageSpeechArgs,
  PageSpeechResult
} from './types'

export async function handlePageSpeechRecognition(
  chromeApi: ChromeApi,
  args: PageSpeechArgs
): Promise<PageSpeechResult> {
  const tab = await activeTab(chromeApi)
  const tabId = tab?.id
  if (!tabId) {
    throw new Error('No active tab is available for browser speech recognition.')
  }
  if (!injectablePageUrl(tab.url)) {
    throw new Error('Browser speech recognition needs an active http or https tab.')
  }

  const [execution] = await chromeApi.scripting.executeScript<PageSpeechResult, PageSpeechArgs>({
    target: { tabId },
    world: 'MAIN',
    func: pageSpeechRecognitionDispatcher,
    args: [args]
  })
  return execution?.result || { error: 'Browser speech recognition did not return a result.' }
}

export async function handlePageAudioCapture(
  chromeApi: ChromeApi,
  args: PageAudioArgs
): Promise<PageAudioResult> {
  const tab = await activeTab(chromeApi)
  const tabId = tab?.id
  if (!tabId) {
    throw new Error('No active tab is available for voice recording.')
  }
  if (!injectablePageUrl(tab.url)) {
    throw new Error('Anda voice recording needs an active http or https tab.')
  }

  const [execution] = await chromeApi.scripting.executeScript<PageAudioResult, PageAudioArgs>({
    target: { tabId },
    world: 'MAIN',
    func: pageAudioCaptureDispatcher,
    args: [args]
  })
  return execution?.result || { error: 'Anda voice recording did not return a result.' }
}

function injectablePageUrl(url?: string): boolean {
  return /^https?:\/\//i.test(url || '')
}
