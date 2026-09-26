const { execFile } = require('node:child_process')
const { promisify } = require('node:util')
const { join } = require('node:path')
const { readFile, writeFile } = require('node:fs/promises')
const { createHash } = require('node:crypto')
const run = promisify(execFile)

// Runtime bytes must be final before sealing the manifest into the app bundle.
// electron-builder 26 signs Windows extraResources during copying, before this
// hook. On macOS we sign the runtime here and exclude only that binary from the
// subsequent recursive app-signing pass; helpers and the outer app still sign.
module.exports = async function sealRuntime(context) {
  const mac = context.electronPlatformName === 'darwin'
  const resources = mac
    ? join(
        context.appOutDir,
        `${context.packager.appInfo.productFilename}.app`,
        'Contents',
        'Resources'
      )
    : join(context.appOutDir, 'resources')
  const runtime = join(
    resources,
    'runtime',
    context.electronPlatformName === 'win32' ? 'anda.exe' : 'anda'
  )
  if (mac) {
    let identity = context.packager.platformSpecificBuildOptions.identity
    let keychain
    if (identity !== '-') {
      keychain = (await context.packager.codeSigningInfo.value).keychainFile
      const { stdout } = await run('/usr/bin/security', [
        'find-identity',
        '-v',
        '-p',
        'codesigning',
        ...(keychain ? [keychain] : [])
      ])
      const identities = [
        ...stdout.matchAll(/\b([A-Fa-f0-9]{40})\s+"(Developer ID Application:[^"]+)"/g)
      ]
      const selected = identities.find(
        (match) => !identity || match[2].includes(identity) || match[1] === identity
      )
      if (!selected) throw new Error('Developer ID runtime signing identity was not found')
      identity = selected[1]
    }
    await run('/usr/bin/codesign', [
      '--force',
      '--sign',
      identity,
      '--options',
      'runtime',
      ...(identity === '-' ? [] : ['--timestamp']),
      ...(keychain ? ['--keychain', keychain] : []),
      runtime
    ])
  }
  const manifestPath = join(resources, 'runtime/manifest.json')
  const manifest = JSON.parse(await readFile(manifestPath, 'utf8'))
  manifest.sha256 = createHash('sha256')
    .update(await readFile(runtime))
    .digest('hex')
  await writeFile(manifestPath, JSON.stringify(manifest, null, 2) + '\n')
}
