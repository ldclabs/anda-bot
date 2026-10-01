import { app, dialog } from 'electron'
import { autoUpdater } from 'electron-updater'
import { readFile } from 'node:fs/promises'
import { join } from 'node:path'
import { DaemonClient, downloadedRelease, type RuntimeUpdateState } from './daemon-client'
import { DesktopStore } from './store'
import { installRuntimeUpdate } from './update-machine'
import type { UpdateStatus } from '../shared/contract'
import { label, type Label } from '../renderer/labels'

const updateCheckInterval = 6 * 3600_000

/**
 * Two independent updates. The shared `anda` runtime updates through its own
 * release channel (the CLI updater), draining active work first; the desktop
 * application updates through electron-updater and never touches the daemon,
 * whose executable lives outside the app.
 */
export class DesktopUpdater {
  installing = false
  runtime?: RuntimeUpdateState
  status: UpdateStatus | null = null
  private busy = false
  private downloaded?: string
  private availableApp?: string
  private notifiedApp = new Set<string>()
  private backgroundCheck?: Promise<void>
  private automaticChecksStarted = false
  private timer?: NodeJS.Timeout
  constructor(
    private daemon: DaemonClient,
    private store: DesktopStore,
    private runningTerminals: () => number,
    private emit: (message: string) => void,
    private changed: () => void,
    private statusChanged: (status: UpdateStatus) => void = () => {}
  ) {
    autoUpdater.autoDownload = false
    autoUpdater.autoInstallOnAppQuit = false
    autoUpdater.allowPrerelease = false
    autoUpdater.on('download-progress', (progress) =>
      this.progress(`Downloading update: ${Math.round(progress.percent)}%`)
    )
    autoUpdater.on('error', (error) => {
      this.installing = false
      // Background failures stay quiet; a running operation reports its own error.
      if (this.backgroundCheck || this.busy) return
      // Asynchronous failures, such as quitAndInstall, arrive after the operation finished.
      this.setStatus({ phase: 'error', message: error.message })
      emit(`Update failed: ${error.message}`)
    })
  }
  private t(key: Label): string {
    return label(this.store.state.preferences?.language || 'en', key)
  }
  private progress(message: string): void {
    this.setStatus({ phase: 'running', message })
  }
  private setStatus(status: UpdateStatus): void {
    this.status = status
    this.statusChanged(status)
  }
  private async run(action: () => Promise<{ message: string; failed?: boolean }>): Promise<string> {
    if (this.busy) return 'An update operation is already in progress.'
    this.busy = true
    this.progress(this.t('checkingUpdates'))
    try {
      // A user can open the dialog while the scheduled check is still running.
      // Finish that check before starting another check or an installation.
      if (this.backgroundCheck) await this.backgroundCheck
      const { message, failed } = await action()
      this.setStatus({ phase: failed ? 'error' : 'complete', message })
      return message
    } catch (error) {
      this.setStatus({
        phase: 'error',
        message: error instanceof Error ? error.message : String(error)
      })
      throw error
    } finally {
      this.busy = false
    }
  }
  /** The runtime release that is downloaded and ready to install, if any. */
  get runtimeRelease(): string | null {
    return downloadedRelease(this.runtime)
  }
  get desktopRelease(): string | null {
    return this.downloaded || this.availableApp || null
  }
  /** Checks both release channels quietly, starting one minute after launch. */
  startAutomaticChecks(): void {
    if (this.automaticChecksStarted) return
    this.automaticChecksStarted = true
    const tick = async () => {
      try {
        if (!this.busy && !this.installing) {
          // Neither a stopped runtime nor a failed runtime check blocks desktop updates.
          this.backgroundCheck = Promise.allSettled([
            this.daemon.manuallyStopped ? Promise.resolve() : this.checkRuntime(false),
            this.checkAppAutomatically()
          ]).then(() => {})
          await this.backgroundCheck
        }
      } finally {
        this.backgroundCheck = undefined
        if (this.automaticChecksStarted) this.timer = setTimeout(tick, updateCheckInterval)
      }
    }
    this.timer = setTimeout(tick, 60_000)
  }
  stop(): void {
    this.automaticChecksStarted = false
    clearTimeout(this.timer)
  }
  private async checkAppAutomatically(): Promise<void> {
    if (this.downloaded || (await this.appUpdateUnavailable())) return
    const result = await this.findAppUpdate()
    if (!this.automaticChecksStarted || !result.isUpdateAvailable) return
    const version = result.updateInfo.version
    if (this.notifiedApp.has(version)) return
    this.notifiedApp.add(version)
    this.emit(this.t('desktopUpdateAvailable').replace('{version}', version))
  }
  private async checkRuntime(force: boolean): Promise<RuntimeUpdateState> {
    this.runtime = await this.daemon.runtimeUpdateState(force)
    this.changed()
    return this.runtime
  }
  /** Settings and the tray: handle both channels, even if a runtime update fails. */
  async check(): Promise<string> {
    return this.run(async () => {
      const messages: string[] = []
      let failed = false
      this.progress(this.t('checkingRuntime'))
      try {
        const runtime = await this.checkRuntime(true)
        const release = downloadedRelease(runtime)
        if (release) messages.push(await this.promptRuntime(release))
        else if (runtime.error) {
          failed = true
          messages.push(`Anda runtime: ${runtime.error}`)
        } else messages.push(this.t('runtimeUpToDate').replace('{version}', runtime.current_tag))
      } catch (error) {
        failed = true
        messages.push(`Anda runtime: ${error instanceof Error ? error.message : String(error)}`)
      }
      // Installing or postponing the runtime must not skip the desktop release.
      // Keep its result visible alongside any failure from the other channel.
      this.progress(this.t('checkingDesktop'))
      try {
        messages.push(await this.checkApp())
      } catch (error) {
        failed = true
        messages.push(`Anda Desktop: ${error instanceof Error ? error.message : String(error)}`)
      }
      return { message: messages.join('\n\n'), failed }
    })
  }
  /** The tray's install action for an already downloaded runtime release. */
  async installRuntime(): Promise<string> {
    const release = this.runtimeRelease
    if (!release) return this.check()
    return this.run(async () => ({ message: await this.promptRuntime(release) }))
  }
  /** Opens a known desktop update independently of the runtime's update state. */
  async checkDesktop(): Promise<string> {
    return this.run(async () => {
      this.progress(this.t('checkingDesktop'))
      return { message: await this.checkApp() }
    })
  }
  private async promptRuntime(release: string): Promise<string> {
    this.progress(`Anda ${release} is ready to install.`)
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
    this.progress('Waiting for active tasks to finish…')
    await installRuntimeUpdate({
      running,
      begin: () => daemon.maintenance('begin'),
      renew: (token) => daemon.maintenance('renew', token),
      release: async (token) => {
        await daemon.maintenance('release', token)
      },
      install: async (lease) => {
        this.progress(`Installing Anda ${release}…`)
        // Stop while the lease is valid: CLI downloads can outlast its 90 seconds.
        if (lease) await daemon.stopForUpdate(lease)
        await daemon.applyRuntimeUpdate()
        if (running) await daemon.startRuntime()
      },
      recover: async (lease) => {
        if (daemon.view.connected && lease) {
          await daemon.maintenance('release', lease)
          daemon.manuallyStopped = false
        } else if (running) await daemon.startRuntime()
      },
      wait: () => new Promise((resolve) => setTimeout(resolve, 5000))
    })
    this.runtime = await daemon.runtimeUpdateState(false).catch(() => undefined)
    this.changed()
    return `Anda ${release} is installed.`
  }
  private async appUpdateUnavailable(): Promise<string | null> {
    if (!app.isPackaged) return this.t('desktopUpdatesReleaseOnly')
    let configured = false
    try {
      configured =
        JSON.parse(await readFile(join(process.resourcesPath, 'release-channel.json'), 'utf8'))
          .signed === true
    } catch {}
    return configured ? null : this.t('desktopUpdatesManual')
  }
  private async findAppUpdate() {
    const result = await autoUpdater.checkForUpdates()
    if (!result) throw new Error(this.t('desktopUpdateCheckUnavailable'))
    this.availableApp = result.isUpdateAvailable ? result.updateInfo.version : undefined
    this.changed()
    return result
  }
  private async checkApp(): Promise<string> {
    const unavailable = await this.appUpdateUnavailable()
    if (unavailable) return unavailable
    if (!this.downloaded) {
      const result = await this.findAppUpdate()
      if (!result.isUpdateAvailable) return this.t('desktopUpToDate')
      this.progress(`Anda Desktop ${result.updateInfo.version} is available.`)
      const choice = await dialog.showMessageBox({
        type: 'question',
        message: `Download Anda Desktop ${result.updateInfo.version}?`,
        buttons: ['Later', 'Download'],
        defaultId: 1,
        cancelId: 0
      })
      if (choice.response !== 1) return 'Desktop update available; download postponed.'
      this.progress(`Downloading Anda Desktop ${result.updateInfo.version}…`)
      await autoUpdater.downloadUpdate()
      this.downloaded = result.updateInfo.version
    }
    this.progress(`Anda Desktop ${this.downloaded} is ready to install.`)
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
    this.progress('Installing Anda Desktop…')
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
