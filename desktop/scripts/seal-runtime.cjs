const { execFile } = require('node:child_process')
const { promisify } = require('node:util')
const { join } = require('node:path')
const { readFile, writeFile } = require('node:fs/promises')
const { createHash } = require('node:crypto')
const { identifiers, verifySignature } = require('../../scripts/macos-bridge.cjs')
const run = promisify(execFile)

// Release packaging preserves the already notarized anda binary byte for byte.
// Local macOS packages ad hoc sign the runtime before sealing its digest.
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
    if (context.packager.platformSpecificBuildOptions.identity === '-') {
      await run('/usr/bin/codesign', [
        '--force', '--sign', '-', '--options', 'runtime',
        '--identifier', identifiers.runtime,
        '--entitlements', join(__dirname, '../../anda_bot/assets/anda.entitlements.plist'),
        runtime
      ])
    } else {
      verifySignature(runtime, 'runtime')
    }
  }
  const manifestPath = join(resources, 'runtime/manifest.json')
  const manifest = JSON.parse(await readFile(manifestPath, 'utf8'))
  manifest.sha256 = createHash('sha256')
    .update(await readFile(runtime))
    .digest('hex')
  await writeFile(manifestPath, JSON.stringify(manifest, null, 2) + '\n')
}
