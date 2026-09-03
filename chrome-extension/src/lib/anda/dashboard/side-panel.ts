/**
 * Opening the Anda side panel from a dashboard tab.
 *
 * `chrome.sidePanel.open` needs a user gesture and a target, and which target is
 * available depends on where the dashboard is running: a real tab has a tab id,
 * a detached window only has a window id, and neither is available when the
 * dashboard is itself rendered inside the panel. Each is tried in turn, and the
 * panel page opens as an ordinary tab if none of them work.
 */
export async function openAndaSidePanel(): Promise<void> {
  if (chrome.sidePanel?.open) {
    try {
      const tab = await chrome.tabs.getCurrent()
      if (typeof tab?.id === 'number') {
        await chrome.sidePanel.open({ tabId: tab.id })
        return
      }
      if (typeof tab?.windowId === 'number') {
        await chrome.sidePanel.open({ windowId: tab.windowId })
        return
      }
    } catch (_error) {
      try {
        const currentWindow = await chrome.windows.getCurrent()
        if (typeof currentWindow?.id === 'number') {
          await chrome.sidePanel.open({ windowId: currentWindow.id })
          return
        }
      } catch (_fallbackError) {
        // Fall through to opening the side panel page as a tab below.
      }
    }
  }

  const url = chrome.runtime.getURL('index.html')
  chrome.tabs.create({ url, active: true }).catch(() => {
    window.open(url, '_blank', 'noopener,noreferrer')
  })
}
