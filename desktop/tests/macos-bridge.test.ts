import { createRequire } from 'node:module'
import { execFileSync } from 'node:child_process'
import { copyFileSync, mkdtempSync, rmSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { expect, test, vi } from 'vitest'

const require = createRequire(import.meta.url)
const { assertBridgeBuild, bridgeRequirement, verifySignature } = require('../../scripts/macos-bridge.cjs')
const { optionsForBridge } = require('../scripts/sign-macos.cjs')
const desktopVersion = require('../package.json').version
const source = 'S9Z2WAMAV8'
const recipient = '63G8VNQPXB'

test.each(['0.15.0', '0.15.1'])('v%s uses the LDC bridge policy', (version) => {
  try {
    vi.stubEnv('GITHUB_REF_TYPE', 'tag')
    vi.stubEnv('GITHUB_REF_NAME', 'v' + version)
    expect(assertBridgeBuild(source, version)).toBe(version)
    expect(() => assertBridgeBuild(recipient, version)).toThrow('LDC Developer ID')
  } finally {
    vi.unstubAllEnvs()
  }
})

test('the release tag must match the desktop version', () => {
  try {
    vi.stubEnv('GITHUB_REF_TYPE', 'tag')
    vi.stubEnv('GITHUB_REF_NAME', 'v' + desktopVersion)
    expect(assertBridgeBuild(source)).toBe(desktopVersion)
    vi.stubEnv('GITHUB_REF_NAME', 'v0.14.10')
    expect(() => assertBridgeBuild(source)).toThrow(`must match desktop version v${desktopVersion}`)
  } finally {
    vi.unstubAllEnvs()
  }
})

test('the LDC bridge policy ends with the 0.15 series', () => {
  for (const version of ['0.14.10', '0.16.0']) {
    expect(() => assertBridgeBuild(source, version)).toThrow('requires a 0.15.x release')
  }
})

test('both bridge requirements pin the component identifier and Developer ID chain', () => {
  for (const [kind, id] of [['app', 'org.ldclabs.anda.desktop'], ['runtime', 'ai.anda.anda-bot']]) {
    const req = bridgeRequirement(kind)
    expect(req).toContain(`identifier "${id}" and anchor apple generic`)
    expect(req).toContain('certificate 1[field.1.2.840.113635.100.6.2.6] exists')
    expect(req).toContain('certificate leaf[field.1.2.840.113635.100.6.1.13] exists')
    expect(req).toContain(`and (certificate leaf[subject.OU] = "${source}" or certificate leaf[subject.OU] = "${recipient}")`)
    if (process.platform === 'darwin') execFileSync('/usr/bin/csreq', ['-r', '=' + req, '-t'])
  }
})

test('only the outer app receives the desktop requirement', () => {
  const app = '/tmp/Anda.app'
  const original = { entitlements: 'nested.plist', hardenedRuntime: true }
  const options = optionsForBridge({ app, ignore: ['keep-runtime'], optionsForFile: () => original })
  expect(options.ignore).toEqual(['keep-runtime'])
  expect(options.optionsForFile(app)).toEqual({ ...original, requirements: '=designated => ' + bridgeRequirement('app') })
  expect(options.optionsForFile(app + '/Contents/Frameworks/Anda Helper.app')).toBe(original)
  expect(options.optionsForFile(app + '/Contents/Resources/runtime/anda')).toBe(original)
})

test('old Macs and bridged Macs can receive different signed ZIPs from the same GitHub Latest release', async () => {
  const { GitHubProvider } = require('electron-updater/out/providers/GitHubProvider')
  const base = 'https://github.com/ldclabs/anda-bot/releases'
  for (const [channel, version, zip] of [
    [undefined, '0.15.0', 'Anda-bridge-0.15.0-mac-arm64.zip'],
    ['stable', '0.15.1', 'Anda-mac-arm64.zip']
  ] as const) {
    const executor = {
      async request({ hostname, path }: { hostname: string; path: string }) {
        expect(hostname).toBe('github.com')
        const url = 'https://' + hostname + path
        if (url === base + '.atom')
          return `<feed><entry><link href="${base}/tag/v0.15.1"/><title>Anda</title></entry></feed>`
        if (url === base + '/latest') return JSON.stringify({ tag_name: 'v0.15.1' })
        expect(url).toBe(`${base}/download/v0.15.1/${channel || 'latest'}-mac.yml`)
        return JSON.stringify({ version, files: [{ url: zip, size: 1, sha512: Buffer.alloc(64).toString('base64') }] })
      }
    }
    const provider = new GitHubProvider(
      { provider: 'github', owner: 'ldclabs', repo: 'anda-bot', channel },
      { allowPrerelease: false }, { platform: 'darwin', executor }
    )
    const update = await provider.getLatestVersion()
    expect(update.version).toBe(version)
    expect(provider.resolveFiles(update)[0].url.href).toBe(`${base}/download/v0.15.1/${zip}`)
  }
})

test.skipIf(process.platform !== 'darwin')('an ad hoc signature cannot assert that it is the LDC bridge', () => {
  const directory = mkdtempSync(join(tmpdir(), 'anda-bridge-test-'))
  try {
    const file = join(directory, 'anda')
    copyFileSync('/usr/bin/true', file)
    execFileSync('/usr/bin/codesign', [
      '--force', '--sign', '-', '--timestamp=none', '--identifier', 'ai.anda.anda-bot',
      '-r', '=designated => ' + bridgeRequirement('runtime'), file
    ], { stdio: 'pipe' })
    expect(() => verifySignature(file, 'runtime')).toThrow()
  } finally {
    rmSync(directory, { recursive: true, force: true })
  }
})
