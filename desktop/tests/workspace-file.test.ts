import { afterEach, expect, it } from 'vitest'
import { mkdtemp, mkdir, realpath, rm, symlink, writeFile } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { readWorkspaceFile } from '../src/main/workspace-file'
import { FILE_PREVIEW_LIMIT } from '../src/shared/workbench'

const directories: string[] = []
afterEach(async () => {
  for (const path of directories.splice(0)) await rm(path, { recursive: true, force: true })
})

it('previews files inside the workspace and refuses paths that leave it', async () => {
  const home = await realpath(await mkdtemp(join(tmpdir(), 'anda-file-test-')))
  directories.push(home)
  const root = join(home, 'root')
  await mkdir(join(root, 'src'), { recursive: true })
  await writeFile(join(root, 'src', 'a.ts'), 'export {}\n')
  await writeFile(join(root, 'big.txt'), 'x'.repeat(FILE_PREVIEW_LIMIT + 10))
  await writeFile(join(root, 'bin.dat'), Buffer.from([1, 0, 2]))
  await writeFile(join(home, 'secret.txt'), 'secret')
  await symlink(join(home, 'secret.txt'), join(root, 'link.txt'))

  expect(await readWorkspaceFile(root, 'src/a.ts')).toMatchObject({
    path: 'src/a.ts',
    text: 'export {}\n',
    binary: false,
    truncated: false
  })
  expect(await readWorkspaceFile(root, join(root, 'src', 'a.ts'))).toMatchObject({
    path: 'src/a.ts'
  })
  expect(await readWorkspaceFile(root, 'big.txt')).toMatchObject({ truncated: true })
  expect(await readWorkspaceFile(root, 'bin.dat')).toMatchObject({ binary: true, text: '' })
  await expect(readWorkspaceFile(root, '../secret.txt')).rejects.toThrow('outside')
  await expect(readWorkspaceFile(root, 'link.txt')).rejects.toThrow('outside')
  await expect(readWorkspaceFile(root, 'missing.ts')).rejects.toThrow('not found')
  await expect(readWorkspaceFile(root, 'src')).rejects.toThrow('Only files')
})
