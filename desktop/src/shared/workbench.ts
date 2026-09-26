export interface GitFile {
  path: string
  previousPath?: string
  index: string
  worktree: string
}
export interface GitSnapshot {
  root: string
  head: string
  branch: string
  revision: string
  files: GitFile[]
  branches: string[]
  log: Array<{ hash: string; subject: string }>
  worktrees: Array<{ path: string; branch: string; head: string }>
  archives: Array<{ id: string; name: string; time: number }>
}
export type GitRequest =
  | { action: 'status'; workspace: string }
  | { action: 'diff'; workspace: string; path: string; staged: boolean }
  | { action: 'stage' | 'unstage'; workspace: string; paths: string[]; revision: string }
  | { action: 'commit'; workspace: string; message: string; revision: string }
  | { action: 'worktree-create'; workspace: string; branch: string; base: string; revision: string }
  | { action: 'worktree-archive'; workspace: string; path: string; revision: string }
  | { action: 'worktree-restore'; workspace: string; id: string; revision: string }
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
