import { afterEach, beforeEach, expect, it, vi } from 'vitest'
import { dialog } from 'electron'
import { autoUpdater } from 'electron-updater'
import { readFile } from 'node:fs/promises'
import type { DaemonClient, RuntimeUpdateState } from '../src/main/daemon-client'
import type { DesktopStore } from '../src/main/store'
import type { UpdateStatus } from '../src/shared/contract'
import { DesktopUpdater } from '../src/main/updater'

const build = vi.hoisted(() => ({ packaged: false }))
vi.mock('electron', () => ({
  app: {
    getVersion: () => '0.13.0',
    get isPackaged() {
      return build.packaged
    }
  },
  dialog: { showMessageBox: vi.fn(async () => ({ response: 1 })) }
}))
vi.mock('electron-updater', () => ({
  autoUpdater: {
    on: vi.fn(),
    checkForUpdates: vi.fn(),
    downloadUpdate: vi.fn(),
    quitAndInstall: vi.fn()
  }
}))
vi.mock('node:fs/promises', async (original) => ({
  ...(await original<typeof import('node:fs/promises')>()),
  readFile: vi.fn()
}))

const updaters: DesktopUpdater[] = []
function appRelease(available = false, version = '0.15.0') {
  const info = { version, files: [], releaseDate: '2026-09-30T00:00:00Z' }
  return { isUpdateAvailable: available, updateInfo: info, versionInfo: info }
}
beforeEach(() => {
  vi.resetAllMocks()
  build.packaged = false
  vi.mocked(dialog.showMessageBox).mockResolvedValue({ response: 1, checkboxChecked: false })
  vi.mocked(autoUpdater.checkForUpdates).mockResolvedValue(appRelease())
})

afterEach(() => {
  for (const updater of updaters.splice(0)) updater.stop()
  vi.useRealTimers()
  vi.unstubAllGlobals()
})

function fixture(running = true) {
  vi.useFakeTimers()
  const actions: string[] = []
  const statuses: UpdateStatus[] = []
  const emitted = vi.fn()
  const changed = vi.fn()
  const daemon = {
    view: { connected: running },
    manuallyStopped: !running,
    maintenance: vi.fn(async (action: string) => {
      actions.push(action)
      return { token: 'lease', ready: true }
    }),
    stopForUpdate: vi.fn(async () => {
      actions.push('stop')
      daemon.manuallyStopped = true
      daemon.view.connected = false
    }),
    applyRuntimeUpdate: vi.fn(async () => {
      actions.push('install')
    }),
    startRuntime: vi.fn(async () => {
      actions.push('start')
      daemon.manuallyStopped = false
      daemon.view.connected = true
    }),
    runtimeUpdateState: vi.fn(async (): Promise<RuntimeUpdateState> => ({
      status: 'current',
      current_tag: 'v0.14.0'
    }))
  }
  const store = { state: {}, save: vi.fn(async () => {}) } as unknown as DesktopStore
  const updater = new DesktopUpdater(
    daemon as unknown as DaemonClient,
    store,
    () => 0,
    emitted,
    changed,
    (status) => statuses.push(status)
  )
  updaters.push(updater)
  updater.runtime = {
    status: 'downloaded',
    current_tag: 'v0.13.0',
    latest_tag: 'v0.14.0',
    downloaded_path: '/download/anda'
  }
  return { updater, daemon, store, actions, statuses, emitted, changed }
}

function packagedFixture(running = true) {
  build.packaged = true
  vi.stubGlobal('process', { ...process, resourcesPath: '/test/resources' })
  vi.mocked(readFile).mockResolvedValue(JSON.stringify({ signed: true }))
  return fixture(running)
}

it('keeps the drained service stopped when installation outlasts its lease', async () => {
  const f = fixture()
  f.daemon.applyRuntimeUpdate.mockImplementation(async () => {
    f.actions.push('install')
    await new Promise((resolve) => setTimeout(resolve, 91_000))
  })
  const installing = f.updater.installRuntime()
  await vi.advanceTimersByTimeAsync(5000)
  expect(f.actions).toEqual(['begin', 'renew', 'stop', 'install'])
  await vi.advanceTimersByTimeAsync(90_000)
  expect(f.daemon.view.connected).toBe(false)
  expect(f.daemon.manuallyStopped).toBe(true)
  expect(f.daemon.startRuntime).not.toHaveBeenCalled()
  await vi.advanceTimersByTimeAsync(1000)
  await expect(installing).resolves.toContain('is installed')
  expect(f.actions).toEqual(['begin', 'renew', 'stop', 'install', 'start'])
})

