import { activeTab } from './browser-tabs'
import { pageAudioCaptureDispatcher } from './page-audio'
import { pageSpeechRecognitionDispatcher } from './page-speech'
import type {
  ChromeApi,
  PageAudioArgs,
  PageAudioResult,
  PageSpeechArgs,
  PageSpeechResult
} from './types'

const captureTabs = new WeakMap<ChromeApi, Map<string, number>>()

async function captureTab(
  chromeApi: ChromeApi,
  kind: string,
  action: string
): Promise<number | null> {
  let tabs = captureTabs.get(chromeApi)
  if (!tabs) captureTabs.set(chromeApi, (tabs = new Map()))
  const key = `anda-capture-${kind}`
  if (action === 'stop' || action === 'cancel') {
    const saved = await chromeApi.storage.session?.get(key)
    const tabId = tabs.get(kind) ?? saved?.[key]
    return typeof tabId === 'number' ? tabId : null
  }
  const tab = await activeTab(chromeApi)
  if (typeof tab?.id !== 'number' || !injectablePageUrl(tab.url)) {
    throw new Error('Voice capture needs an active http or https tab.')
  }
  if (action === 'start') {
    tabs.set(kind, tab.id)
    await chromeApi.storage.session?.set({ [key]: tab.id })
  }
  return tab.id
}

async function clearCaptureTab(chromeApi: ChromeApi, kind: string, tabId: number): Promise<void> {
  if (
    captureTabs.get(chromeApi)?.get(kind) !== undefined &&
    captureTabs.get(chromeApi)?.get(kind) !== tabId
  )
    return
  captureTabs.get(chromeApi)?.delete(kind)
  await chromeApi.storage.session?.remove(`anda-capture-${kind}`)
}

export async function handlePageSpeechRecognition(
  chromeApi: ChromeApi,
  args: PageSpeechArgs
): Promise<PageSpeechResult> {
  const tabId = await captureTab(chromeApi, 'speech', args.action)
  if (tabId === null) return { available: true, canceled: true }

  try {
    const [execution] = await chromeApi.scripting.executeScript<PageSpeechResult, PageSpeechArgs>({
      target: { tabId },
      world: 'MAIN',
      func: pageSpeechRecognitionDispatcher,
      args: [args]
    })
    return execution?.result || { error: 'Browser speech recognition did not return a result.' }
  } finally {
    if (args.action === 'stop' || args.action === 'cancel')
      await clearCaptureTab(chromeApi, 'speech', tabId)
  }
}

export async function handlePageAudioCapture(
  chromeApi: ChromeApi,
  args: PageAudioArgs
): Promise<PageAudioResult> {
  const tabId = await captureTab(chromeApi, 'audio', args.action)
  if (tabId === null) return { available: true, canceled: true }

  try {
    const [execution] = await chromeApi.scripting.executeScript<PageAudioResult, PageAudioArgs>({
      target: { tabId },
      world: 'MAIN',
      func: pageAudioCaptureDispatcher,
      args: [args]
    })
    return execution?.result || { error: 'Anda voice recording did not return a result.' }
  } finally {
    if (args.action === 'stop' || args.action === 'cancel')
      await clearCaptureTab(chromeApi, 'audio', tabId)
  }
}

function injectablePageUrl(url?: string): boolean {
  return /^https?:\/\//i.test(url || '')
}
