export interface GitFile {
  path: string
  previousPath?: string
  index: string
  worktree: string
  submodule?: boolean
}
/** Lines added and removed against HEAD; `null` for a binary file. */
export interface GitLineStats {
  additions: number | null
  deletions: number | null
}
export interface GitSnapshot {
  root: string
  head: string
  branch: string
  revision: string
  files: GitFile[]
  /** Per-path line counts of tracked changes (staged and unstaged together). */
  stats: Record<string, GitLineStats>
  branches: string[]
  log: Array<{ hash: string; subject: string }>
  worktrees: Array<{ path: string; branch: string; head: string }>
  archives: Array<{ id: string; name: string; time: number }>
}
/** What the chat header shows about a workspace folder. */
export interface GitBranchInfo {
  repository: boolean
  branch: string
}
export type GitRequest =
  | { action: 'status'; workspace: string }
  | { action: 'branch'; workspace: string }
  | { action: 'diff'; workspace: string; path: string; staged: boolean }
  | { action: 'stage' | 'unstage'; workspace: string; paths: string[]; revision: string }
  | { action: 'commit'; workspace: string; message: string; revision: string }
  | { action: 'worktree-create'; workspace: string; branch: string; base: string; revision: string }
  | { action: 'worktree-archive'; workspace: string; path: string; revision: string }
  | { action: 'worktree-restore'; workspace: string; id: string; revision: string }
/**
 * A file inside a chat's workspace, named by a path in the transcript. Only
 * reading a bounded text preview and revealing it in the file manager are
 * offered: opening it would launch scripts and application bundles.
 */
export type WorkspaceFileRequest =
  | { action: 'read'; workspace: string; path: string }
  | { action: 'reveal'; workspace: string; path?: string }
export interface WorkspaceFilePreview {
  /** Path relative to the workspace, with `/` separators. */
  path: string
  size: number
  text: string
  binary: boolean
  truncated: boolean
}
export const FILE_PREVIEW_LIMIT = 512 * 1024
export const SCROLLBACK = 512 * 1024
/** Appends terminal output, trimming in bulk so each chunk costs O(chunk), not a scrollback copy. */
export function appendScrollback(output: string, data: string): string {
  output += data
  return output.length > 2 * SCROLLBACK ? output.slice(-SCROLLBACK) : output
}
export interface TerminalSession {
  id: string
  workspace: string
  title: string
  exited?: number
  output: string
  sequence: number
}
export type TerminalRequest =
  | { action: 'list'; workspace: string }
  | { action: 'create'; workspace: string; cols: number; rows: number }
  | { action: 'input'; id: string; data: string }
  | { action: 'resize'; id: string; cols: number; rows: number }
  | { action: 'close'; id: string }
  | { action: 'ack'; id: string; sequence: number }
export interface TerminalEvent {
  id: string
  sequence: number
  data?: string
  exited?: number
}
