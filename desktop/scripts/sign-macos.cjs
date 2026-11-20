const { resolve } = require('node:path')
const { signAsync } = require('@electron/osx-sign')
const { bridgeRequirement, verifySignature } = require('../../scripts/macos-bridge.cjs')

// Squirrel.Mac uses the outer app's designated requirement to validate its successor.
function optionsForBridge(options) {
  const app = resolve(options.app)
  return {
    ...options,
    optionsForFile(file) {
      const original = options.optionsForFile?.(file)
      if (resolve(file) !== app) return original
      return { ...original, requirements: '=designated => ' + bridgeRequirement('app') }
    }
  }
}

module.exports = async function signMac(options) {
  await signAsync(optionsForBridge(options))
  verifySignature(options.app, 'app')
}
module.exports.optionsForBridge = optionsForBridge
