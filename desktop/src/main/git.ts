import { execFile } from 'node:child_process'
import { promisify } from 'node:util'
import { randomUUID, createHash } from 'node:crypto'
import { mkdtemp, readFile, writeFile, mkdir, rename, rm, realpath, lstat } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { join, isAbsolute, dirname } from 'node:path'
import type { GitFile, GitRequest, GitSnapshot } from '../shared/workbench'

const execute = promisify(execFile)
const MAX_OUTPUT = 8 * 1024 * 1024
interface Archive {
  id: string
  repository: string
  path: string
  name: string
  commit: string
  time: number
  archived: boolean
}
interface Registry {
  managed: Record<string, string>
  archives: Archive[]
}

const STATUS = ['status', '--porcelain=v2', '-z', '--untracked-files=all']

/** Parses `git status --porcelain=v2 -z [--branch]`, reporting v1 status codes (` ` for unchanged). */
export function parseStatus(raw: string): { head: string; branch: string; files: GitFile[] } {
  const parts = raw.split('\0')
  const status = { head: '', branch: '', files: [] as GitFile[] }
  // Fields before the path: `1 XY sub mH mI mW hH hI`, `2 … Xscore`, `u … h1 h2 h3`.
  const pathAfter = { '1': 8, '2': 9, u: 10 } as Record<string, number>
  for (let i = 0; i < parts.length; i++) {
    const entry = parts[i]
    const kind = entry.slice(0, entry.indexOf(' '))
    if (entry.startsWith('# branch.oid ')) {
      const oid = entry.slice(13)
      status.head = oid === '(initial)' ? '' : oid
    } else if (entry.startsWith('# branch.head ')) status.branch = entry.slice(14)
    else if (kind === '?') status.files.push({ index: '?', worktree: '?', path: entry.slice(2) })
    else if (kind in pathAfter) {
      const file: GitFile = {
        index: entry[2].replace('.', ' '),
        worktree: entry[3].replace('.', ' '),
        path: entry.split(' ').slice(pathAfter[kind]).join(' ')
      }
      if (entry[5] === 'S') file.submodule = true
      if (kind === '2') file.previousPath = parts[++i]
      status.files.push(file)
    }
  }
  return status
}

/** Fixed Git operations over user-selected repositories. Never interpolates a shell command. */
export class GitService {
  private writes = new Map<string, Promise<unknown>>()
  private registryWrites: Promise<unknown> = Promise.resolve()
  constructor(
    private directory: string,
    private authorize: (path: string) => Promise<string>
  ) {}

