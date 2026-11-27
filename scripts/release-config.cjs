const fs = require('node:fs')
const path = require('node:path')

// Keep Anda's v-prefixed tags. Only numbered betas are desktop update candidates.
function releaseChannel(version) {
  if (!/^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(-beta\.(0|[1-9]\d*))?$/.test(version))
    throw new Error(`Expected X.Y.Z or X.Y.Z-beta.N, got ${version}`)
  return version.includes('-') ? 'beta' : 'stable'
}

module.exports = { releaseChannel }

if (require.main === module) {
  const version = require('../desktop/package.json').version
  const cargo = fs.readFileSync(path.join(__dirname, '../anda_bot/Cargo.toml'), 'utf8')
  const runtime = /^version = "([^"]+)"$/m.exec(cargo)?.[1]
  if (runtime !== version) throw new Error(`anda_bot is ${runtime}, Anda Desktop is ${version}`)
  if (process.env.GITHUB_REF_TYPE === 'tag' && process.env.GITHUB_REF_NAME !== 'v' + version)
    throw new Error(`Expected the release tag v${version}`)
  const channel = releaseChannel(version)
  if (process.env.GITHUB_OUTPUT) fs.appendFileSync(process.env.GITHUB_OUTPUT, `channel=${channel}\n`)
  console.log(`Anda ${version} (${channel})`)
}
