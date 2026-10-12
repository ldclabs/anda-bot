import type { BrowserWindow, WebContents } from 'electron'
import type { NativeEvent } from '../shared/contract'
import type { PanelContext, PanelEdge, PanelTab, PanelWindowEvent } from '../shared/panels'

/**
 * The workbench panels the user moved into their own windows, one window per
 * panel. They follow the chat open in the main window, whose renderer pushes
 * it here as a PanelContext; closing a window closes its panel, and a panel's
 * "return to main window" closes it with a place in the canvas to go back to.
 */
export class PanelWindows {
  private windows = new Map<PanelTab, BrowserWindow>()
  private docking = new Map<PanelTab, PanelEdge | 'canvas'>()
  context: PanelContext | null = null
  constructor(
    private create: (tab: PanelTab, at?: { x: number; y: number }) => BrowserWindow,
    /** Tells the main window's renderer. */
    private changed: (event: PanelWindowEvent) => void,
    /** Runs while a closing window still holds its native views. */
    private closing: (window: BrowserWindow) => void
  ) {}
  get tabs(): PanelTab[] {
    return [...this.windows.keys()]
  }
  get all(): BrowserWindow[] {
    return [...this.windows.values()].filter((window) => !window.isDestroyed())
  }
  tabOf(contents: WebContents): PanelTab | undefined {
    for (const [tab, window] of this.windows)
      if (!window.isDestroyed() && window.webContents === contents) return tab
    return undefined
  }
  open(tab: PanelTab, focus: boolean, at?: { x: number; y: number }): void {
    const existing = this.windows.get(tab)
    if (existing && !existing.isDestroyed()) {
      if (existing.isMinimized()) existing.restore()
      if (focus) {
        existing.show()
        existing.focus()
      } else if (!existing.isVisible()) existing.showInactive()
      return
    }
    const window = this.create(tab, at)
    this.windows.set(tab, window)
    window.once('ready-to-show', () => (focus ? window.show() : window.showInactive()))
    window.on('close', () => this.closing(window))
    window.on('closed', () => {
      if (this.windows.get(tab) === window) this.windows.delete(tab)
      const dock = this.docking.get(tab)
      this.docking.delete(tab)
      this.changed({ tab, open: false, ...(dock ? { dock } : {}) })
    })
    this.changed({ tab, open: true })
  }
  close(tab: PanelTab): void {
    this.windows.get(tab)?.close()
  }
  dock(tab: PanelTab, dock: PanelEdge | 'canvas'): void {
    if (!this.windows.has(tab)) return
    this.docking.set(tab, dock)
    this.close(tab)
  }
  setContext(context: PanelContext): void {
    this.context = context
    this.send({ type: 'panel-context', value: context })
  }
  send(event: NativeEvent): void {
    for (const window of this.all) window.webContents.send('anda:event', event)
  }
}
