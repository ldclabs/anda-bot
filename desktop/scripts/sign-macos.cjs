const { resolve } = require('node:path')
const { signAsync } = require('@electron/osx-sign')
const { releaseRequirement, verifySignature } = require('../../scripts/macos-signing.cjs')

// Squirrel.Mac uses the outer app's designated requirement to validate its successor.
function optionsForRelease(options) {
  const app = resolve(options.app)
  return {
    ...options,
    optionsForFile(file) {
      const original = options.optionsForFile?.(file)
      if (resolve(file) !== app) return original
      return { ...original, requirements: '=designated => ' + releaseRequirement('app') }
    }
  }
}

module.exports = async function signMac(options) {
  await signAsync(optionsForRelease(options))
  verifySignature(options.app, 'app')
}
module.exports.optionsForRelease = optionsForRelease
