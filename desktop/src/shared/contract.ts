import type { GitRequest, TerminalRequest } from './workbench'
import type { BrowserRequest, BrowserState } from './browser'
import type { SubmissionReceipt } from './app-protocol'
export type Theme = 'system' | 'light' | 'dark'
export type ApprovalMode = 'request_approval' | 'on_risk' | 'full_access' | 'custom'
export interface ChatEntry {
  source: string
  title: string
  workspace?: string
  pinned?: boolean
  archived?: boolean
  updatedAt: number
  readAt?: number
}
export interface ProjectEntry {
  id: string
  path: string
  name: string
}
export interface Preferences {
  theme: Theme
  language: string
  submitKeyMode: 'enter' | 'modifier-enter'
  approvalMode: ApprovalMode
  notifications: boolean
  launchAtLogin: boolean
  chats: ChatEntry[]
  projects: ProjectEntry[]
  activeSource?: string
  drafts: Record<string, string>
}
export interface DaemonView {
  connected: boolean
  home: string
  baseUrl: string
  binary: string | null
  managed: boolean
  error?: string
  version?: string
  desktopProtocol?: number
  liveEvents?: boolean
  runtimeOwnership?: 'managed' | 'external' | 'unknown'
}
export interface Bootstrap {
  daemon: DaemonView
  preferences: Preferences
  platform: string
  version: string
  pending: PendingSubmission[]
}
export interface PendingSubmission {
  id: string
  source: string
  prompt: string
  time: number
  state: 'sending' | 'unknown' | 'completed' | 'failed'
  receipt?: boolean
}
export interface NativeEvent {
  type:
    | 'navigate'
    | 'connection'
    | 'menu'
    | 'update'
    | 'submissions'
    | 'state'
    | 'terminal'
    | 'browser'
  value?: unknown
}
export interface DesktopBridge {
  browser(request: BrowserRequest): Promise<BrowserState>
  git<Result>(request: GitRequest): Promise<Result>
  terminal<Result>(request: TerminalRequest): Promise<Result>
  bootstrap(): Promise<Bootstrap>
  connect(): Promise<DaemonView>
  control(action: 'stop' | 'restart'): Promise<DaemonView>
  rpc<Result>(method: string, params: unknown[], submissionId?: string): Promise<Result>
  config<Result>(
    method: 'GET' | 'PUT',
    content?: string,
    expectedRevision?: string
  ): Promise<Result>
  preferences(patch: Partial<Preferences>): Promise<Preferences>
  storageGet(keys: string[]): Promise<Record<string, unknown>>
  storageSet(items: Record<string, unknown>): Promise<void>
  chooseWorkspace(): Promise<string | null>
  chooseBinary(): Promise<DaemonView>
  notify(source: string, title: string, body: string): Promise<void>
  acknowledgeSubmission(id: string): Promise<void>
  readSubmission(id: string): Promise<SubmissionReceipt | null>
  openExternal(url: string): Promise<void>
  showLogs(): Promise<void>
  printHtml(html: string): Promise<void>
  checkUpdate(): Promise<string>
  onEvent(listener: (event: NativeEvent) => void): () => void
}
