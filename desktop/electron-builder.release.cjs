const fs = require('node:fs')
const path = require('node:path')
const { parse } = require('yaml')

const config = parse(fs.readFileSync(path.join(__dirname, 'electron-builder.yml'), 'utf8'))
const signed = Boolean(process.env.CSC_LINK)
// Squirrel.Mac installs only updates signed like the running app, so a macOS
// update feed needs the Developer ID certificate. Windows packages may ship
// unsigned until a release certificate is configured.
if (process.platform === 'darwin' && !signed)
  throw new Error('A macOS release signing certificate must be configured with CSC_LINK')
config.forceCodeSigning = signed
// Desktop packages ship in the same GitHub release as the anda CLI, so the
// update feed is that release's latest*.yml metadata.
config.publish = [
  { provider: 'github', owner: 'ldclabs', repo: 'anda-bot', releaseType: 'release' }
]
delete config.mac.identity
config.mac.entitlements = 'resources/entitlements.mac.plist'
config.mac.entitlementsInherit = 'resources/entitlements.mac.plist'
config.mac.notarize = true
if (
  process.platform === 'darwin' &&
  !(process.env.APPLE_ID && process.env.APPLE_APP_SPECIFIC_PASSWORD && process.env.APPLE_TEAM_ID)
)
  throw new Error('Configure Apple notarization credentials')
if (process.platform === 'win32') {
  if (signed) {
    if (!process.env.ANDA_WINDOWS_PUBLISHER)
      throw new Error('Configure the exact certificate publisher name')
    config.win.signtoolOptions = {
      publisherName: process.env.ANDA_WINDOWS_PUBLISHER,
      signingHashAlgorithms: ['sha256']
    }
  }
  // Without a publisher name electron-updater trusts the sha512 that the
  // release's latest.yml lists, as `anda update` trusts the release checksums.
  config.win.verifyUpdateCodeSignature = signed
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
