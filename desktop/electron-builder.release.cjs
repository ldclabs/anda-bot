const fs = require('node:fs')
const path = require('node:path')
const { parse } = require('yaml')

const config = parse(fs.readFileSync(path.join(__dirname, 'electron-builder.yml'), 'utf8'))
if (!process.env.CSC_LINK)
  throw new Error('A release signing certificate must be configured with CSC_LINK')
config.forceCodeSigning = true
// Desktop packages ship in the same GitHub release as the anda CLI, so the
// update feed is that release's latest*.yml metadata.
config.publish = [{ provider: 'github', owner: 'ldclabs', repo: 'anda-bot', releaseType: 'release' }]
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
  if (!process.env.ANDA_WINDOWS_PUBLISHER)
    throw new Error('Configure the exact certificate publisher name')
  config.win.signtoolOptions = {
    publisherName: process.env.ANDA_WINDOWS_PUBLISHER,
    signingHashAlgorithms: ['sha256']
  }
  config.win.verifyUpdateCodeSignature = true
}
// Written only by the explicit release configuration; local packages never
// inherit a release feed or claim to have a signed update channel.
config.extraResources.push({
  from: path.join(__dirname, 'resources/release-channel.json'),
  to: 'release-channel.json'
})
fs.writeFileSync(
  path.join(__dirname, 'resources/release-channel.json'),
  JSON.stringify({ signed: true, provider: 'github' })
)
module.exports = config