it('brings the stopped service back after installation fails', async () => {
  const f = fixture()
  f.daemon.applyRuntimeUpdate.mockRejectedValue(new Error('replacement failed'))
  const result = expect(f.updater.installRuntime()).rejects.toThrow('replacement failed')
  await vi.runAllTimersAsync()
  await result
  expect(f.actions).toEqual(['begin', 'renew', 'stop', 'start'])
  expect(f.daemon.view.connected).toBe(true)
  expect(f.daemon.manuallyStopped).toBe(false)
})

it('releases maintenance without installing when shutdown is refused', async () => {
  const f = fixture()
  f.daemon.stopForUpdate.mockImplementation(async () => {
    f.daemon.manuallyStopped = true
    throw new Error('Runtime still has active work')
  })
  const result = expect(f.updater.installRuntime()).rejects.toThrow('still has active work')
  await vi.runAllTimersAsync()
  await result
  expect(f.actions).toEqual(['begin', 'renew', 'release'])
  expect(f.daemon.applyRuntimeUpdate).not.toHaveBeenCalled()
  expect(f.daemon.startRuntime).not.toHaveBeenCalled()
  expect(f.daemon.manuallyStopped).toBe(false)
})

it('leaves an explicitly stopped service stopped after installing', async () => {
  const f = fixture(false)
  await f.updater.installRuntime()
  expect(f.actions).toEqual(['install'])
  expect(f.daemon.view.connected).toBe(false)
})

it('publishes progress immediately and retains the final check result', async () => {
  const f = fixture()
  f.daemon.runtimeUpdateState.mockImplementation(async () => {
    await new Promise((resolve) => setTimeout(resolve, 1000))
    return { status: 'current', current_tag: 'v0.14.0' }
  })
  const checking = f.updater.check()
  expect(f.updater.status).toEqual({
    phase: 'running',
    message: expect.stringContaining('Checking Anda runtime')
  })
  // Repeated clicks keep the current operation and its progress intact.
  const pending = f.updater.status
  await f.updater.check()
  expect(f.updater.status).toBe(pending)
  expect(f.daemon.runtimeUpdateState).toHaveBeenCalledTimes(1)
  await vi.advanceTimersByTimeAsync(1000)
  const message = await checking
  expect(message).toContain('v0.14.0 is up to date')
  expect(message).toContain('only in release builds')
  expect(f.statuses).toContainEqual({
    phase: 'running',
    message: 'Checking Anda Desktop updates…'
  })
  expect(f.updater.status).toEqual({ phase: 'complete', message })
})

it('retains check failures and allows retrying', async () => {
  const f = packagedFixture()
  f.daemon.runtimeUpdateState.mockRejectedValueOnce(new Error('Release server unavailable'))
  const message = await f.updater.check()
  expect(message).toContain('Anda runtime: Release server unavailable')
  expect(message).toContain('Anda Desktop is up to date.')
  expect(autoUpdater.checkForUpdates).toHaveBeenCalledTimes(1)
  expect(f.updater.status).toEqual({ phase: 'error', message })
  await f.updater.check()
  expect(f.updater.status?.phase).toBe('complete')
})

it('shows runtime errors returned by the CLI as failures', async () => {
  const f = packagedFixture()
  f.daemon.runtimeUpdateState.mockResolvedValue({
    status: 'failed',
    current_tag: 'v0.14.0',
    error: 'Download failed'
  })
  await f.updater.check()
  expect(f.updater.status?.phase).toBe('error')
  expect(f.updater.status?.message).toContain('Download failed')
  expect(f.updater.status?.message).toContain('Anda Desktop is up to date.')
  expect(autoUpdater.checkForUpdates).toHaveBeenCalledTimes(1)
})

