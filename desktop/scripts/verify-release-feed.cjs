// Verify the updater will download the exact package that was signed.
const { createHash } = require('node:crypto')
const { readFileSync, statSync } = require('node:fs')
const { join } = require('node:path')
const { parse } = require('yaml')
const { releaseChannel } = require('../../scripts/release-config.cjs')
const { version } = require('../package.json')

const out = join(__dirname, '../release')
const mac = process.platform === 'darwin'
const beta = releaseChannel(version) === 'beta'
const channel = beta ? 'beta' : mac ? 'stable' : 'latest'
const feed = parse(readFileSync(join(out, channel + (mac ? '-mac.yml' : '.yml')), 'utf8'))
const installer = mac ? 'Anda-mac-arm64.zip' : 'Anda-win-x64.exe'
const file = feed.files.find((entry) => entry.url === installer)
const bytes = readFileSync(join(out, installer))
if (
  feed.version !== version ||
  file?.size !== bytes.length ||
  file.sha512 !== createHash('sha512').update(bytes).digest('base64') ||
  !statSync(join(out, installer + '.blockmap')).size
) {
  throw new Error(`Invalid ${channel} update metadata for ${installer} ${version}`)
}
if (!mac) {
  const installed = parse(readFileSync(join(out, 'win-unpacked/resources/app-update.yml'), 'utf8'))
  const expected = process.env.ANDA_WINDOWS_PUBLISHER
  const publishers = [installed.publisherName].flat()
  if (!expected || publishers.length !== 1 || publishers[0] !== expected)
    throw new Error('The installed updater must verify the Yiwen publisher')
}
