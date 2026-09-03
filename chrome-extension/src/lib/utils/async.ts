/** Resolves after `ms`. Shared by the side panel and the service worker. */
export function delay(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms))
}