it.each([0, 1])(
  'continues to download and install desktop after the runtime prompt (response: %s)',
  async (response) => {
    const f = packagedFixture()
    f.daemon.runtimeUpdateState.mockResolvedValueOnce(f.updater.runtime!)
    vi.mocked(autoUpdater.checkForUpdates).mockResolvedValue(appRelease(true))
    vi.mocked(dialog.showMessageBox).mockResolvedValueOnce({ response, checkboxChecked: false })
    const checking = f.updater.check()
    await vi.runAllTimersAsync()
    const message = await checking

    expect(message).toContain(response === 1 ? 'Anda v0.14.0 is installed.' : 'stays downloaded')
    expect(message).toContain('Installing update')
    expect(f.actions).toEqual(response === 1 ? ['begin', 'renew', 'stop', 'install', 'start'] : [])
    expect(f.daemon.view.connected).toBe(true)
    expect(autoUpdater.checkForUpdates).toHaveBeenCalledTimes(1)
    expect(autoUpdater.downloadUpdate).toHaveBeenCalledTimes(1)
    expect(dialog.showMessageBox).toHaveBeenNthCalledWith(
      2,
      expect.objectContaining({ message: 'Download Anda Desktop 0.15.0?' })
    )
    expect(dialog.showMessageBox).toHaveBeenNthCalledWith(
      3,
      expect.objectContaining({ message: 'Install Anda Desktop 0.15.0 and restart it?' })
    )
    expect(f.store.state.updateIntent).toEqual({
      previous: '0.13.0',
      target: '0.15.0',
      startedAt: expect.any(Number)
    })
    expect(f.store.save).toHaveBeenCalledTimes(1)
    expect(autoUpdater.quitAndInstall).toHaveBeenCalledExactlyOnceWith(false, true)
    expect(f.updater.status).toEqual({ phase: 'complete', message })
  }
)

it('still offers a desktop update after a runtime installation fails and recovers', async () => {
  const f = packagedFixture()
  f.daemon.runtimeUpdateState.mockResolvedValueOnce(f.updater.runtime!)
  f.daemon.applyRuntimeUpdate.mockRejectedValue(new Error('replacement failed'))
  vi.mocked(autoUpdater.checkForUpdates).mockResolvedValue(appRelease(true))
  vi.mocked(dialog.showMessageBox)
    .mockResolvedValueOnce({ response: 1, checkboxChecked: false })
    .mockResolvedValueOnce({ response: 0, checkboxChecked: false })
  const checking = f.updater.check()
  await vi.runAllTimersAsync()
  const message = await checking

  expect(f.actions).toEqual(['begin', 'renew', 'stop', 'start'])
  expect(f.daemon.view.connected).toBe(true)
  expect(message).toContain('Anda runtime: replacement failed')
  expect(message).toContain('Desktop update available; download postponed.')
  expect(f.updater.desktopRelease).toBe('0.15.0')
  expect(f.updater.status).toEqual({ phase: 'error', message })
  expect(autoUpdater.downloadUpdate).not.toHaveBeenCalled()
  expect(autoUpdater.quitAndInstall).not.toHaveBeenCalled()
})

it('keeps the successful runtime result visible when the desktop check fails', async () => {
  const f = packagedFixture(false)
  f.daemon.runtimeUpdateState.mockResolvedValueOnce(f.updater.runtime!)
  vi.mocked(autoUpdater.checkForUpdates).mockRejectedValue(new Error('Desktop feed unavailable'))
  const message = await f.updater.check()

  expect(message).toContain('Anda v0.14.0 is installed.')
  expect(message).toContain('Anda Desktop: Desktop feed unavailable')
  expect(f.actions).toEqual(['install'])
  expect(f.updater.status).toEqual({ phase: 'error', message })
})

it('reports both errors when neither update channel can be checked', async () => {
  const f = packagedFixture()
  f.daemon.runtimeUpdateState.mockRejectedValue(new Error('Runtime unavailable'))
  vi.mocked(autoUpdater.checkForUpdates).mockRejectedValue(new Error('Desktop feed unavailable'))
  const message = await f.updater.check()

  expect(message).toContain('Anda runtime: Runtime unavailable')
  expect(message).toContain('Anda Desktop: Desktop feed unavailable')
  expect(f.updater.status).toEqual({ phase: 'error', message })
})

it('explains manual desktop updates after installing the runtime in an unsigned build', async () => {
  const f = packagedFixture(false)
  vi.mocked(readFile).mockResolvedValue(JSON.stringify({ signed: false }))
  f.daemon.runtimeUpdateState.mockResolvedValueOnce(f.updater.runtime!)
  const message = await f.updater.check()

  expect(message).toContain('Anda v0.14.0 is installed.')
  expect(message).toContain('no signed update channel')
  expect(autoUpdater.checkForUpdates).not.toHaveBeenCalled()
  expect(f.updater.status).toEqual({ phase: 'complete', message })
})

it('reports a downloaded release and preserves it when installation is postponed', async () => {
  const f = fixture()
  vi.mocked(dialog.showMessageBox).mockResolvedValueOnce({ response: 0, checkboxChecked: false })
  const result = await f.updater.installRuntime()
  expect(f.statuses).toContainEqual({
    phase: 'running',
    message: 'Anda v0.14.0 is ready to install.'
  })
  expect(f.updater.status).toEqual({ phase: 'complete', message: result })
  expect(f.daemon.applyRuntimeUpdate).not.toHaveBeenCalled()
  expect(f.updater.runtimeRelease).toBe('v0.14.0')
})

