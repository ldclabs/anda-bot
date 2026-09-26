import { afterEach, expect, it } from 'vitest'
import { mkdtemp, mkdir, readFile, writeFile, rm, realpath, access } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { execFile } from 'node:child_process'
import { promisify } from 'node:util'
import { GitService, parseStatus } from '../src/main/git'
import { workbenchMessages } from '../src/renderer/workbench-labels'

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
    if (!resolved.startsWith(`${home}/`)) throw new Error('Not authorized')
    return resolved
  })
  return { root, git, service }
}
it('parses rename and literal newline paths without quoting ambiguities', () => {
  expect(parseStatus('R  new\nfile\0old file\0?? --flag\0')).toEqual([
    { index: 'R', worktree: ' ', path: 'new\nfile', previousPath: 'old file' },
    { index: '?', worktree: '?', path: '--flag' }
  ])
})
it('keeps six workbench dictionaries aligned', () => {
  for (const locale of Object.values(workbenchMessages))
    expect(Object.keys(locale)).toEqual(Object.keys(workbenchMessages.en))
})
it('stages literal paths, preserves files when unstaging unborn HEAD and rejects stale writes', async () => {
  const f = await fixture()
  const path = '--[中文] file.txt'
  await writeFile(join(f.root, path), 'first')
  const initial = await f.service.status(f.root)
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
