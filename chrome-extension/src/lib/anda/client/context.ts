import { getContext, setContext } from 'svelte'
import type { AndaSidePanelClient } from './side-panel.svelte'

/** The UI contract shared by the extension and the desktop client. */
export type UiClient = Pick<
  AndaSidePanelClient,
  | 'init'
  | 'sending'
  | 'activeChannel'
  | 'status'
  | 'skills'
  | 'bookmarks'
  | 'respondAction'
  | 'loadResource'
> & { readonly readOnly?: boolean }

const clientContext = Symbol('anda-client')

export function provideAndaClient(client: UiClient): void {
  setContext(clientContext, client)
}

export function useAndaClient(): UiClient {
  const client = getContext<UiClient>(clientContext)
  if (!client) throw new Error('Anda client context is missing')
  return client
}