it('checks both channels after one minute and every six hours without opening a dialog', async () => {
  const f = packagedFixture()
  f.updater.startAutomaticChecks()
  f.updater.startAutomaticChecks()
  await vi.advanceTimersByTimeAsync(59_999)
  expect(autoUpdater.checkForUpdates).not.toHaveBeenCalled()
  expect(f.daemon.runtimeUpdateState).not.toHaveBeenCalled()
  await vi.advanceTimersByTimeAsync(1)
  expect(f.daemon.runtimeUpdateState).toHaveBeenCalledExactlyOnceWith(false)
  expect(autoUpdater.checkForUpdates).toHaveBeenCalledTimes(1)
  await vi.advanceTimersByTimeAsync(6 * 3600_000)
  expect(autoUpdater.checkForUpdates).toHaveBeenCalledTimes(2)
  expect(f.daemon.runtimeUpdateState).toHaveBeenCalledTimes(2)
  expect(f.statuses).toEqual([])
  expect(f.emitted).not.toHaveBeenCalled()
  expect(dialog.showMessageBox).not.toHaveBeenCalled()
  expect(autoUpdater.downloadUpdate).not.toHaveBeenCalled()
  expect(autoUpdater.quitAndInstall).not.toHaveBeenCalled()
  f.updater.stop()
  await vi.advanceTimersByTimeAsync(6 * 3600_000)
  expect(autoUpdater.checkForUpdates).toHaveBeenCalledTimes(2)
})

it('still checks desktop updates when the runtime was explicitly stopped', async () => {
  const f = packagedFixture(false)
  f.updater.startAutomaticChecks()
  await vi.advanceTimersByTimeAsync(60_000)
  expect(f.daemon.runtimeUpdateState).not.toHaveBeenCalled()
  expect(autoUpdater.checkForUpdates).toHaveBeenCalledTimes(1)
  expect(f.daemon.startRuntime).not.toHaveBeenCalled()
})

it('reports each new desktop version once even if the runtime check fails', async () => {
  const f = packagedFixture()
  f.daemon.runtimeUpdateState.mockRejectedValue(new Error('Runtime unavailable'))
  vi.mocked(autoUpdater.checkForUpdates).mockResolvedValue(appRelease(true))
  f.updater.startAutomaticChecks()
  await vi.advanceTimersByTimeAsync(60_000)
  expect(f.updater.desktopRelease).toBe('0.15.0')
  expect(f.changed).toHaveBeenCalledTimes(1)
  expect(f.emitted).toHaveBeenCalledExactlyOnceWith('Anda Desktop 0.15.0 available')
  await vi.advanceTimersByTimeAsync(6 * 3600_000)
  expect(f.emitted).toHaveBeenCalledTimes(1)
  vi.mocked(autoUpdater.checkForUpdates).mockResolvedValue(appRelease(true, '0.16.0'))
  await vi.advanceTimersByTimeAsync(6 * 3600_000)
  expect(f.emitted).toHaveBeenLastCalledWith('Anda Desktop 0.16.0 available')
  expect(f.emitted).toHaveBeenCalledTimes(2)
  vi.mocked(autoUpdater.checkForUpdates).mockResolvedValue(appRelease(true))
  await vi.advanceTimersByTimeAsync(6 * 3600_000)
  expect(f.emitted).toHaveBeenCalledTimes(2)
  expect(f.updater.status).toBeNull()
  expect(dialog.showMessageBox).not.toHaveBeenCalled()
  expect(autoUpdater.downloadUpdate).not.toHaveBeenCalled()
})

it('reports updater errors that arrive after an operation finished', async () => {
  const f = fixture()
  const onError = vi.mocked(autoUpdater.on).mock.calls.find(([event]) => event === 'error')![1]
  f.updater.installing = true
  onError(new Error('Install failed'))
  expect(f.updater.installing).toBe(false)
  expect(f.updater.status).toEqual({ phase: 'error', message: 'Install failed' })
  expect(f.emitted).toHaveBeenCalledExactlyOnceWith('Update failed: Install failed')
})

