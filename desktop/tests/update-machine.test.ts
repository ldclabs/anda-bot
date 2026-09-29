import { expect, it } from 'vitest'
import {
  installRuntimeUpdate,
  isOlderRelease,
  type RuntimeUpdate
} from '../src/main/update-machine'
function fixture(ready = true) {
  const actions: string[] = []
  const update: RuntimeUpdate = {
    running: true,
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
    install: async (lease) => {
      actions.push(`install:${lease ?? '-'}`)
    },
    recover: async (lease) => {
      actions.push(`recover:${lease ?? '-'}`)
    },
    wait: async () => {}
  }
  return { update, actions }
}
it('drains a running daemon before replacing its runtime', async () => {
  const f = fixture()
  await installRuntimeUpdate(f.update)
  expect(f.actions).toEqual(['begin', 'renew', 'install:lease'])
})
it('keeps active work running and releases maintenance when drain times out', async () => {
  const f = fixture(false)
  await expect(installRuntimeUpdate(f.update)).rejects.toThrow('still active')
  expect(f.actions.at(-1)).toBe('release')
  expect(f.actions.some((action) => action.startsWith('install'))).toBe(false)
})
it('installs directly when the daemon is not running', async () => {
  const f = fixture()
  f.update.running = false
  await installRuntimeUpdate(f.update)
  expect(f.actions).toEqual(['install:-'])
})
it('recovers the daemon after a failed install', async () => {
  const f = fixture()
  f.update.install = async () => {
    throw new Error('replacement failed')
  }
  await expect(installRuntimeUpdate(f.update)).rejects.toThrow('replacement failed')
  expect(f.actions).toEqual(['begin', 'renew', 'recover:lease'])
})
it('orders release versions numerically', () => {
  expect(isOlderRelease('v0.9.0', '0.13.0')).toBe(true)
  expect(isOlderRelease('v0.13.0', 'v0.13.0')).toBe(false)
  expect(isOlderRelease('0.13.1', 'v0.13.0')).toBe(false)
  expect(isOlderRelease('v0.13', 'v0.13.1')).toBe(true)
})
