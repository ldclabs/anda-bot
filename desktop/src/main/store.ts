import { readFile, writeFile, mkdir, rename } from 'node:fs/promises'
import { dirname } from 'node:path'
import { randomUUID } from 'node:crypto'
import { defaultPreferences, type PendingSubmission, type Preferences } from '../shared/contract'

interface StoredState {
  daemonStopped?: boolean
  updateIntent?: {
    previous: string
    target: string
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
      const preferences = { ...defaultPreferences, ...raw.preferences }
      // Drafts of never-sent chats cannot be reopened after a restart.
      const reachable = new Set([
        ...preferences.chats.map((c) => c.source),
        preferences.activeSource
      ])
      preferences.drafts = Object.fromEntries(
        Object.entries(preferences.drafts || {}).filter(
          ([source, text]) => text && reachable.has(source)
        )
      )
      this.state = {
        daemonStopped: raw.daemonStopped,
        updateIntent: raw.updateIntent,
        preferences,
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
    const content = JSON.stringify(this.state)
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
