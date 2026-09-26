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
  state: 'sending' | 'unknown'
}
export interface NativeEvent {
  type: 'navigate' | 'connection' | 'menu' | 'update' | 'submissions'
  value?: unknown
}
export interface DesktopBridge {
  bootstrap(): Promise<Bootstrap>
  connect(): Promise<DaemonView>
  control(action: 'stop' | 'restart'): Promise<DaemonView>
  rpc<Result>(method: string, params: unknown[]): Promise<Result>
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
  openExternal(url: string): Promise<void>
  showLogs(): Promise<void>
  printHtml(html: string): Promise<void>
  checkUpdate(): Promise<string>
  onEvent(listener: (event: NativeEvent) => void): () => void
}
