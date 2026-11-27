import { afterEach, expect, it, vi } from 'vitest'
import { BetaFeed } from '../src/main/beta-feed'

vi.mock('electron', () => ({ net: { fetch: (url: string, options: RequestInit) => fetch(url, options) } }))
afterEach(() => vi.unstubAllGlobals())

function release(version: string, mac: boolean) {
  const installer = mac ? 'Anda-mac-arm64.zip' : 'Anda-win-x64.exe'
  const runtime = mac ? 'anda-macos-arm64' : 'anda-windows-x86_64.exe'
  const beta = version.includes('-beta.')
  const channel = beta ? 'beta' : mac ? 'stable' : 'latest'
  return {
    tag_name: 'v' + version,
    prerelease: beta,
    draft: false,
    assets: [
      installer,
      installer + '.blockmap',
      installer + '.sha256',
      runtime,
      runtime + '.sha256',
      channel + (mac ? '-mac.yml' : '.yml')
    ].map((name) => ({ name, state: 'uploaded' }))
  }
}

it.each([true, false])(
  'finds the highest complete release across pages (macOS = %s)',
  async (mac) => {
    vi.stubGlobal('process', {
      ...process,
      platform: mac ? 'darwin' : 'win32',
      arch: mac ? 'arm64' : 'x64'
    })
    const page = Array.from({ length: 100 }, () => release('0.16.0', mac))
    const incomplete = release('0.19.0-beta.1', mac)
    incomplete.assets[0]!.state = 'new'
    const get = vi
      .fn()
      .mockResolvedValueOnce(Response.json(page))
      .mockResolvedValueOnce(
        Response.json([
          release('0.17.0-beta.2', mac),
          release('0.17.0-beta.10', mac),
          { ...release('0.17.0', mac), draft: true },
          { ...release('0.18.0-beta.1', mac), prerelease: false },
          release('0.17.0-beta.020', mac),
          incomplete,
          release('0.16.1', mac)
        ])
      )
    vi.stubGlobal('fetch', get)
    const feed = new BetaFeed()
    const signal = new AbortController().signal
    const found = await feed.latest('0.16.0', signal)
    expect(found).toMatchObject({ version: '0.17.0-beta.10', channel: 'beta' })
    expect(await feed.latest('0.16.0', signal)).toEqual(found)
    expect(get).toHaveBeenCalledTimes(2)
  }
)

it('moves a beta installation forward to stable without downgrading a newer beta', async () => {
  vi.stubGlobal('process', { ...process, platform: 'darwin', arch: 'arm64' })
  vi.stubGlobal(
    'fetch',
    vi.fn(async () => Response.json([release('0.16.0-beta.20', true), release('0.16.0', true)]))
  )
  const signal = new AbortController().signal
  expect(await new BetaFeed().latest('0.16.0-beta.10', signal)).toMatchObject({
    version: '0.16.0',
    channel: 'stable'
  })
  expect(await new BetaFeed().latest('0.17.0-beta.1', signal)).toBeNull()
})
