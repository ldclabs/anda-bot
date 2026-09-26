import { expect, it } from 'vitest'
import { installCoordinated, type UpdateCoordinator } from '../src/main/update-machine'
function fixture(ready = true) {
  const actions: string[] = []
  const coordinator: UpdateCoordinator = {
    managed: true,
    wasRunning: true,
    begin: async () => {
      actions.push('begin')
      return { token: 'lease', ready }
    },
    renew: async () => {
      actions.push('renew')
      return { token: 'lease', ready }
    },
    release: async () => {
      actions.push('release')
    },
    stop: async () => {
      actions.push('stop')
    },
    saveIntent: async () => {
      actions.push('save')
    },
    install: () => {
      actions.push('install')
    },
    recover: async () => {
      actions.push('recover')
    },
    wait: async () => {}
  }
  return { coordinator, actions }
}
it('persists update intent before stopping only a drained managed runtime', async () => {
  const f = fixture()
  await installCoordinated(f.coordinator)
  expect(f.actions).toEqual(['begin', 'renew', 'save', 'stop', 'install'])
})
it('keeps active work running and releases maintenance when drain times out', async () => {
  const f = fixture(false)
  await expect(installCoordinated(f.coordinator)).rejects.toThrow('still active')
  expect(f.actions.at(-1)).toBe('release')
  expect(f.actions).not.toContain('stop')
  expect(f.actions).not.toContain('install')
})
it('never stops or restarts an external runtime', async () => {
  const f = fixture()
  f.coordinator.managed = false
  await installCoordinated(f.coordinator)
  expect(f.actions).toEqual(['save', 'install'])
})
it('attempts recovery after failure to stop or launch the installer', async () => {
  const f = fixture()
  f.coordinator.install = () => {
    throw new Error('installer unavailable')
  }
  await expect(installCoordinated(f.coordinator)).rejects.toThrow('installer unavailable')
  expect(f.actions).toEqual(['begin', 'renew', 'save', 'stop', 'recover'])
})
