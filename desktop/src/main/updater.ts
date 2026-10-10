import { app, dialog } from 'electron'
import { autoUpdater } from 'electron-updater'
import { readFile } from 'node:fs/promises'
import { join } from 'node:path'
import { DaemonClient, downloadedRelease, type RuntimeUpdateState } from './daemon-client'
import { DesktopStore } from './store'
import { installRuntimeUpdate, isOlderRelease } from './update-machine'
import type { UpdateOffer, UpdateOperation, UpdateStatus } from '../shared/contract'
import { label, updateOperationLabels, type Label } from '../renderer/labels'

const updateCheckInterval = 6 * 3600_000

/**
 * The desktop release to offer, or a final `message` when there is none.
 * `runtime`: installing it also updates the shared `anda`.
 */
type AppRelease = { message: string } | { version: string; downloaded: boolean; runtime: boolean }

/**
 * Two update channels from one GitHub release. The shared `anda` runtime
 * updates through its own release channel (the CLI updater), draining active
 * work first; the desktop application updates through electron-updater, and
 * its executable lives outside the daemon's. The desktop package bundles the
 * same release's `anda`, which the app installs on start, so when the app owns
 * the shared runtime a desktop update brings both: one download, one
 * confirmation, and a drained service that the restarted app starts again.
 */
