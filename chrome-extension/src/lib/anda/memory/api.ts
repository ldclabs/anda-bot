import type { BrainGraphSettings } from '../brain/api'
import { normalizeSettings } from '$lib/service-worker/settings'

export type ReadState =
  | 'reachable'
  | 'available'
  | 'not_configured'
  | 'unauthorized'
  | 'forbidden'
  | 'unavailable'
  | 'timeout'
export interface Overview {
  learning?: { state: string; next_step: string }
  caller?: string | null
  schema_version: number
  observed_at: number
  memory: {
    state: ReadState
    formation_active: boolean | null
    maintenance_active: boolean | null
    reason: string | null
  }
  inbox: {
    state: ReadState
    visible_items: number | null
    inventory_complete: boolean
    reason: string | null
  }
  capabilities: Record<string, { state: string; reason: string | null }>
}
export interface Activity {
  id: string
  conversation: string
  state: string
  submitted_at: number
  last_checked_at: number | null
  stale: boolean
  provenance_complete: boolean
  source_messages: Array<{
    kind: string
    conversation: string | null
    index: string | null
    role: string
    content_digest: string
  }>
}
export interface ActivityPage {
  schema_version: number
  items: Activity[]
  complete: boolean
  partial_reason: string | null
  next_cursor: string | null
}

export interface MemoryRecord {
  id: string
  revision: string
  text: string
  kind: string
  scope: unknown
  effective_at: string | null
  subject_label: string
  predicate_label: string
  object_label: string
  about_owner: boolean
  stance: string
  state: string
  updated_at: string | null
  sources: Array<{
    kind: string
    conversation: string | null
    index: string | null
    role: string
    text: string | null
    text_truncated: boolean
    source: string
  }>
  sources_complete: boolean
  allowed_actions: string[]
}
export interface RecordPage {
  schema_version: number
  items: MemoryRecord[]
  complete: boolean
  partial_reason: string | null
  next_cursor: string | null
}

/** `correct`: the owner's claim was wrong. `world_change`: the world moved
 * on. `misrecorded`: Brain recorded what was never said (recording repair). */
export type ChangeKind = 'correct' | 'world_change' | 'misrecorded' | 'suppress' | 'delete'
export const REVISION_KINDS = ['correct', 'world_change', 'misrecorded'] as const
export interface ChangeInput {
  operation_id: string
  record_id: string
  expected_revision: string
  kind: ChangeKind
  new_value: string | null
}
/** The Memory Interface receipt of a misrecording repair or a deletion. */
export interface MemoryChange {
  receipt_ref: string
  phase: string
  erasure?: {
    status: 'pending' | 'partial' | 'blocked' | 'completed'
    plan_ref: string
    summary: string
    coverage_ref: string
  } | null
}
export interface ChangeView {
  schema_version: number
  operation_id: string
  state: string
  preview_digest: string
  expires_at: number
  kind: ChangeKind
  before: MemoryRecord | null
  new_value: string | null
  targets: string[]
  affected_records: Array<{ id: string; text: string; state: string }>
  excluded_source_count: number
  resets_notes: boolean
  replacement_record: string | null
  error: string | null
  memory?: MemoryChange | null
}
export interface SetupPreview {
  schema_version: number
  preview_digest: string
  state: string
  expires_at: number
  runtime_file: string
  config_file: string
  restart_required: boolean
  changes: Array<{
    path: string
    before: string | null
    after: string
    reformats_config: boolean
    private_backup: boolean
  }>
  managed_runtime: string
}
export interface SearchResult {
  schema_version: number
  packet: string
  incomplete: boolean
  found: boolean
  conversation: string | null
  budget: { tokenizer: string; token_limit: number; tokens: number; context_token_limit: number }
}
export interface RecordWatch {
  operation_id: string
  watch_id: string
  target_id: string
  state: string
}
export interface WatchPage {
  schema_version: number
  items: RecordWatch[]
  complete: boolean
  partial_reason?: string | null
}
interface Envelope<T> {
  result?: T
  error?: { code: string; message: string }
  next_cursor?: string | null
}

export class MemoryApiError extends Error {
  constructor(
    readonly code: string,
    message: string
  ) {
    super(`${code}: ${message}`)
    this.name = 'MemoryApiError'
  }
}

export function unwrap<T extends { schema_version: number }>(envelope: Envelope<T>): T {
  if (envelope.error) throw new MemoryApiError(envelope.error.code, envelope.error.message)
  if (!envelope.result || envelope.result.schema_version !== 1)
    throw new Error('unsupported_memory_api')
  return envelope.result
}

