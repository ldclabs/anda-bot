// Release policy for the last LDC-signed macOS bridge.
const { execFileSync, spawnSync } = require('node:child_process')
const { createHash } = require('node:crypto')
const { readFileSync } = require('node:fs')
const { join } = require('node:path')

const bridgeVersion = '0.15.0'
const sourceTeam = 'S9Z2WAMAV8'
const recipientTeam = '63G8VNQPXB'
const identifiers = {
  app: 'org.ldclabs.anda.desktop',
  runtime: 'ai.anda.anda-bot',
  launcher: 'ai.anda.anda-bot.launcher'
}

function assertBridgeBuild(team) {
  const version = require('../desktop/package.json').version
  const tag = process.env.GITHUB_REF_TYPE === 'tag' ? process.env.GITHUB_REF_NAME : undefined
  if (team !== sourceTeam || version !== bridgeVersion || (tag !== undefined && tag !== 'v' + bridgeVersion)) {
    throw new Error(`The macOS bridge requires v${bridgeVersion} and LDC Developer ID team ${sourceTeam}`)
  }
}

function requirement(kind, teams) {
  const identifier = identifiers[kind]
  if (!identifier) throw new Error(`Unknown macOS component: ${kind}`)
  const signers = teams.map(team => `certificate leaf[subject.OU] = "${team}"`).join(' or ')
  return `identifier "${identifier}" and anchor apple generic and certificate 1[field.1.2.840.113635.100.6.2.6] exists and certificate leaf[field.1.2.840.113635.100.6.1.13] exists and (${signers})`
}

function bridgeRequirement(kind) {
  return requirement(kind, [sourceTeam, recipientTeam])
}

function verifySignature(file, kind) {
  const deep = kind === 'app' ? ['--deep'] : []
  execFileSync('/usr/bin/codesign', ['--verify', '--strict', ...deep, '-R', '=' + requirement(kind, [sourceTeam]), file])
  if (kind === 'launcher') return
  // Check the policy the installed app will use on its next update as well as its actual signer.
  const result = spawnSync('/usr/bin/codesign', ['-d', '-r-', file], { encoding: 'utf8' })
  const actual = /^designated => (.+)$/m.exec(result.stdout + result.stderr)?.[1]
  const canonical = value => execFileSync('/usr/bin/csreq', ['-r', '=' + value, '-t'], { encoding: 'utf8' }).trim()
  if (result.status !== 0 || !actual || canonical(actual) !== canonical(bridgeRequirement(kind))) {
    throw new Error(`Unexpected designated requirement for ${file}`)
  }
}

function verifyPackage(app, standalone) {
  verifySignature(app, 'app')
  const resources = join(app, 'Contents/Resources')
  // The standalone signing job only needs Node. Desktop packaging supplies yaml.
  const desktopRequire = require('node:module').createRequire(require.resolve('../desktop/package.json'))
  const feed = desktopRequire('yaml').parse(readFileSync(join(resources, 'app-update.yml'), 'utf8'))
  if (feed.provider !== 'github' || feed.owner !== 'ldclabs' || feed.repo !== 'anda-bot' || feed.channel !== 'stable') {
    throw new Error('The installed bridge must read the stable macOS update feed')
  }
  const runtime = join(resources, 'runtime')
  const binary = join(runtime, 'anda')
  verifySignature(binary, 'runtime')
  const version = execFileSync('/usr/bin/plutil', ['-extract', 'CFBundleShortVersionString', 'raw', '-o', '-', join(app, 'Contents/Info.plist')], { encoding: 'utf8' }).trim()
  const manifest = JSON.parse(readFileSync(join(runtime, 'manifest.json'), 'utf8'))
  const digest = file => createHash('sha256').update(readFileSync(file)).digest('hex')
  const sha256 = digest(binary)
  if (version !== bridgeVersion || manifest.version !== 'anda_bot ' + bridgeVersion ||
      manifest.sha256 !== sha256 || sha256 !== digest(standalone)) {
    throw new Error('The bridge app, runtime manifest and standalone release binary must match')
  }
}

module.exports = { assertBridgeBuild, bridgeRequirement, identifiers, sourceTeam, verifySignature }

if (require.main === module) {
  const [command, first, second] = process.argv.slice(2)
  switch (command) {
    case 'check-build': assertBridgeBuild(first); break
    case 'runtime-requirement': console.log('=designated => ' + bridgeRequirement('runtime')); break
    case 'verify-runtime': verifySignature(first, 'runtime'); break
    case 'verify-launcher': verifySignature(first, 'launcher'); break
    case 'verify-package': verifyPackage(first, second); break
    default: throw new Error('Unknown macOS bridge verification command')
  }
}