export class DesktopUpdater {
  installing = false
  runtime?: RuntimeUpdateState
  status: UpdateStatus | null = null
  private busy = false
  /** The operation the status reports, kept for errors that arrive after it finished. */
  private operation: UpdateOperation = 'check'
  private downloaded?: string
  private availableApp?: string
  private notifiedApp = new Set<string>()
  private backgroundCheck?: Promise<void>
  private automaticChecksStarted = false
  /** The service was stopped for an app installation that has not quit yet. */
  private serviceStopped = false
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
      // A failed installation reports an error instead of quitting the app.
      if (this.serviceStopped) {
        this.serviceStopped = false
        void this.daemon.startRuntime().catch(() => {})
      }
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
  private setStatus(status: Omit<UpdateStatus, 'operation'>): void {
    this.status = { ...status, operation: this.operation }
    this.statusChanged(this.status)
  }
  private async run(
    operation: UpdateOperation,
    action: () => Promise<{ message: string; failed?: boolean }>
  ): Promise<string> {
    if (this.busy) return 'An update operation is already in progress.'
    this.busy = true
    this.operation = operation
    this.progress(this.t(updateOperationLabels[operation].progress))
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
  /** The downloaded runtime release to install, unless the desktop release brings it. */
  get runtimeRelease(): string | null {
    const release = downloadedRelease(this.runtime)
    const desktop = this.desktopRelease
    return release && desktop && this.bringsRuntime(desktop, release) ? null : release
  }
  get desktopRelease(): string | null {
    return this.downloaded || this.availableApp || null
  }
  /**
   * The status bar's step: download a found release, or restart into a
   * downloaded one. A runtime kept by a failed check is left to the tray:
   * installing it needs the release server, and this step asks nothing.
   */
  get offer(): UpdateOffer | null {
    const ready = this.downloaded || (!this.runtime?.error && this.runtimeRelease)
    if (ready) return { version: ready, ready: true }
    return this.availableApp ? { version: this.availableApp, ready: false } : null
  }
  /** Whether installing desktop `version` also brings the shared runtime to `runtime`. */
  private bringsRuntime(version: string, runtime = this.runtime?.latest_tag): boolean {
    return this.daemon.bundleOwnsRuntime && !(runtime && isOlderRelease(version, runtime))
  }
  /** Checks both release channels quietly, starting one minute after launch. */
  startAutomaticChecks(): void {
    if (this.automaticChecksStarted) return
    this.automaticChecksStarted = true
    const tick = async () => {
      try {
        if (!this.busy && !this.installing) {
          this.backgroundCheck = this.checkAutomatically()
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
  private async checkAutomatically(): Promise<void> {
    // Neither a stopped runtime nor a failed runtime check blocks desktop updates.
    const desktop = await this.checkAppAutomatically().catch(() => null)
    if (!this.automaticChecksStarted || this.daemon.manuallyStopped) return
    // The desktop release carries this runtime; do not download it twice.
    if (desktop && 'version' in desktop && desktop.runtime) return
    await this.checkRuntime(false).catch(() => {})
  }
  private async checkAppAutomatically(): Promise<AppRelease> {
    const release = await this.findApp()
    if (
      !('version' in release) ||
      release.downloaded ||
      !this.automaticChecksStarted ||
      this.notifiedApp.has(release.version)
    )
      return release
    this.notifiedApp.add(release.version)
    this.emit(this.t('desktopUpdateAvailable').replace('{version}', release.version))
    return release
  }
  private async checkRuntime(force: boolean): Promise<RuntimeUpdateState> {
    this.runtime = await this.daemon.runtimeUpdateState(force)
    this.changed()
    return this.runtime
  }
  /** Settings and the tray: handle both channels, even if a runtime update fails. */
  async check(): Promise<string> {
    return this.run('check', async () => {
      const messages: string[] = []
      let failed = false
      const fail = (key: 'runtimeUpdateError' | 'desktopUpdateError', error: unknown) => {
        failed = true
        const detail = error instanceof Error ? error.message : String(error)
        messages.push(this.t(key).replace('{error}', detail))
      }
      // Look up the desktop release before the runtime check downloads anything:
      // when it carries the runtime, the whole update is that one package.
      let desktop: AppRelease | undefined
      let desktopError: unknown
      try {
        desktop = await this.findApp()
      } catch (error) {
        desktopError = error
      }
      if (desktop && 'version' in desktop && desktop.runtime)
        return { message: await this.installApp(desktop) }
      this.progress(this.t('checkingRuntime'))
      try {
        const runtime = await this.checkRuntime(true)
        const release = downloadedRelease(runtime)
        if (runtime.error) {
          fail('runtimeUpdateError', runtime.error)
          // A failed check keeps a verified download in the tray. Installing
          // needs the release server too, so do not stop the service for it now.
          if (release) messages.push(this.t('runtimeUpdateKept').replace('{version}', release))
        } else if (release) messages.push(await this.promptRuntime(release))
        else messages.push(this.t('runtimeUpToDate').replace('{version}', runtime.current_tag))
      } catch (error) {
        fail('runtimeUpdateError', error)
      }
      // Installing or postponing the runtime must not skip the desktop release.
      // Keep its result visible alongside any failure from the other channel.
      this.progress(this.t('checkingDesktop'))
      try {
        if (!desktop) throw desktopError
        messages.push(await this.installApp(desktop))
      } catch (error) {
        fail('desktopUpdateError', error)
      }
      return { message: messages.join('\n\n'), failed }
    })
  }
  /** The tray's install action for an already downloaded runtime release. */
  async installRuntime(): Promise<string> {
    const release = this.runtimeRelease
    if (!release) return this.check()
    return this.run('install', async () => ({ message: await this.promptRuntime(release) }))
  }
  /** Takes the offered step. The click is the consent, so no dialog asks again. */
  async continueUpdate(): Promise<string> {
    const offer = this.offer
    if (!offer) return this.check()
    return this.run(offer.ready ? 'install' : 'download', async () => {
      const runtime = this.runtimeRelease
      if (offer.ready && !this.downloaded && runtime)
        return { message: await this.promptRuntime(runtime, true) }
      const release = await this.findApp()
      if (offer.ready || !('version' in release))
        return { message: await this.installApp(release, true) }
      if (!release.downloaded) await this.downloadApp(release.version)
      return { message: `Anda Desktop ${release.version} is downloaded. Restart to update.` }
    })
  }
  /** Opens a known desktop update independently of the runtime's update state. */
  async checkDesktop(): Promise<string> {
    return this.run('check', async () => {
      this.progress(this.t('checkingDesktop'))
      return { message: await this.installApp(await this.findApp()) }
    })
  }
  private async promptRuntime(release: string, confirmed = false): Promise<string> {
    this.progress(`Anda ${release} is ready to install.`)
    const choice = confirmed
      ? { response: 1 }
      : await dialog.showMessageBox({
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
    await this.drainService(release, async (running) => {
      await daemon.applyRuntimeUpdate()
      if (running) await daemon.startRuntime()
    })
    this.runtime = await daemon.runtimeUpdateState(false).catch(() => undefined)
    this.changed()
    return `Anda ${release} is installed.`
  }
  /**
   * Pauses new tasks and waits for active ones, stops a running service and
   * runs `install`, which receives whether it was running. A busy service or
   * a failed step leaves the service running.
   */
  private async drainService(
    release: string,
    install: (running: boolean) => Promise<void>
  ): Promise<void> {
    const daemon = this.daemon
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
        await install(running)
      },
      recover: async (lease) => {
        if (daemon.view.connected && lease) {
          await daemon.maintenance('release', lease)
          daemon.manuallyStopped = false
        } else if (running) await daemon.startRuntime()
      },
      wait: () => new Promise((resolve) => setTimeout(resolve, 5000))
    })
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
  private async findApp(): Promise<AppRelease> {
    const unavailable = await this.appUpdateUnavailable()
    if (unavailable) return { message: unavailable }
    let version = this.downloaded
    if (!version) {
      const result = await this.findAppUpdate()
      if (!result.isUpdateAvailable) return { message: this.t('desktopUpToDate') }
      version = result.updateInfo.version
    }
    // Resolving the runtime runs the bundle's `anda install`, which reports its owner.
    await this.daemon.discover().catch(() => {})
    return {
      version,
      downloaded: version === this.downloaded,
      runtime: this.bringsRuntime(version)
    }
  }
  private async downloadApp(version: string): Promise<void> {
    this.progress(`Downloading Anda Desktop ${version}…`)
    await autoUpdater.downloadUpdate()
    this.downloaded = version
    this.changed()
  }
  /** Downloads the desktop release, then installs it once the restart is confirmed. */
  private async installApp(release: AppRelease, confirmed = false): Promise<string> {
    if ('message' in release) return release.message
    const { version } = release
    // Checking for updates asked for this release; only the restart needs consent.
    if (!release.downloaded) await this.downloadApp(version)
    this.progress(`Anda Desktop ${version} is ready to install.`)
    if (this.runningTerminals())
      return 'Desktop update downloaded. Close your terminal sessions before installing.'
    const daemon = this.daemon
    // The restarted app installs the bundled runtime; a service on an older one restarts too.
    const restartService =
      release.runtime &&
      !daemon.manuallyStopped &&
      (!daemon.view.version || isOlderRelease(daemon.view.version, version))
    const choice = confirmed
      ? { response: 1 }
      : await dialog.showMessageBox({
          type: 'question',
          ...(restartService
            ? {
                message: `Install Anda ${version} and restart?`,
                detail:
                  'New tasks pause while active tasks finish. Then the service stops, and Anda Desktop restarts and starts it on the new version.'
              }
            : {
                message: `Install Anda Desktop ${version} and restart it?`,
                detail: 'The Anda service keeps running; only the desktop app restarts.'
              }),
          buttons: ['Later', 'Install and restart'],
          defaultId: 0,
          cancelId: 0
        })
    if (choice.response !== 1)
      return 'Desktop update downloaded. Choose Check for updates when ready to install.'
    this.installing = true
    try {
      if (!restartService) await this.quitAndInstall(version)
      else {
        // Reconnect without starting a stopped service: the restarted app starts it.
        if (!daemon.view.connected) await daemon.connect(false).catch(() => {})
        await this.drainService(version, (running) => this.quitAndInstall(version, running))
      }
    } catch (error) {
      this.installing = false
      throw error
    }
    return 'Installing update…'
  }
  private async quitAndInstall(version: string, serviceStopped = false): Promise<void> {
    this.progress('Installing Anda Desktop…')
    this.store.state.updateIntent = {
      previous: app.getVersion(),
      target: version,
      startedAt: Date.now()
    }
    await this.store.save()
    this.serviceStopped = serviceStopped
    autoUpdater.quitAndInstall(false, true)
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