  private async git(cwd: string, args: string[], env: NodeJS.ProcessEnv = {}): Promise<string> {
    const inherited = Object.fromEntries(
      Object.entries(process.env).filter(([key]) => !key.startsWith('GIT_'))
    )
    try {
      const { stdout } = await execute(
        'git',
        ['--no-pager', '--literal-pathspecs', '-c', 'core.fsmonitor=false', ...args],
        {
          cwd,
          env: { ...inherited, GIT_TERMINAL_PROMPT: '0', ...env },
          windowsHide: true,
          timeout: 30_000,
          maxBuffer: MAX_OUTPUT,
          encoding: 'utf8'
        }
      )
      return stdout
    } catch (error) {
      const e = error as NodeJS.ErrnoException & { stderr?: string }
      if (e.code === 'ENOENT') throw new Error('Git is not installed or is not available on PATH.')
      throw new Error(
        e.stderr?.trim().slice(0, 3000) || 'Git operation failed or exceeded the output limit.'
      )
    }
  }
  private async root(path: string): Promise<string> {
    const workspace = await this.authorize(path)
    const root = await realpath(
      (await this.git(workspace, ['rev-parse', '--show-toplevel'])).trim()
    )
    if (root !== workspace)
      throw new Error('Select the repository root as the workspace to use Git operations.')
    return root
  }
  private async registry(): Promise<Registry> {
    try {
      return JSON.parse(await readFile(join(this.directory, 'worktrees.json'), 'utf8'))
    } catch (error) {
      if ((error as NodeJS.ErrnoException).code !== 'ENOENT') throw error
      return { managed: {}, archives: [] }
    }
  }
  private updateRegistry(fn: (registry: Registry) => void): Promise<void> {
    const write = this.registryWrites
      .catch(() => {})
      .then(async () => {
        const state = await this.registry()
        fn(state)
        await mkdir(this.directory, { recursive: true, mode: 0o700 })
        const target = join(this.directory, 'worktrees.json')
        const temp = `${target}.${randomUUID()}.tmp`
        await writeFile(temp, JSON.stringify(state), { mode: 0o600 })
        await rename(temp, target)
      })
    this.registryWrites = write
    return write
  }
  /**
   * Status plus a revision that changes with HEAD, the branch, staged content
   * (porcelain v2 carries index object IDs) and any edit to a changed or
   * untracked file (status alone cannot see a second edit to a modified file).
   * Changed submodules also include their checked-out HEAD.
   */
  private async scan(
    root: string
  ): Promise<{ head: string; branch: string; files: GitFile[]; revision: string }> {
    const raw = await this.git(root, [...STATUS, '--branch', '--no-ahead-behind'])
    const status = parseStatus(raw)
    const hash = createHash('sha256').update(raw)
    const worktreeState = await Promise.all(
      status.files
        .filter((f) => f.worktree !== ' ')
        .map(async (file) => {
          const path = join(root, file.path)
          const stat = await lstat(path).catch(() => null)
          if (!stat) return '\0-'
          // A submodule commit can leave both its status and directory metadata unchanged.
          const head =
            stat.isDirectory() && file.submodule
              ? await this.git(path, ['rev-parse', '--verify', 'HEAD'])
              : ''
          return `\0${stat.size}:${stat.mtimeMs}:${stat.ctimeMs}:${stat.ino}:${head}`
        })
    )
    for (const state of worktreeState) hash.update(state)
    return { ...status, revision: hash.digest('hex') }
  }
  private async common(root: string): Promise<string> {
    return realpath(
      (await this.git(root, ['rev-parse', '--path-format=absolute', '--git-common-dir'])).trim()
    )
  }
  async status(workspace: string): Promise<GitSnapshot> {
    const root = await this.root(workspace)
    const [scan, branches, log, worktrees, registry, repository] = await Promise.all([
      this.scan(root),
      this.git(root, ['for-each-ref', '--format=%(refname:short)', 'refs/heads']),
      // An unborn HEAD has no log.
      this.git(root, ['log', '-30', '--format=%h%x00%s%x00']).catch(() => ''),
      this.git(root, ['worktree', 'list', '--porcelain', '-z']),
      this.registry(),
      this.common(root)
    ])
    const logs = log.split('\0')
    const trees: GitSnapshot['worktrees'] = []
    for (const part of worktrees.split('\0')) {
      if (part.startsWith('worktree ')) trees.push({ path: part.slice(9), branch: '', head: '' })
      else if (part.startsWith('HEAD ') && trees.length) trees.at(-1)!.head = part.slice(5)
      else if (part.startsWith('branch ') && trees.length)
        trees.at(-1)!.branch = part.slice(7).replace(/^refs\/heads\//, '')
    }
    return {
      root,
      head: scan.head,
      branch: scan.branch,
      revision: scan.revision,
      files: scan.files,
      branches: branches.trim().split('\n').filter(Boolean),
      log: Array.from({ length: Math.floor(logs.length / 2) }, (_, i) => ({
        hash: logs[i * 2].trim(),
        subject: logs[i * 2 + 1]
      })),
      worktrees: trees,
      archives: registry.archives
        .filter((a) => a.repository === repository && a.archived)
        .map(({ id, name, time }) => ({ id, name, time }))
    }
  }
  async request(request: GitRequest): Promise<unknown> {
    if (!request || typeof request.workspace !== 'string') throw new Error('Invalid Git request')
    if (request.action === 'status') return this.status(request.workspace)
    const root = await this.root(request.workspace)
    if (request.action === 'diff') {
      this.paths([request.path])
      // A preview only needs this path's status, not the whole-repository fingerprint.
      const { files } = parseStatus(await this.git(root, [...STATUS, '--', request.path]))
      if (files.some((f) => f.path === request.path && f.index === '?')) {
        const path = join(root, request.path)
        const stat = await lstat(path)
        if (!stat.isFile() || stat.size > 1024 * 1024)
          return 'Preview unavailable: large file or symbolic link.'
        const bytes = await readFile(path)
        return bytes.includes(0) ? 'Binary file' : bytes.toString('utf8')
      }
      return this.git(root, [
        'diff',
        '--no-ext-diff',
        '--no-textconv',
        ...(request.staged ? ['--cached'] : []),
        '--',
        request.path
      ])
    }
    const repository = await this.common(root)
    const pending = (this.writes.get(repository) || Promise.resolve())
      .catch(() => {})
      .then(async () => {
        const current = await this.scan(root)
        if (request.revision !== current.revision)
          throw new Error(
            'The repository changed. Refresh and review before applying this operation.'
          )
        switch (request.action) {
          case 'stage':
            this.paths(request.paths)
            await this.git(root, ['add', '--', ...request.paths])
            break
          case 'unstage':
            this.paths(request.paths)
            await this.git(
              root,
              current.head
                ? ['restore', '--staged', '--', ...request.paths]
                : ['rm', '--cached', '--', ...request.paths]
            )
            break
          case 'commit':
            if (
              typeof request.message !== 'string' ||
              !request.message.trim() ||
              request.message.length > 10000
            )
              throw new Error('Enter a commit message.')
            await this.git(root, ['commit', '-m', request.message])
            break
          case 'worktree-create': {
            if (!request.branch || request.branch.startsWith('-') || request.base.startsWith('-'))
              throw new Error('Invalid branch')
            await this.git(root, ['check-ref-format', '--branch', request.branch])
            const base = (
              await this.git(root, ['rev-parse', '--verify', `${request.base}^{commit}`])
            ).trim()
            const requestedPath = join(this.directory, 'worktrees', randomUUID())
            await mkdir(dirname(requestedPath), { recursive: true })
            await this.git(root, ['worktree', 'add', '-b', request.branch, requestedPath, base])
            const path = await realpath(requestedPath)
            await this.updateRegistry((r) => {
              r.managed[path] = repository
            })
            return { path, name: request.branch }
          }
          case 'worktree-archive':
            return this.archive(root, repository, request.path)
          case 'worktree-restore': {
            const entry = (await this.registry()).archives.find(
              (a) => a.id === request.id && a.repository === repository && a.archived
            )
            if (!entry) throw new Error('Archive not found')
            await this.git(root, ['worktree', 'add', '--detach', entry.path, entry.commit])
            await this.updateRegistry((r) => {
              r.archives.find((a) => a.id === entry.id)!.archived = false
              r.managed[entry.path] = repository
            })
            return { path: entry.path, name: entry.name }
          }
          default:
            throw new Error('Unsupported Git operation')
        }
        return this.status(root)
      })
    this.writes.set(repository, pending)
    try {
      return await pending
    } finally {
      if (this.writes.get(repository) === pending) this.writes.delete(repository)
    }
  }
  private paths(paths: string[]): void {
    if (
      !Array.isArray(paths) ||
      !paths.length ||
      paths.length > 1000 ||
      paths.some(
        (p) =>
          typeof p !== 'string' ||
          !p ||
          p.includes('\0') ||
          isAbsolute(p) ||
          p.split(/[\\/]/).includes('..') ||
          p.split(/[\\/]/).includes('.git')
      )
    )
      throw new Error('Invalid repository path')
  }
  private async archive(
    root: string,
    repository: string,
    path: string
  ): Promise<{ archived: string }> {
    const registry = await this.registry()
    if (
      registry.managed[path] !== repository ||
      path === root ||
      (await this.common(path)) !== repository
    )
      throw new Error('Only another worktree created by Anda can be archived.')
    if (
      (await this.git(path, ['ls-files', '--others', '--ignored', '--exclude-standard', '-z']))
        .length
    )
      throw new Error('This worktree has ignored files. Move them to a backup before archiving.')
    const before = await this.scan(path)
    if (
      (await this.git(path, ['ls-files', '--stage']))
        .split('\n')
        .some((s) => s.startsWith('160000 '))
    )
      throw new Error('Submodule worktrees cannot be archived.')
    for (const file of before.files.filter((f) => f.index === '?')) {
      if ((await lstat(join(path, file.path))).isDirectory())
        throw new Error('Nested repositories cannot be archived.')
    }
    const temporary = await mkdtemp(join(tmpdir(), 'anda-git-index-'))
    try {
      const env = {
        GIT_INDEX_FILE: join(temporary, 'index'),
        GIT_AUTHOR_NAME: 'Anda archive',
        GIT_AUTHOR_EMAIL: 'archive@localhost',
        GIT_COMMITTER_NAME: 'Anda archive',
        GIT_COMMITTER_EMAIL: 'archive@localhost'
      }
      const head = (await this.git(path, ['rev-parse', 'HEAD'])).trim()
      await this.git(path, ['read-tree', head], env)
      await this.git(path, ['add', '-A', '--', '.'], env)
      const tree = (await this.git(path, ['write-tree'], env)).trim()
      const commit = (
        await this.git(path, ['commit-tree', tree, '-p', head, '-m', 'Anda worktree archive'], env)
      ).trim()
      const id = randomUUID()
      await this.git(root, ['update-ref', `refs/anda/archives/${id}`, commit])
      const entry: Archive = {
        id,
        path,
        repository,
        commit,
        name: (await this.git(path, ['branch', '--show-current'])).trim() || head.slice(0, 8),
        time: Date.now(),
        archived: true
      }
      // Persist recoverability before removing anything. A failed remove leaves
      // the snapshot available and the original checkout intact.
      await this.updateRegistry((r) => {
        r.archives.push(entry)
      })
      if (before.revision !== (await this.scan(path)).revision)
        throw new Error('Worktree changed during archive; snapshot kept, checkout preserved.')
      await this.git(root, ['worktree', 'remove', '--force', path])
      await this.updateRegistry((r) => {
        delete r.managed[path]
      })
      return { archived: id }
    } finally {
      await rm(temporary, { recursive: true, force: true })
    }
  }
}