export class MemoryApi {
  constructor(private settings: BrainGraphSettings) {}
  async overview(signal?: AbortSignal): Promise<Overview> {
    return unwrap(await this.read<Overview>('memory_overview', [], '/overview', signal))
  }
  async setupPreview(): Promise<SetupPreview> {
    return unwrap(
      await this.read<SetupPreview>(
        'memory_inbox_setup_prepare',
        [],
        '/inbox/setup/prepare',
        undefined,
        {}
      )
    )
  }
  async setupCommit(preview_digest: string): Promise<SetupPreview> {
    return unwrap(
      await this.read<SetupPreview>(
        'memory_inbox_setup_commit',
        [{ preview_digest }],
        '/inbox/setup/commit',
        undefined,
        { preview_digest }
      )
    )
  }
  async search(query: string): Promise<SearchResult> {
    return unwrap(
      await this.read<SearchResult>('memory_search', [{ query }], '/search', undefined, { query })
    )
  }
  async watches(signal?: AbortSignal): Promise<WatchPage> {
    const items = new Map<string, RecordWatch>()
    const cursors = new Set<string>()
    let cursor: string | null = null
    let partial: string | null = null
    while (true) {
      signal?.throwIfAborted()
      const query = { cursor, limit: 50 }
      const path = cursor ? `/watches?${new URLSearchParams({ cursor, limit: '50' })}` : '/watches'
      const envelope: Envelope<WatchPage> = await this.read<WatchPage>(
        'memory_watches',
        cursor ? [query] : [],
        path,
        signal
      )
      const page = unwrap(envelope)
      partial ||= page.partial_reason || null
      for (const watch of page.items) {
        if (watch.state !== 'cancelled') items.set(watch.operation_id, watch)
      }
      cursor = envelope.next_cursor ?? null
      if (!cursor)
        return {
          schema_version: 1,
          items: [...items.values()],
          complete: page.complete && !partial,
          partial_reason: partial
        }
      if (cursors.has(cursor)) throw new Error('invalid_cursor')
      cursors.add(cursor)
    }
  }
  async watch(operation_id: string, record_id: string): Promise<RecordWatch> {
    return unwrap(
      await this.read<{ schema_version: number; watch: RecordWatch }>(
        'memory_watch',
        [{ operation_id, record_id }],
        '/watches',
        undefined,
        { operation_id, record_id }
      )
    ).watch
  }
  async cancelWatch(id: string): Promise<RecordWatch> {
    return unwrap(
      await this.read<{ schema_version: number; watch: RecordWatch }>(
        'memory_watch_cancel',
        [id],
        `/watches/${encodeURIComponent(id)}/cancel`,
        undefined,
        {}
      )
    ).watch
  }
  async prepareChange(input: ChangeInput): Promise<ChangeView> {
    return unwrap(
      await this.read<ChangeView>(
        'memory_change_prepare',
        [input],
        '/changes/prepare',
        undefined,
        input
      )
    )
  }
  async commitChange(id: string, preview_digest: string): Promise<ChangeView> {
    return unwrap(
      await this.read<ChangeView>(
        'memory_change_commit',
        [id, { preview_digest }],
        `/changes/${encodeURIComponent(id)}/commit`,
        undefined,
        { preview_digest }
      )
    )
  }
  async changeStatus(id: string): Promise<ChangeView> {
    return unwrap(
      await this.read<ChangeView>(
        'memory_change_status',
        [id],
        `/changes/${encodeURIComponent(id)}`
      )
    )
  }
  async discardChange(id: string): Promise<void> {
    unwrap(
      await this.read<{ schema_version: number; discarded: boolean }>(
        'memory_change_discard',
        [id],
        `/changes/${encodeURIComponent(id)}/discard`,
        undefined,
        {}
      )
    )
  }

  async record(id: string): Promise<MemoryRecord> {
    return unwrap(
      await this.read<{ schema_version: number; record: MemoryRecord }>(
        'memory_record',
        [id],
        `/records/${encodeURIComponent(id)}`
      )
    ).record
  }
  async records(cursor: string | null = null, signal?: AbortSignal): Promise<RecordPage> {
    const params = new URLSearchParams({ limit: '20' })
    if (cursor) params.set('cursor', cursor)
    const envelope = await this.read<RecordPage>(
      'memory_records',
      [{ cursor, limit: 20 }],
      `/records?${params}`,
      signal
    )
    return { ...unwrap(envelope), next_cursor: envelope.next_cursor ?? null }
  }
  async activity(
    cursor: string | null = null,
    signal?: AbortSignal,
    conversation: string | null = null
  ): Promise<ActivityPage> {
    const query = { conversation, cursor, limit: 20 }
    const params = new URLSearchParams({ limit: '20' })
    if (cursor) params.set('cursor', cursor)
    if (conversation) params.set('conversation', conversation)
    const envelope = await this.read<ActivityPage>(
      'memory_activity',
      [query],
      `/activity?${params}`,
      signal
    )
    return { ...unwrap(envelope), next_cursor: envelope.next_cursor ?? null }
  }
  private async read<T>(
    method: string,
    params: unknown[],
    path: string,
    signal?: AbortSignal,
    body?: unknown
  ): Promise<Envelope<T>> {
    if (this.settings.spaceId !== 'anda_bot') throw new Error('unsupported_memory_space')
    if (!this.settings.token) throw new Error('unauthorized')
    const extension = typeof chrome !== 'undefined' && chrome.runtime?.sendMessage
    if (extension) {
      // A failed RPC is never retried over HTTP with a different identity.
      const response = await chrome.runtime.sendMessage({
        type: 'anda_rpc',
        settings: normalizeSettings(this.settings),
        method,
        params
      })
      if (!response?.ok) throw new Error(response?.error || 'memory_transport_unavailable')
      return response.result as Envelope<T>
    }
    const response = await fetch(
      `${this.settings.baseUrl.replace(/\/$/, '')}/daemon/memory/v1${path}`,
      {
        headers: {
          Authorization: `Bearer ${this.settings.token}`,
          Accept: 'application/json',
          ...(body === undefined ? {} : { 'Content-Type': 'application/json' })
        },
        signal,
        method: body === undefined ? 'GET' : 'POST',
        ...(body === undefined ? {} : { body: JSON.stringify(body) })
      }
    )
    let envelope: Envelope<T>
    try {
      envelope = (await response.json()) as Envelope<T>
    } catch {
      throw new Error(
        response.status === 404 ? 'unsupported_memory_api' : 'memory_transport_unavailable'
      )
    }
    if (!response.ok && !envelope.error) throw new Error(`memory_api_${response.status}`)
    return envelope
  }
}
