const fs = require('node:fs')
const path = require('node:path')
const { parse } = require('yaml')

const config = parse(fs.readFileSync(path.join(__dirname, 'electron-builder.yml'), 'utf8'))
const url = new URL(process.env.ANDA_UPDATE_URL || '')
if (url.protocol !== 'https:' || url.username || url.password)
  throw new Error('ANDA_UPDATE_URL must be a public HTTPS update directory')
if (!process.env.CSC_LINK)
  throw new Error('A release signing certificate must be configured with CSC_LINK')
config.forceCodeSigning = true
config.publish = [{ provider: 'generic', url: url.href }]
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
  JSON.stringify({ signed: true, url: url.href })
)
module.exports = config
