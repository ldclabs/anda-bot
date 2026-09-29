import { app, dialog } from 'electron'
import { autoUpdater } from 'electron-updater'
import { readFile } from 'node:fs/promises'
import { join } from 'node:path'
import { DaemonClient, downloadedRelease, type RuntimeUpdateState } from './daemon-client'
import { DesktopStore } from './store'
import { installRuntimeUpdate } from './update-machine'

const runtimeCheckInterval = 6 * 3600_000

/**
 * Two independent updates. The shared `anda` runtime updates through its own
 * release channel (the CLI updater), draining active work first; the desktop
 * application updates through electron-updater and never touches the daemon,
 * whose executable lives outside the app.
 */
export class DesktopUpdater {
  installing = false
  runtime?: RuntimeUpdateState
  private busy = false
  private downloaded?: string
  private timer?: NodeJS.Timeout
  constructor(
    private daemon: DaemonClient,
    private store: DesktopStore,
    private runningTerminals: () => number,
    private emit: (message: string) => void,
    private changed: () => void
  ) {
    autoUpdater.autoDownload = false
    autoUpdater.autoInstallOnAppQuit = false
    autoUpdater.allowPrerelease = false
    autoUpdater.on('download-progress', (progress) =>
      emit(`Downloading update: ${Math.round(progress.percent)}%`)
    )
    autoUpdater.on('error', (error) => {
      this.installing = false
      emit(`Update failed: ${error.message}`)
    })
  }
  /** The runtime release that is downloaded and ready to install, if any. */
  get runtimeRelease(): string | null {
    return downloadedRelease(this.runtime)
  }
  /** Checks for runtime releases now and then at the updater's interval. */
  startRuntimeChecks(): void {
    const tick = () => {
      if (!this.daemon.manuallyStopped && !this.busy) void this.checkRuntime(false).catch(() => {})
      this.timer = setTimeout(tick, runtimeCheckInterval)
    }
    this.timer = setTimeout(tick, 60_000)
  }
  stop(): void {
    clearTimeout(this.timer)
  }
  private async checkRuntime(force: boolean): Promise<RuntimeUpdateState> {
    this.runtime = await this.daemon.runtimeUpdateState(force)
    this.changed()
    return this.runtime
  }
  /** Settings and the tray: install a ready runtime, else check both channels. */
  async check(): Promise<string> {
    if (this.busy) return 'An update operation is already in progress.'
    this.busy = true
    try {
      const runtime = await this.checkRuntime(true)
      const release = downloadedRelease(runtime)
      if (release) return await this.promptRuntime(release)
      const runtimeMessage = runtime.error
        ? `Anda runtime: ${runtime.error}`
        : `Anda runtime ${runtime.current_tag} is up to date.`
      return `${runtimeMessage} ${await this.checkApp()}`
    } finally {
      this.busy = false
    }
  }
  /** The tray's install action for an already downloaded runtime release. */
  async installRuntime(): Promise<string> {
    const release = this.runtimeRelease
    if (!release) return this.check()
    if (this.busy) return 'An update operation is already in progress.'
    this.busy = true
    try {
      return await this.promptRuntime(release)
    } finally {
      this.busy = false
    }
  }
  private async promptRuntime(release: string): Promise<string> {
    const choice = await dialog.showMessageBox({
      type: 'question',
      message: `Install Anda ${release} and restart the service?`,
      detail: 'New tasks pause while active tasks finish, then the service restarts.',
      buttons: ['Later', 'Install and restart'],
      defaultId: 1,
      cancelId: 0
    })
    if (choice.response !== 1) return 'The update stays downloaded. Install it when you are ready.'
    const daemon = this.daemon
    if (!daemon.view.connected && !daemon.manuallyStopped) await daemon.connect()
    const running = daemon.view.connected
    const windows = process.platform === 'win32'
    this.emit('Waiting for active tasks to finish…')
    await installRuntimeUpdate({
      running,
      begin: () => daemon.maintenance('begin'),
      renew: (token) => daemon.maintenance('renew', token),
      release: async (token) => {
        await daemon.maintenance('release', token)
      },
      install: async (lease) => {
        // A running executable can only be replaced on Windows once it stops.
        if (windows && lease) await daemon.stopForUpdate(lease)
        await daemon.applyRuntimeUpdate()
        if (running) await daemon.startRuntime(!windows)
      },
      recover: async (lease) => {
        if (daemon.view.connected && lease) await daemon.maintenance('release', lease)
        else if (running) await daemon.startRuntime(false)
      },
      wait: () => new Promise((resolve) => setTimeout(resolve, 5000))
    })
    this.runtime = await daemon.runtimeUpdateState(false).catch(() => undefined)
    this.changed()
    return `Anda ${release} is installed.`
  }
  private async checkApp(): Promise<string> {
    if (!app.isPackaged) return 'Desktop updates are available only in release builds.'
    let configured = false
    try {
      configured =
        JSON.parse(await readFile(join(process.resourcesPath, 'release-channel.json'), 'utf8'))
          .signed === true
    } catch {}
    if (!configured)
      return 'This desktop build has no signed update channel; install the next package manually.'
    if (!this.downloaded) {
      const result = await autoUpdater.checkForUpdates()
      if (!result?.isUpdateAvailable) return 'Anda Desktop is up to date.'
      const choice = await dialog.showMessageBox({
        type: 'question',
        message: `Download Anda Desktop ${result.updateInfo.version}?`,
        buttons: ['Later', 'Download'],
        defaultId: 1,
        cancelId: 0
      })
      if (choice.response !== 1) return 'Desktop update available; download postponed.'
      await autoUpdater.downloadUpdate()
      this.downloaded = result.updateInfo.version
    }
    if (this.runningTerminals())
      return 'Desktop update downloaded. Close your terminal sessions before installing.'
    const choice = await dialog.showMessageBox({
      type: 'question',
      message: `Install Anda Desktop ${this.downloaded} and restart it?`,
      detail: 'The Anda service keeps running; only the desktop app restarts.',
      buttons: ['Later', 'Install and restart'],
      defaultId: 0,
      cancelId: 0
    })
    if (choice.response !== 1)
      return 'Desktop update downloaded. Choose Check for updates when ready to install.'
    this.installing = true
    this.store.state.updateIntent = {
      previous: app.getVersion(),
      target: this.downloaded,
      startedAt: Date.now()
    }
    await this.store.save()
    autoUpdater.quitAndInstall(false, true)
    return 'Installing update…'
  }
  /** Reports the result of a desktop update that restarted the app. */
  async recover(): Promise<void> {
    const intent = this.store.state.updateIntent
    if (!intent) return
    this.emit(
      app.getVersion() === intent.target
        ? `Updated Anda Desktop to ${intent.target}.`
        : 'The previous desktop update did not finish; your previous version is preserved.'
    )
    delete this.store.state.updateIntent
    await this.store.save()
  }
}
