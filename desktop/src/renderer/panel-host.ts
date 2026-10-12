import type { Resource } from '$lib/anda/client/types'
import type { Preferences } from '../shared/contract'

/**
 * What a workbench panel needs from its window: the main window's client, or
 * a panel window's, which follows the chat open in the main window.
 */
export interface PanelHost {
  readonly platform: string
  readonly preferences: Preferences
  readonly activeSource: string
  readonly workspace: string | undefined
  loadResource(resource: Resource): Promise<Resource | null>
  rpc<Result>(method: string, params: unknown[]): Promise<Result>
  savePreferences(patch: Partial<Preferences>): Promise<void>
  fail(error: unknown): void
}
