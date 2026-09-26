import { readFile, writeFile, mkdir, rename } from 'node:fs/promises'
import { dirname } from 'node:path'
import { randomUUID } from 'node:crypto'
import type { PendingSubmission, Preferences } from '../shared/contract'

export const defaultPreferences: Preferences = {
  theme: 'system',
  language: '',
  submitKeyMode: 'enter',
  approvalMode: 'on_risk',
  notifications: true,
  launchAtLogin: false,
  chats: [],
  projects: [],
  drafts: {}
}
interface StoredState {
  daemonStopped?: boolean
  updateIntent?: {
    previous: string
    target: string
    managed: boolean
    wasRunning: boolean
    startedAt: number
  }
  preferences: Preferences
  storage: Record<string, unknown>
  pending: PendingSubmission[]
  binary?: string
  windowBounds?: { x: number; y: number; width: number; height: number }
}
/** One serialized, atomic writer for desktop metadata. No daemon credentials. */
export class DesktopStore {
  state: StoredState = {
    preferences: structuredClone(defaultPreferences),
    storage: {},
    pending: []
  }
  private writes: Promise<void> = Promise.resolve()
  constructor(private path: string) {}
  async load(): Promise<void> {
    try {
      const raw = JSON.parse(await readFile(this.path, 'utf8')) as Partial<StoredState>
      this.state = {
        daemonStopped: raw.daemonStopped,
        updateIntent: raw.updateIntent,
        preferences: { ...defaultPreferences, ...raw.preferences },
        storage: raw.storage || {},
        pending: (raw.pending || []).map((p) => ({ ...p, state: 'unknown' })),
        binary: raw.binary,
        windowBounds: raw.windowBounds
      }
    } catch (error) {
      if ((error as NodeJS.ErrnoException).code !== 'ENOENT')
        throw new Error('Desktop settings could not be read; the original file has been preserved.')
    }
  }
  save(): Promise<void> {
    const content = JSON.stringify(this.state, null, 2)
    if (Buffer.byteLength(content) > 8 * 1024 * 1024)
      return Promise.reject(new Error('Desktop settings are too large'))
    const write = this.writes
      .catch(() => {})
      .then(async () => {
        await mkdir(dirname(this.path), { recursive: true, mode: 0o700 })
        const temporary = `${this.path}.${randomUUID()}.tmp`
        await writeFile(temporary, content, { mode: 0o600 })
        await rename(temporary, this.path)
      })
    this.writes = write
    return write
  }
}
