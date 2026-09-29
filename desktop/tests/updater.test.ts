import { afterEach, expect, it, vi } from 'vitest'
import type { DaemonClient } from '../src/main/daemon-client'
import type { DesktopStore } from '../src/main/store'
import { DesktopUpdater } from '../src/main/updater'

vi.mock('electron', () => ({
  app: { isPackaged: false },
  dialog: { showMessageBox: vi.fn(async () => ({ response: 1 })) }
}))
vi.mock('electron-updater', () => ({ autoUpdater: { on: vi.fn() } }))

afterEach(() => vi.useRealTimers())

function fixture(running = true) {
  vi.useFakeTimers()
  const actions: string[] = []
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
    runtimeUpdateState: vi.fn(async () => ({ status: 'current', current_tag: 'v0.14.0' }))
  }
  const updater = new DesktopUpdater(
    daemon as unknown as DaemonClient,
    { state: {} } as DesktopStore,
    () => 0,
    () => {},
    () => {}
  )
  updater.runtime = {
    status: 'downloaded',
    current_tag: 'v0.13.0',
    latest_tag: 'v0.14.0',
    downloaded_path: '/download/anda'
  }
  return { updater, daemon, actions }
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
