/**
 * Reading a workspace path out of a directory-backed channel source.
 *
 * Directory-backed sources are `cli:<path>`, `cli:voice:<path>`, or a plain
 * absolute path. Only an absolute path
 * counts as a workspace — POSIX (`/srv/app`), Windows drive (`C:\app`), or UNC
 * (`\\host\share`) — because a relative path would resolve differently in the
 * daemon than it did in the terminal that produced it. Everything here answers
 * '' rather than throwing when the source is not a usable workspace.
 */

export function isAbsoluteWorkspacePath(value: string): boolean {
  return value.startsWith('/') || /^[A-Za-z]:[\\/]/.test(value) || value.startsWith('\\\\')
}

/** Normalizes an absolute path, dropping trailing separators. '' if relative. */
export function normalizeAbsoluteWorkspace(value: unknown): string {
  const trimmed = String(value || '').trim()
  if (!isAbsoluteWorkspacePath(trimmed)) {
    return ''
  }

  let normalized = trimmed
  // Keep the root itself ('/' and 'C:\') intact.
  while (
    normalized.length > 1 &&
    /[\\/]$/.test(normalized) &&
    normalized !== '/' &&
    !/^[A-Za-z]:[\\/]$/.test(normalized)
  ) {
    normalized = normalized.slice(0, -1)
  }
  return normalized
}

/** The workspace a directory-backed channel source points at, or ''. */
export function workspaceFromCliSource(source: string): string {
  const trimmed = source.trim()
  if (isAbsoluteWorkspacePath(trimmed)) {
    return normalizeAbsoluteWorkspace(trimmed)
  }
  if (!trimmed.startsWith('cli:')) {
    return ''
  }
  const raw = trimmed.slice('cli:'.length).trim()
  return normalizeAbsoluteWorkspace(raw.startsWith('voice:') ? raw.slice('voice:'.length) : raw)
}

/** Re-normalizes a stored source, preserving its `cli:` / `cli:voice:` prefix. */
export function normalizeWorkspaceChannelSource(source: string): string {
  const trimmed = source.trim()
  if (!trimmed.startsWith('cli:')) {
    return ''
  }
  if (trimmed.startsWith('cli:voice:')) {
    const workspace = normalizeAbsoluteWorkspace(trimmed.slice('cli:voice:'.length))
    return workspace ? `cli:voice:${workspace}` : ''
  }
  const workspace = normalizeAbsoluteWorkspace(trimmed.slice('cli:'.length))
  return workspace ? `cli:${workspace}` : ''
}