it('keeps automatic check errors quiet and retries at the next interval', async () => {
  const f = packagedFixture()
  const onError = vi.mocked(autoUpdater.on).mock.calls.find(([event]) => event === 'error')![1]
  vi.mocked(autoUpdater.checkForUpdates).mockImplementationOnce(async () => {
    const error = new Error('Network unavailable')
    onError(error)
    throw error
  })
  f.updater.startAutomaticChecks()
  await vi.advanceTimersByTimeAsync(60_000)
  expect(f.emitted).not.toHaveBeenCalled()
  expect(f.updater.status).toBeNull()
  await vi.advanceTimersByTimeAsync(6 * 3600_000)
  expect(autoUpdater.checkForUpdates).toHaveBeenCalledTimes(2)
  expect(f.emitted).not.toHaveBeenCalled()
})

it.each([false, true])(
  'skips desktop checks without a signed release channel (packaged: %s)',
  async (packaged) => {
    const f = packagedFixture()
    build.packaged = packaged
    vi.mocked(readFile).mockResolvedValue(JSON.stringify({ signed: false }))
    f.updater.startAutomaticChecks()
    await vi.advanceTimersByTimeAsync(60_000)
    expect(autoUpdater.checkForUpdates).not.toHaveBeenCalled()
    expect(f.daemon.runtimeUpdateState).toHaveBeenCalledExactlyOnceWith(false)
  }
)

it('waits for an automatic check before starting a manual desktop check', async () => {
  const f = packagedFixture()
  f.daemon.runtimeUpdateState.mockImplementation(async () => {
    await new Promise((resolve) => setTimeout(resolve, 1000))
    return { status: 'current', current_tag: 'v0.14.0' }
  })
  f.updater.startAutomaticChecks()
  await vi.advanceTimersByTimeAsync(60_000)
  const checking = f.updater.checkDesktop()
  expect(f.updater.status?.phase).toBe('running')
  expect(autoUpdater.checkForUpdates).toHaveBeenCalledTimes(1)
  await vi.advanceTimersByTimeAsync(1000)
  await expect(checking).resolves.toBe('Anda Desktop is up to date.')
  expect(autoUpdater.checkForUpdates).toHaveBeenCalledTimes(2)
  expect(f.updater.status?.phase).toBe('complete')
})

it('skips automatic checks during a manual update operation', async () => {
  const f = packagedFixture()
  f.daemon.runtimeUpdateState.mockImplementation(async () => {
    await new Promise((resolve) => setTimeout(resolve, 61_000))
    return { status: 'current', current_tag: 'v0.14.0' }
  })
  f.updater.startAutomaticChecks()
  const checking = f.updater.check()
  await vi.advanceTimersByTimeAsync(60_000)
  expect(f.daemon.runtimeUpdateState).toHaveBeenCalledExactlyOnceWith(true)
  expect(autoUpdater.checkForUpdates).not.toHaveBeenCalled()
  await vi.advanceTimersByTimeAsync(1000)
  await checking
  expect(autoUpdater.checkForUpdates).toHaveBeenCalledTimes(1)
})

it('does not notify or rearm the timer if stopped during an automatic check', async () => {
  const f = packagedFixture()
  vi.mocked(autoUpdater.checkForUpdates).mockImplementation(async () => {
    await new Promise((resolve) => setTimeout(resolve, 1000))
    return appRelease(true)
  })
  f.updater.startAutomaticChecks()
  await vi.advanceTimersByTimeAsync(60_000)
  f.updater.stop()
  await vi.advanceTimersByTimeAsync(24 * 3600_000)
  expect(autoUpdater.checkForUpdates).toHaveBeenCalledTimes(1)
  expect(f.emitted).not.toHaveBeenCalled()
  expect(vi.getTimerCount()).toBe(0)
})

it('opens desktop updates independently of a failed runtime check', async () => {
  const f = packagedFixture(false)
  f.updater.runtime = { status: 'failed', current_tag: 'v0.14.0', error: 'Runtime unavailable' }
  vi.mocked(autoUpdater.checkForUpdates).mockResolvedValue(appRelease(true))
  vi.mocked(dialog.showMessageBox).mockResolvedValueOnce({ response: 0, checkboxChecked: false })
  await expect(f.updater.checkDesktop()).resolves.toContain('download postponed')
  expect(f.updater.status?.phase).toBe('complete')
  expect(f.updater.desktopRelease).toBe('0.15.0')
  expect(f.daemon.runtimeUpdateState).not.toHaveBeenCalled()
})

it('does not report a null desktop check result as up to date', async () => {
  const f = packagedFixture()
  vi.mocked(autoUpdater.checkForUpdates).mockResolvedValue(null)
  await expect(f.updater.checkDesktop()).rejects.toThrow('Unable to check')
  expect(f.updater.status?.phase).toBe('error')
})
