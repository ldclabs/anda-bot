import { afterEach, describe, expect, it, vi } from 'vitest'
import { setClientPlatform, type ClientPlatform } from './platform'
import { MemoryApi } from '../memory/api'
import { BrainApi } from '../brain/api'
import { DaemonConfigApi } from '../config/api'

const settings = {
  baseUrl: 'http://127.0.0.1:8042',
  token: 'native-session',
  spaceId: 'anda_bot',
  appearanceTheme: 'system' as const,
  submitKeyMode: 'enter' as const,
  approvalMode: 'on_risk' as const
}
afterEach(() => {
  setClientPlatform(undefined)
  vi.restoreAllMocks()
})

describe('native adapters for shared extension views', () => {
  it('routes memory and Brain calls through the host without fetching with a renderer credential', async () => {
    const fetch = vi
      .spyOn(globalThis, 'fetch')
      .mockRejectedValue(new Error('renderer must not fetch'))
    const rpc = vi.fn().mockResolvedValue({ result: { schema_version: 1 } })
    setClientPlatform({ rpc } as unknown as ClientPlatform)
    await new MemoryApi(settings).overview()
    expect(rpc).toHaveBeenCalledWith('memory_overview', [])
    await new BrainApi(settings).runtimeStatus()
    expect(rpc).toHaveBeenCalledWith('brain_runtime_status', [])
    expect(fetch).not.toHaveBeenCalled()
    await expect(
      new MemoryApi({ ...settings, spaceId: 'another-space' }).overview()
    ).rejects.toThrow('unsupported_memory_space')
  })
  it('preserves the configuration revision when crossing the native boundary', async () => {
    const config = vi
      .fn()
      .mockResolvedValue({ path: 'config.yaml', content: 'next', revision: 'r2', config: {} })
    setClientPlatform({ config } as unknown as ClientPlatform)
    await new DaemonConfigApi(settings).save('next', 'r1')
    expect(config).toHaveBeenCalledWith('PUT', 'next', 'r1')
  })
})
