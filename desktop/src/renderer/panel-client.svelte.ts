import { ResourceCache } from '$lib/anda/client/resources'
import { errorToError } from '$lib/service-worker/settings'
import { normalizeUiLanguage } from '$lib/i18n'
import type { DaemonApi } from '$lib/anda/client/daemon'
import type { ChatAttachment, Resource, ToolOutput } from '$lib/anda/client/types'
import { defaultPreferences, type Preferences } from '../shared/contract'
import type { PanelContext, PanelEdge, PanelTab, PanelWindowState } from '../shared/panels'
import type { PanelHost } from './panel-host'

/**
 * A panel window's client. The window shows one workbench panel for the chat
 * open in the main window, which sends that chat here; preferences changed
 * in either window reach the other through Main.
 */
export class PanelClient implements PanelHost, DaemonApi {
  ready = $state(false)
  tab = $state<PanelTab>('resources')
  preferences = $state<Preferences>(structuredClone(defaultPreferences))
  platform = $state('darwin')
  context = $state<PanelContext<ChatAttachment>>({
    source: '',
    branch: '',
    resources: [],
    sessionFiles: [],
    fileRequest: null,
    changeFocus: null
  })
  systemMessage = $state<string | null>(null)
  /** Main rejects daemon calls while it is not connected. */
  readonly authorized = true
  private readonly resources = new ResourceCache(this)
  private unsubscribe?: () => void
  // Derived, so each context Main sends does not count as a chat switch: the
  // panel clears its preview only when the chat or folder really changes.
  activeSource = $derived(this.context.source)
  workspace = $derived(this.context.workspace)
  get language(): string {
    return normalizeUiLanguage(this.preferences.language || navigator.language) || 'en'
  }
  async init(): Promise<void> {
    this.unsubscribe = window.anda.onEvent((event) => {
      if (event.type === 'panel-context') this.context = event.value as typeof this.context
      else if (event.type === 'preferences')
        this.preferences = { ...this.preferences, ...(event.value as Partial<Preferences>) }
      else if (event.type === 'fullscreen')
        document.documentElement.toggleAttribute('data-fullscreen', event.value === true)
    })
    const state = await window.anda.panel<PanelWindowState>({ action: 'state' })
    this.tab = state.tab
    this.preferences = state.preferences
    this.platform = state.platform
    if (state.context) this.context = state.context as typeof this.context
    document.documentElement.dataset.platform = state.platform
    document.documentElement.toggleAttribute('data-fullscreen', state.fullScreen)
    this.ready = true
  }
  /** Back onto the main window's canvas: along `edge`, or where panels open. */
  dock(edge?: PanelEdge): void {
    void window.anda
      .panel({ action: 'dock', tab: this.tab, ...(edge ? { edge } : {}) })
      .catch((error) => this.fail(error))
  }
  loadResource(resource: Resource): Promise<Resource | null> {
    return this.resources.load(resource)
  }
  rpc<Result>(method: string, params: unknown[]): Promise<Result> {
    return window.anda.rpc<Result>(method, $state.snapshot(params))
  }
  async toolCall<Result>(
    name: string,
    args: Record<string, unknown>,
    resources: Resource[] = [],
    meta?: Record<string, unknown>
  ): Promise<ToolOutput<Result>> {
    const result = await this.rpc<ToolOutput<Result>>('tool_call', [
      { name, args, resources, ...(meta ? { meta } : {}) }
    ])
    const error = (result.output as { error?: unknown })?.error
    if (error != null) throw errorToError(error)
    return result
  }
  async savePreferences(patch: Partial<Preferences>): Promise<void> {
    this.preferences = { ...this.preferences, ...patch }
    await window.anda.preferences($state.snapshot(patch))
  }
  fail(error: unknown): void {
    this.systemMessage = error instanceof Error ? error.message : String(error)
  }
  dispose(): void {
    this.unsubscribe?.()
  }
}
