import type { DesktopBridge } from '../shared/contract'
declare global {
  interface Window {
    anda: DesktopBridge
  }
}
export {}
