import { afterEach, expect, it } from 'vitest'
import { mkdtemp, mkdir, readFile, writeFile, rm, realpath, access } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { join, sep } from 'node:path'
import { execFile } from 'node:child_process'
import { randomBytes } from 'node:crypto'
import { promisify } from 'node:util'
import { GitService, parseStatus } from '../src/main/git'

const run = promisify(execFile)
const directories: string[] = []
afterEach(async () => {
  for (const path of directories.splice(0)) await rm(path, { recursive: true, force: true })
})
async function fixture() {
  const home = await realpath(await mkdtemp(join(tmpdir(), 'anda-git-test-')))
  directories.push(home)
  const root = join(home, 'project with spaces')
  await mkdir(root)
  const git = (args: string[]) => run('git', args, { cwd: root })
  await git(['init', '-b', 'main'])
  await git(['config', 'user.name', 'Test'])
  await git(['config', 'user.email', 'test@localhost'])
  const service = new GitService(join(home, 'desktop'), async (path) => {
    const resolved = await realpath(path)
    if (!resolved.startsWith(`${home}${sep}`)) throw new Error('Not authorized')
    return resolved
  })
  return { root, git, service }
}
it('parses rename and literal newline paths without quoting ambiguities', () => {
  const hash = '0'.repeat(40)
  expect(
    parseStatus(
      [
        '# branch.oid (initial)',
        '# branch.head main',
        `2 R. N... 100644 100644 100644 ${hash} ${hash} R100 new\nfile  two`,
        'old file',
        `1 .M N... 100644 100644 100644 ${hash} ${hash} a b`,
        `1 .M SC.. 160000 160000 160000 ${hash} ${hash} linked module`,
        '? --flag',
        ''
      ].join('\0')
    )
  ).toEqual({
    head: '',
    branch: 'main',
    files: [
      { index: 'R', worktree: ' ', path: 'new\nfile  two', previousPath: 'old file' },
      { index: ' ', worktree: 'M', path: 'a b' },
      { index: ' ', worktree: 'M', path: 'linked module', submodule: true },
      { index: '?', worktree: '?', path: '--flag' }
    ]
  })
})
it('fingerprints large changes, repeated edits and restaged content without full diffs', async () => {
  const f = await fixture()
  await writeFile(join(f.root, 'asset.bin'), randomBytes(1024))
  await writeFile(join(f.root, 'staged.txt'), 'one')
  await f.git(['add', '.'])
  await f.git(['commit', '-m', 'init'])
  await writeFile(join(f.root, 'asset.bin'), randomBytes(9 * 1024 * 1024))
  const large = await f.service.status(f.root)
  expect(large.files).toEqual([{ index: ' ', worktree: 'M', path: 'asset.bin' }])
  await writeFile(join(f.root, 'asset.bin'), randomBytes(9 * 1024 * 1024))
  const edited = await f.service.status(f.root)
  expect(edited.revision).not.toBe(large.revision)
  await writeFile(join(f.root, 'staged.txt'), 'two')
  await f.git(['add', 'staged.txt'])
  const staged = await f.service.status(f.root)
  await writeFile(join(f.root, 'staged.txt'), 'six')
  await f.git(['add', 'staged.txt'])
  expect((await f.service.status(f.root)).revision).not.toBe(staged.revision)
})
it('rejects stale staging after a changed submodule advances again', async () => {
  const library = await fixture()
  await writeFile(join(library.root, 'file.txt'), 'base')
  await library.git(['add', '.'])
  await library.git(['commit', '-m', 'base'])
  const f = await fixture()
  const path = 'module with spaces'
  await f.git(['-c', 'protocol.file.allow=always', 'submodule', 'add', library.root, path])
  await f.git(['commit', '-am', 'add submodule'])
  const submodule = join(f.root, path)
  const git = (args: string[]) => run('git', args, { cwd: submodule })
  await git(['config', 'user.name', 'Test'])
  await git(['config', 'user.email', 'test@localhost'])
  await writeFile(join(submodule, 'file.txt'), 'first')
  await git(['commit', '-am', 'first'])
  const before = await f.service.status(f.root)
  const statusArgs = ['status', '--porcelain=v2', '-z']
  const { stdout: status } = await f.git(statusArgs)
  const { stdout: index } = await f.git(['ls-files', '--stage', '--', path])

  await writeFile(join(submodule, 'file.txt'), 'second')
  await git(['commit', '-am', 'second'])
  expect((await f.git(statusArgs)).stdout).toBe(status)
  await expect(
    f.service.request({
      action: 'stage',
      workspace: f.root,
      paths: [path],
      revision: before.revision
    })
  ).rejects.toThrow('repository changed')
  expect((await f.git(['ls-files', '--stage', '--', path])).stdout).toBe(index)

  const after = await f.service.status(f.root)
  expect(after.revision).not.toBe(before.revision)
  await f.service.request({
    action: 'stage',
    workspace: f.root,
    paths: [path],
    revision: after.revision
  })
  const { stdout: head } = await git(['rev-parse', 'HEAD'])
  expect((await f.git(['rev-parse', `:${path}`])).stdout).toBe(head)
})
it('stages literal paths, preserves files when unstaging unborn HEAD and rejects stale writes', async () => {
  const f = await fixture()
  const path = '--[中文] file.txt'
  await writeFile(join(f.root, path), 'first')
  const initial = await f.service.status(f.root)
  expect(await f.service.request({ action: 'diff', workspace: f.root, path, staged: false })).toBe(
    'first'
  )
  await writeFile(join(f.root, path), 'changed content')
  await expect(
    f.service.request({
      action: 'stage',
      workspace: f.root,
      paths: [path],
      revision: initial.revision
    })
  ).rejects.toThrow('repository changed')
  await f.service.request({
    action: 'stage',
    workspace: f.root,
    paths: [path],
    revision: (await f.service.status(f.root)).revision
  })
  await f.service.request({
    action: 'unstage',
    workspace: f.root,
    paths: [path],
    revision: (await f.service.status(f.root)).revision
  })
  expect(await readFile(join(f.root, path), 'utf8')).toBe('changed content')
  await f.service.request({
    action: 'stage',
    workspace: f.root,
    paths: [path],
    revision: (await f.service.status(f.root)).revision
  })
  await f.service.request({
    action: 'commit',
    workspace: f.root,
    message: 'Initial commit',
    revision: (await f.service.status(f.root)).revision
  })
  expect((await f.service.status(f.root)).files).toEqual([])
  await expect(
    f.service.request({ action: 'diff', workspace: f.root, path: '../private', staged: false })
  ).rejects.toThrow('Invalid repository path')
})
it('archives owned worktrees with modified and untracked files, then restores the exact content', async () => {
  const f = await fixture()
  await writeFile(join(f.root, 'tracked.txt'), 'original')
  await f.git(['add', '.'])
  await f.git(['commit', '-m', 'init'])
  const { path } = (await f.service.request({
    action: 'worktree-create',
    workspace: f.root,
    branch: 'test-work',
    base: 'HEAD',
    revision: (await f.service.status(f.root)).revision
  })) as { path: string }
  await writeFile(join(path, 'tracked.txt'), 'modified')
  await writeFile(join(path, 'new.txt'), 'untracked')
  const { archived } = (await f.service.request({
    action: 'worktree-archive',
    workspace: f.root,
    path,
    revision: (await f.service.status(f.root)).revision
  })) as { archived: string }
  await expect(access(path)).rejects.toThrow()
  await f.service.request({
    action: 'worktree-restore',
    workspace: f.root,
    id: archived,
    revision: (await f.service.status(f.root)).revision
  })
  expect(await readFile(join(path, 'tracked.txt'), 'utf8')).toBe('modified')
  expect(await readFile(join(path, 'new.txt'), 'utf8')).toBe('untracked')
  expect(await readFile(join(f.root, 'tracked.txt'), 'utf8')).toBe('original')
})
it('refuses to discard ignored files during archive', async () => {
  const f = await fixture()
  await writeFile(join(f.root, '.gitignore'), 'private.txt\n')
  await f.git(['add', '.'])
  await f.git(['commit', '-m', 'init'])
  const { path } = (await f.service.request({
    action: 'worktree-create',
    workspace: f.root,
    branch: 'ignored-test',
    base: 'HEAD',
    revision: (await f.service.status(f.root)).revision
  })) as { path: string }
  await writeFile(join(path, 'private.txt'), 'keep me')
  await expect(
    f.service.request({
      action: 'worktree-archive',
      workspace: f.root,
      path,
      revision: (await f.service.status(f.root)).revision
    })
  ).rejects.toThrow('ignored files')
  expect(await readFile(join(path, 'private.txt'), 'utf8')).toBe('keep me')
})

it('rejects a write if HEAD moved to another branch at the same commit', async () => {
  const f = await fixture()
  await writeFile(join(f.root, 'tracked.txt'), 'one')
  await f.git(['add', '.'])
  await f.git(['commit', '-m', 'initial'])
  await writeFile(join(f.root, 'tracked.txt'), 'two')
  const before = await f.service.status(f.root)
  await f.git(['switch', '-c', 'different-branch'])
  await expect(
    f.service.request({
      action: 'stage',
      workspace: f.root,
      paths: ['tracked.txt'],
      revision: before.revision
    })
  ).rejects.toThrow('repository changed')
})
