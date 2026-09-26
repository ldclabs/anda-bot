export interface BrowserTab {
  id: number
  title: string
  url: string
  loading: boolean
  canBack: boolean
  canForward: boolean
  error?: string
}
export interface BrowserDownload {
  id: number
  name: string
  received: number
  total: number
  state: string
}
export interface BrowserState {
  source: string
  session: string
  active: number | null
  tabs: BrowserTab[]
  downloads: BrowserDownload[]
}
export type BrowserRequest =
  | { action: 'state' | 'new'; source: string }
  | { action: 'select' | 'close' | 'back' | 'forward' | 'reload'; source: string; id: number }
  | { action: 'navigate'; source: string; id: number; url: string }
  | { action: 'find'; source: string; id: number; text: string }
  | {
      action: 'bounds'
      source: string
      visible: boolean
      x: number
      y: number
      width: number
      height: number
    }
  | { action: 'download-open' | 'download-cancel'; source: string; id: number }
