import {
  actionTimeoutMs,
  activeTab,
  normalizeOptionalText,
  positiveInteger,
  requirePositiveInteger,
  tabSummary
} from './browser-tabs'
import type {
  BrowserActionArgs,
  BrowserActionResult,
  ChromeApi,
  ChromeCookieInfo,
  ChromeDownloadItem
} from './types'

/**
 * Actions answered from browser state rather than from a page: the tab list,
 * the download manager, the cookie jar, and the caches.
 *
 * `downloads` and `cookies` are optional Chrome APIs, so each entry point
 * asserts its API is present and fails with a message naming the missing
 * permission instead of throwing on an undefined property.
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
  await downloads.open(downloadId)
  return { opened: true, download_id: downloadId }
}

export async function getCookies(
  chromeApi: ChromeApi,
  args: BrowserActionArgs
): Promise<BrowserActionResult> {
  const cookies = requireCookies(chromeApi)
  const domain = normalizeOptionalText(args.domain)
  const name = normalizeOptionalText(args.name)
  const storeId = normalizeOptionalText(args.store_id)
  const details: { url?: string; domain?: string; name?: string; storeId?: string } = {}
  if (domain) {
    details.domain = domain
  } else {
    details.url = await cookieUrl(chromeApi, args)
  }
  if (name) {
    details.name = name
  }
  if (storeId) {
    details.storeId = storeId
  }
  const found = await cookies.getAll(details)
  return { cookies: found.map(cookieSummary), count: found.length }
}

export async function setCookie(
  chromeApi: ChromeApi,
  args: BrowserActionArgs
): Promise<BrowserActionResult> {
  const cookies = requireCookies(chromeApi)
  const name = normalizeOptionalText(args.name)
  if (!name) {
    throw new Error('set_cookie requires name')
  }
  const value = typeof args.value === 'string' ? args.value : ''
  const details = {
    url: await cookieUrl(chromeApi, args),
    name,
    value,
    domain: normalizeOptionalText(args.domain),
    path: normalizeOptionalText(args.path),
    secure: args.secure,
    httpOnly: args.http_only,
    sameSite: args.same_site,
    expirationDate: typeof args.expiration_date === 'number' ? args.expiration_date : undefined,
    storeId: normalizeOptionalText(args.store_id)
  }
  const cookie = await cookies.set(details)
  return { set: true, cookie: cookieSummary(cookie) }
}

export async function deleteCookie(
  chromeApi: ChromeApi,
  args: BrowserActionArgs
): Promise<BrowserActionResult> {
  const cookies = requireCookies(chromeApi)
  const name = normalizeOptionalText(args.name)
  if (!name) {
    throw new Error('delete_cookie requires name')
  }
  const removed = await cookies.remove({
    url: await cookieUrl(chromeApi, args),
    name,
    storeId: normalizeOptionalText(args.store_id)
  })
  return { deleted: Boolean(removed), cookie: removed || null }
}

export async function clearBrowserCache(
  chromeApi: ChromeApi,
  args: BrowserActionArgs
): Promise<BrowserActionResult> {
  if (!chromeApi.browsingData) {
    throw new Error('Chrome browsingData API is unavailable; enable the browsingData permission')
  }
  const origins = normalizedOrigins(args)
  const since =
    typeof args.since_ms === 'number' && Number.isFinite(args.since_ms)
      ? Math.max(0, Math.floor(args.since_ms))
      : 0
  await chromeApi.browsingData.remove(
    { since, origins: origins.length ? origins : undefined },
    {
      cache: true,
      cacheStorage: true,
      indexedDB: true,
      localStorage: true,
      serviceWorkers: true
    }
  )
  return { cleared: true, origins, since_ms: since }
}

function requireDownloads(chromeApi: ChromeApi): NonNullable<ChromeApi['downloads']> {
  if (!chromeApi.downloads) {
    throw new Error('Chrome downloads API is unavailable; enable the downloads permission')
  }
  return chromeApi.downloads
}

function requireCookies(chromeApi: ChromeApi): NonNullable<ChromeApi['cookies']> {
  if (!chromeApi.cookies) {
    throw new Error('Chrome cookies API is unavailable; enable the cookies permission')
  }
  return chromeApi.cookies
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

function cookieSummary(
  cookie: ChromeCookieInfo | null | undefined
): Record<string, unknown> | null {
  if (!cookie) {
    return null
  }
  return {
    name: cookie.name || '',
    value: cookie.value || '',
    domain: cookie.domain || '',
    path: cookie.path || '',
    secure: Boolean(cookie.secure),
    http_only: Boolean(cookie.httpOnly),
    same_site: cookie.sameSite || null,
    expiration_date: cookie.expirationDate || null,
    session: Boolean(cookie.session),
    store_id: cookie.storeId || null
  }
}

async function cookieUrl(chromeApi: ChromeApi, args: BrowserActionArgs): Promise<string> {
  const explicit = normalizeOptionalText(args.url)
  if (explicit) {
    return explicit
  }
  const tab = await activeTab(chromeApi)
  const url = normalizeOptionalText(tab?.url)
  if (!url) {
    throw new Error('cookie action requires url or an active tab with a URL')
  }
  return url
}

function normalizedOrigins(args: BrowserActionArgs): string[] {
  const origins = Array.isArray(args.origins)
    ? args.origins
        .filter((origin) => typeof origin === 'string' && origin.trim())
        .map((origin) => origin.trim())
    : []
  const url = normalizeOptionalText(args.url)
  if (!url) {
    return origins
  }
  try {
    origins.push(new URL(url).origin)
  } catch (_error) {
    origins.push(url)
  }
  return Array.from(new Set(origins))
}
