import { createRequire } from 'node:module'
import { dirname, join } from 'node:path'
import { readdir, chmod } from 'node:fs/promises'

// node-pty 1.1's published macOS prebuild includes a non-executable helper.
// Repair the package-local helper at installation/build time, before signing.
if (process.platform !== 'win32') {
  const require = createRequire(import.meta.url)
  const root = dirname(require.resolve('node-pty/package.json'))
  async function visit(path) {
    for (const entry of await readdir(path, { withFileTypes: true }).catch(() => [])) {
      const child = join(path, entry.name)
      if (entry.isDirectory()) await visit(child)
      else if (entry.isFile() && entry.name === 'spawn-helper') await chmod(child, 0o755)
    }
  }
  await visit(join(root, 'prebuilds'))
  await visit(join(root, 'build'))
}
