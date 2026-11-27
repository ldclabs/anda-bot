const fs = require('node:fs')
const path = require('node:path')
const { parse } = require('yaml')
const { assertReleaseBuild, releaseTeam } = require('../scripts/macos-signing.cjs')
const { releaseChannel } = require('../scripts/release-config.cjs')

const config = parse(fs.readFileSync(path.join(__dirname, 'electron-builder.yml'), 'utf8'))
const certificateSha1 = process.platform === 'win32' ? process.env.ANDA_WINDOWS_CERT_SHA1 : undefined
if (process.platform === 'darwin') {
  if (!process.env.CSC_LINK)
    throw new Error('A macOS release signing certificate must be configured with CSC_LINK')
  assertReleaseBuild(process.env.APPLE_TEAM_ID)
  config.mac.sign = require('./scripts/sign-macos.cjs')
}
config.forceCodeSigning = true
// Desktop packages ship in the same GitHub release as the anda CLI, so the
// update feed is that release's latest*.yml metadata.
const beta = releaseChannel(require('./package.json').version) === 'beta'
config.publish = [{
  provider: 'github', owner: 'ldclabs', repo: 'anda-bot',
  releaseType: beta ? 'prerelease' : 'release', channel: beta ? 'beta' : 'latest'
}]
config.generateUpdatesFilesForAllChannels = false
// Pre-bridge Macs keep reading latest-mac.yml. The bridge and its successors
// read stable-mac.yml from the same GitHub release.
config.mac.publish = [{ ...config.publish[0], channel: beta ? 'beta' : 'stable' }]
config.mac.identity = `(${releaseTeam})`
config.mac.entitlements = 'resources/entitlements.mac.plist'
config.mac.entitlementsInherit = 'resources/entitlements.mac.plist'
config.mac.notarize = true
if (
  process.platform === 'darwin' &&
  !(process.env.APPLE_ID && process.env.APPLE_APP_SPECIFIC_PASSWORD && process.env.APPLE_TEAM_ID)
)
  throw new Error('Configure Apple notarization credentials')
if (process.platform === 'win32') {
  if (!(certificateSha1 || process.env.CSC_LINK) || !process.env.ANDA_WINDOWS_PUBLISHER)
    throw new Error('A Windows release needs the Yiwen signing certificate and publisher')
  config.win.signtoolOptions = {
    publisherName: process.env.ANDA_WINDOWS_PUBLISHER,
    signingHashAlgorithms: ['sha256']
  }
  if (certificateSha1) {
    config.win.signtoolOptions.certificateSha1 = certificateSha1
    config.win.signtoolOptions.rfc3161TimeStampServer = 'http://time.certum.pl'
  }
  // The embedded CLI must match the standalone release, including its own signature.
  config.win.signExts = ['!\\runtime\\anda.exe']
  config.win.verifyUpdateCodeSignature = true
}
// Written only by the explicit release configuration; local packages never
// inherit a release feed.
config.extraResources.push({
  from: path.join(__dirname, 'resources/release-channel.json'),
  to: 'release-channel.json'
})
fs.writeFileSync(
  path.join(__dirname, 'resources/release-channel.json'),
  JSON.stringify({ provider: 'github' })
)
module.exports = config
