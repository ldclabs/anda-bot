import { net } from 'electron'
import { isOlderRelease } from './update-machine'

const api = 'https://api.github.com/repos/ldclabs/anda-bot/releases'
const tagPattern = /^v(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(?:-beta\.(0|[1-9]\d*))?$/

interface GitHubRelease {
  tag_name: string
  draft: boolean
  prerelease: boolean
  assets: { name: string; state: string }[]
}

export interface DesktopRelease {
  version: string
  channel: 'stable' | 'latest' | 'beta'
  installer: string
  assets: string[]
}

function desktopRelease(
  release: GitHubRelease,
  platform: string,
  arch: string
): DesktopRelease | null {
  const tag = release?.tag_name
  if (
    typeof tag !== 'string' ||
    !tagPattern.test(tag) ||
    release.draft !== false ||
    release.prerelease !== tag.includes('-beta.') ||
    !Array.isArray(release.assets)
  )
    return null
  const mac = platform === 'darwin' && arch === 'arm64'
  if (!mac && !(platform === 'win32' && arch === 'x64')) return null
  const channel = release.prerelease ? 'beta' : mac ? 'stable' : 'latest'
  const installer = mac ? 'Anda-mac-arm64.zip' : 'Anda-win-x64.exe'
  const runtime = mac ? 'anda-macos-arm64' : 'anda-windows-x86_64.exe'
  const assets = release.assets.filter((asset) => asset?.state === 'uploaded').map((asset) => asset.name)
  const required = [
    installer,
    installer + '.blockmap',
    installer + '.sha256',
    runtime,
    runtime + '.sha256',
    channel + (mac ? '-mac.yml' : '.yml')
  ]
  if (!required.every((name) => assets.includes(name))) return null
  return { version: tag.slice(1), channel, installer, assets }
}

/** GitHub Latest excludes prereleases. Compare complete releases from every page for beta opt-ins. */
export class BetaFeed {
  private cached?: { current: string; until: number; release: DesktopRelease | null }

  async latest(current: string, signal: AbortSignal): Promise<DesktopRelease | null> {
    signal.throwIfAborted()
    if (this.cached?.current === current && Date.now() < this.cached.until) return this.cached.release
    let best: DesktopRelease | null = null
    for (let page = 1; ; page++) {
      const response = await net.fetch(`${api}?per_page=100&page=${page}`, {
        headers: { Accept: 'application/vnd.github+json' },
        signal: AbortSignal.any([signal, AbortSignal.timeout(30_000)])
      })
      if (!response.ok) throw new Error(`GitHub update check failed (HTTP ${response.status}).`)
      const releases: GitHubRelease[] = await response.json()
      if (!Array.isArray(releases)) throw new Error('Invalid GitHub releases response.')
      for (const value of releases) {
        const release = desktopRelease(value, process.platform, process.arch)
        if (release && isOlderRelease(best?.version || current, release.version)) best = release
      }
      if (releases.length < 100) break
    }
    signal.throwIfAborted()
    this.cached = { current, until: Date.now() + 60_000, release: best }
    return best
  }
}
