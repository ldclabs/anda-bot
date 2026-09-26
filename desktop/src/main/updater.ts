import { app, dialog } from 'electron'
import { autoUpdater } from 'electron-updater'
import { readFile } from 'node:fs/promises'
import { join } from 'node:path'
import { DaemonClient } from './daemon-client'
import { DesktopStore } from './store'
import { installCoordinated } from './update-machine'

export class DesktopUpdater {
  installing = false
  private busy = false
  private downloaded?: string
  constructor(
    private daemon: DaemonClient,
    private store: DesktopStore,
    private runningTerminals: () => number,
    private emit: (message: string) => void
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
      if (store.state.updateIntent?.managed && store.state.updateIntent.wasRunning)
        void daemon.connect()
    })
  }
  async check(): Promise<string> {
    if (this.busy) return 'An update operation is already in progress.'
    if (!app.isPackaged) return 'Updates are available only in signed release builds.'
    let configured = false
    try {
      configured =
        JSON.parse(await readFile(join(process.resourcesPath, 'release-channel.json'), 'utf8'))
          .signed === true
    } catch {}
    if (!configured)
      return 'This local build has no signed update channel. Install the next local package manually.'
    this.busy = true
    try {
      if (!this.downloaded) {
        const result = await autoUpdater.checkForUpdates()
        if (!result?.isUpdateAvailable) return 'Anda is up to date.'
        const choice = await dialog.showMessageBox({
          type: 'question',
          message: `Download Anda ${result.updateInfo.version}?`,
          buttons: ['Later', 'Download'],
          defaultId: 1,
          cancelId: 0
        })
        if (choice.response !== 1) return 'Update available; download postponed.'
        await autoUpdater.downloadUpdate()
        this.downloaded = result.updateInfo.version
      }
      if (this.runningTerminals())
        return 'Update downloaded. Close your terminal sessions before installing.'
      const choice = await dialog.showMessageBox({
        type: 'question',
        message: `Install Anda ${this.downloaded} and restart?`,
        detail:
          'New tasks will pause while active tasks finish. External daemon installations are preserved.',
        buttons: ['Later', 'Install and restart'],
        defaultId: 0,
        cancelId: 0
      })
      if (choice.response !== 1)
        return 'Update downloaded. Choose Check for updates when ready to install.'
      const daemon = this.daemon
      if (!daemon.view.connected && !daemon.manuallyStopped) {
        await daemon.connect()
        if (!daemon.view.connected)
          throw new Error('Cannot verify runtime state. Reconnect before installing.')
      }
      const wasRunning = daemon.view.connected
      if (wasRunning && daemon.view.runtimeOwnership === 'unknown')
        throw new Error(
          'The older runtime cannot prove update ownership. Stop it explicitly before installing.'
        )
      const managed = daemon.view.managed
      this.emit('Waiting for active tasks to finish…')
      this.installing = true
      await installCoordinated({
        managed,
        wasRunning,
        begin: () => daemon.maintenance('begin'),
        renew: (token) => daemon.maintenance('renew', token),
        release: async (token) => {
          await daemon.maintenance('release', token)
        },
        stop: (token) => daemon.stopForUpdate(token),
        saveIntent: async () => {
          this.store.state.updateIntent = {
            previous: app.getVersion(),
            target: this.downloaded!,
            managed,
            wasRunning,
            startedAt: Date.now()
          }
          await this.store.save()
        },
        install: () => {
          if (this.runningTerminals())
            throw new Error('Close your terminal sessions before installing.')
          autoUpdater.quitAndInstall(false, true)
        },
        recover: async () => {
          await daemon.connect()
        },
        wait: () => new Promise((resolve) => setTimeout(resolve, 5000))
      })
      return 'Installing update…'
    } catch (error) {
      this.installing = false
      throw error
    } finally {
      this.busy = false
    }
  }
  async recover(): Promise<void> {
    const intent = this.store.state.updateIntent
    if (!intent) return
    if (intent.managed && intent.wasRunning) {
      const connected = await this.daemon.connect()
      if (!connected.connected) {
        this.emit('Update recovery needs attention: the runtime is not connected.')
        return
      }
    }
    this.emit(
      app.getVersion() === intent.target
        ? `Updated to ${intent.target}.`
        : 'Previous update did not finish; your previous version and data are preserved.'
    )
    delete this.store.state.updateIntent
    await this.store.save()
  }
}
