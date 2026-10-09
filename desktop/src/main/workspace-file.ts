import { lstat, open, realpath } from 'node:fs/promises'
import { isAbsolute, join, relative, sep } from 'node:path'
import { FILE_PREVIEW_LIMIT, type WorkspaceFilePreview } from '../shared/workbench'

/**
 * Resolves a path a transcript names to a file inside `root`, the chat's
 * authorized workspace (already a real path). Symbolic links are followed
 * first, so a link cannot lead the preview outside the folder.
 */
export async function resolveWorkspacePath(root: string, path: string): Promise<string> {
  if (typeof path !== 'string' || !path || path.length > 4096 || path.includes('\0'))
    throw new Error('Invalid file path')
  let target: string
  try {
    target = await realpath(isAbsolute(path) ? path : join(root, path))
  } catch {
    throw new Error('This file was not found in the chat’s workspace.')
  }
  const child = relative(root, target)
  if (child.startsWith('..') || isAbsolute(child))
    throw new Error('This file is outside the chat’s workspace.')
  return target
}

/** A bounded text preview; binary files report only their size. */
export async function readWorkspaceFile(root: string, path: string): Promise<WorkspaceFilePreview> {
  const target = await resolveWorkspacePath(root, path)
  const stat = await lstat(target)
  if (!stat.isFile()) throw new Error('Only files can be previewed.')
  const length = Math.min(stat.size, FILE_PREVIEW_LIMIT)
  const buffer = Buffer.alloc(length)
  const handle = await open(target, 'r')
  try {
    await handle.read(buffer, 0, length, 0)
  } finally {
    await handle.close()
  }
  const binary = buffer.includes(0)
  return {
    path: relative(root, target).split(sep).join('/'),
    size: stat.size,
    text: binary ? '' : buffer.toString('utf8'),
    binary,
    truncated: stat.size > length
  }
}
