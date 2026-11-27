import { createRequire } from 'node:module'
import { execFileSync } from 'node:child_process'
import { copyFileSync, mkdtempSync, rmSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { expect, test, vi } from 'vitest'

const require = createRequire(import.meta.url)
const { assertReleaseBuild, releaseRequirement, verifySignature } = require('../../scripts/macos-signing.cjs')
const { optionsForRelease } = require('../scripts/sign-macos.cjs')
const { releaseChannel } = require('../../scripts/release-config.cjs')
const desktopVersion = require('../package.json').version
const source = 'S9Z2WAMAV8'
const recipient = '63G8VNQPXB'

test('release packaging accepts stable versions and canonical numbered betas', () => {
  expect(releaseChannel('0.16.0')).toBe('stable')
  expect(releaseChannel('0.17.0-beta.10')).toBe('beta')
  for (const version of ['0.17', '0.17.0-beta', '0.17.0-beta.01', '0.17.0-rc.1'])
    expect(() => releaseChannel(version)).toThrow()
})

test('only Yiwen can sign a release after the LDC bridge', () => {
  expect(() => assertReleaseBuild(source)).toThrow('Yiwen Developer ID')
  expect(assertReleaseBuild(recipient)).toBe(desktopVersion)
})

test('the release tag must match the desktop version', () => {
  try {
    vi.stubEnv('GITHUB_REF_TYPE', 'tag')
    vi.stubEnv('GITHUB_REF_NAME', 'v' + desktopVersion)
    expect(assertReleaseBuild(recipient)).toBe(desktopVersion)
    vi.stubEnv('GITHUB_REF_NAME', 'v0.14.10')
    expect(() => assertReleaseBuild(recipient)).toThrow(`must match desktop version v${desktopVersion}`)
  } finally {
    vi.unstubAllEnvs()
  }
})

test('app and runtime requirements pin their identifier and Yiwen Developer ID chain', () => {
  for (const [kind, id] of [['app', 'org.ldclabs.anda.desktop'], ['runtime', 'ai.anda.anda-bot']]) {
    const req = releaseRequirement(kind)
    expect(req).toContain(`identifier "${id}" and anchor apple generic`)
    expect(req).toContain('certificate 1[field.1.2.840.113635.100.6.2.6] exists')
    expect(req).toContain('certificate leaf[field.1.2.840.113635.100.6.1.13] exists')
    expect(req).toContain(`and certificate leaf[subject.OU] = "${recipient}"`)
    expect(req).not.toContain(source)
    if (process.platform === 'darwin') execFileSync('/usr/bin/csreq', ['-r', '=' + req, '-t'])
  }
})

test('only the outer app receives the desktop requirement', () => {
  const app = '/tmp/Anda.app'
  const original = { entitlements: 'nested.plist', hardenedRuntime: true }
  const options = optionsForRelease({ app, ignore: ['keep-runtime'], optionsForFile: () => original })
  expect(options.ignore).toEqual(['keep-runtime'])
  expect(options.optionsForFile(app)).toEqual({ ...original, requirements: '=designated => ' + releaseRequirement('app') })
  expect(options.optionsForFile(app + '/Contents/Frameworks/Anda Helper.app')).toBe(original)
  expect(options.optionsForFile(app + '/Contents/Resources/runtime/anda')).toBe(original)
})

test('old Macs and bridged Macs can receive different signed ZIPs from the same GitHub Latest release', async () => {
  const { GitHubProvider } = require('electron-updater/out/providers/GitHubProvider')
  const base = 'https://github.com/ldclabs/anda-bot/releases'
  for (const [channel, version, zip] of [
    [undefined, '0.15.0', 'Anda-bridge-0.15.0-mac-arm64.zip'],
    ['stable', desktopVersion, 'Anda-mac-arm64.zip']
  ] as const) {
    const executor = {
      async request({ hostname, path }: { hostname: string; path: string }) {
        expect(hostname).toBe('github.com')
        const url = 'https://' + hostname + path
        if (url === base + '.atom')
          return `<feed><entry><link href="${base}/tag/v${desktopVersion}"/><title>Anda</title></entry></feed>`
        if (url === base + '/latest') return JSON.stringify({ tag_name: `v${desktopVersion}` })
        expect(url).toBe(`${base}/download/v${desktopVersion}/${channel || 'latest'}-mac.yml`)
        return JSON.stringify({ version, files: [{ url: zip, size: 1, sha512: Buffer.alloc(64).toString('base64') }] })
      }
    }
    const provider = new GitHubProvider(
      { provider: 'github', owner: 'ldclabs', repo: 'anda-bot', channel },
      { allowPrerelease: false }, { platform: 'darwin', executor }
    )
    const update = await provider.getLatestVersion()
    expect(update.version).toBe(version)
    expect(provider.resolveFiles(update)[0].url.href).toBe(`${base}/download/v${desktopVersion}/${zip}`)
  }
})

test.skipIf(process.platform !== 'darwin')('an ad hoc signature cannot assert that it is a Yiwen release', () => {
  const directory = mkdtempSync(join(tmpdir(), 'anda-bridge-test-'))
  try {
    const file = join(directory, 'anda')
    copyFileSync('/usr/bin/true', file)
    execFileSync('/usr/bin/codesign', [
      '--force', '--sign', '-', '--timestamp=none', '--identifier', 'ai.anda.anda-bot',
      '-r', '=designated => ' + releaseRequirement('runtime'), file
    ], { stdio: 'pipe' })
    expect(() => verifySignature(file, 'runtime')).toThrow()
  } finally {
    rmSync(directory, { recursive: true, force: true })
  }
})
