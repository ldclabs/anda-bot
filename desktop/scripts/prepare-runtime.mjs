import { access, copyFile, chmod, mkdir, readFile, writeFile } from 'node:fs/promises'
import { resolve, dirname } from 'node:path'
import { fileURLToPath } from 'node:url'
import { createHash } from 'node:crypto'
import { execFileSync } from 'node:child_process'

const desktop = resolve(dirname(fileURLToPath(import.meta.url)), '..')
const repository = resolve(desktop, '..')
process.env.electron_config_cache ||= resolve(repository, 'node_modules/.cache/electron')
await import('electron') // Electron 44+ downloads its runtime on first import.
const executable = process.platform === 'win32' ? 'anda.exe' : 'anda'
let source = process.env.ANDA_DESKTOP_RUNTIME
if (!source) {
  execFileSync('cargo', ['build', '--release', '--locked', '-p', 'anda_bot', '--bin', 'anda'], {
    cwd: repository,
    stdio: 'inherit'
  })
  source = resolve(repository, 'target/release', executable)
}
await access(source)
const target = resolve(desktop, 'resources/runtime')
await mkdir(target, { recursive: true })
await copyFile(source, resolve(target, executable))
await chmod(resolve(target, executable), 0o755)
const version = execFileSync(source, ['--version'], {
  encoding: 'utf8'
}).trim()
const bytes = await readFile(source)
await writeFile(
  resolve(target, 'manifest.json'),
  JSON.stringify(
    {
      version,
      platform: process.platform,
      arch: process.arch,
      sha256: createHash('sha256').update(bytes).digest('hex')
    },
    null,
    2
  ) + '\n'
)
console.log(`Bundled ${version} for ${process.platform}/${process.arch}`)
