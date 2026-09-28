import {
  actionTimeoutMs,
  activeTab,
  normalizeOptionalText,
  positiveInteger,
  requirePositiveInteger,
  tabSummary
} from './browser-tabs'
import type { BrowserActionArgs, BrowserActionResult, ChromeApi, ChromeDownloadItem } from './types'

/**
 * Actions answered from browser state rather than from a page: the tab list
 * and the download manager.
 *
 * `downloads` is an optional Chrome API, so each entry point asserts it is
 * present and fails with a message naming the missing permission instead of
 * throwing on an undefined property.
 */

export async function listTabs(
  chromeApi: ChromeApi,
  args: BrowserActionArgs
): Promise<BrowserActionResult> {
  const windowId = positiveInteger(args.window_id)
  const queryInfo = windowId ? { windowId } : {}
  const tabs = await chromeApi.tabs.query(queryInfo)
  const active = windowId ? tabs.find((tab) => tab.active) || null : await activeTab(chromeApi)
  return {
    tabs: tabs.map(tabSummary),
    active_tab_id: active?.id || null
  }
}

export async function downloadFile(
  chromeApi: ChromeApi,
  args: BrowserActionArgs
): Promise<BrowserActionResult> {
  const downloads = requireDownloads(chromeApi)
  const url = normalizeOptionalText(args.url)
  if (!url) {
    throw new Error('download requires url')
  }
  const filename = normalizeOptionalText(args.filename)
  const downloadId = await downloads.download({
    url,
    filename,
    saveAs: args.save_as ?? false
  })
  const download = await waitForDownloadComplete(downloads, downloadId, actionTimeoutMs(args))
  return {
    downloaded: true,
    download_id: downloadId,
    url,
    filename: filename || null,
    download: downloadSummary(download)
  }
}

export async function listDownloads(
  chromeApi: ChromeApi,
  args: BrowserActionArgs
): Promise<BrowserActionResult> {
  const downloads = requireDownloads(chromeApi)
  const downloadId = positiveInteger(args.download_id)
  const limit = Math.max(1, Math.min(100, positiveInteger(args.amount) || 50))
  const query: { id?: number; limit?: number; orderBy?: string[]; state?: string } = {
    limit,
    orderBy: ['-startTime']
  }
  if (downloadId) {
    query.id = downloadId
  }
  const state = normalizeOptionalText(args.value)
  if (state) {
    query.state = state
  }
  const downloadsFound = await downloads.search(query)
  return {
    downloads: downloadsFound.map(downloadSummary),
    count: downloadsFound.length
  }
}

export async function cancelDownload(
  chromeApi: ChromeApi,
  args: BrowserActionArgs
): Promise<BrowserActionResult> {
  const downloads = requireDownloads(chromeApi)
  const downloadId = requirePositiveInteger(
    args.download_id,
    'cancel_download requires download_id'
  )
  await downloads.cancel(downloadId)
  return { canceled: true, download_id: downloadId }
}

export async function openDownload(
  chromeApi: ChromeApi,
  args: BrowserActionArgs
): Promise<BrowserActionResult> {
  const downloads = requireDownloads(chromeApi)
  const downloadId = requirePositiveInteger(args.download_id, 'open_download requires download_id')
  // downloads.open requires a user gesture; WebSocket commands have none.
  if (!downloads.show) throw new Error('Showing downloads is unavailable')
  await downloads.show(downloadId)
  return { opened: false, shown_in_folder: true, download_id: downloadId }
}

function requireDownloads(chromeApi: ChromeApi): NonNullable<ChromeApi['downloads']> {
  if (!chromeApi.downloads) {
    throw new Error('Chrome downloads API is unavailable; enable the downloads permission')
  }
  return chromeApi.downloads
}

function downloadSummary(item: ChromeDownloadItem): Record<string, unknown> {
  return {
    id: item.id,
    url: item.url || '',
    final_url: item.finalUrl || '',
    filename: item.filename || '',
    state: item.state || '',
    paused: Boolean(item.paused),
    error: item.error || null,
    bytes_received: item.bytesReceived || 0,
    total_bytes: item.totalBytes || 0,
    start_time: item.startTime || null,
    end_time: item.endTime || null,
    exists: item.exists ?? null
  }
}

function waitForDownloadComplete(
  downloads: NonNullable<ChromeApi['downloads']>,
  downloadId: number,
  timeout: number
): Promise<ChromeDownloadItem> {
  const startedAt = Date.now()

  return new Promise((resolve, reject) => {
    const poll = async () => {
      try {
        const [item] = await downloads.search({ id: downloadId, limit: 1 })
        if (item?.state === 'complete') {
          resolve(item)
          return
        }
        if (item?.state === 'interrupted') {
          reject(new Error(`download interrupted: ${item.error || downloadId}`))
          return
        }
        if (Date.now() - startedAt >= timeout) {
          reject(new Error(`download ${downloadId} did not complete before timeout: ${timeout}ms`))
          return
        }
        setTimeout(poll, 250)
      } catch (error) {
        reject(error instanceof Error ? error : new Error(String(error)))
      }
    }

    void poll()
  })
}
