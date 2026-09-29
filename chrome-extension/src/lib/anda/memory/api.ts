import { daemonRpc, getClientPlatform } from '../client/platform'
import type { SettingsState } from '$lib/service-worker/types'

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
  /** The subject Concept, for opening its entity page. */
  subject_id?: string | null
  /** The object Concept; none when the object is a literal value. */
  object_id?: string | null
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

export interface MemoryEntity {
  id: string
  name: string
  /** The Concept type's local name, such as `Person`. */
  type: string
  about_owner: boolean
  /** `self` or `system` for the Brain's own actors, `$self` and `$system`. */
  actor?: 'self' | 'system' | null
}
/** The Proposition's projection under the standard recall policy.
 * `excluded_reason` says why this claim itself did not count, e.g.
 * `outside_valid_time` once a newer statement took over. */
export interface MemoryBelief {
  status: 'accepted' | 'rejected' | 'contested' | 'uncertain' | 'insufficient' | string
  excluded_reason: string | null
}
export interface EntityClaim {
  /** `outgoing` when the entity is the subject, `incoming` when the object. */
  direction: 'outgoing' | 'incoming'
  /** The other end; a literal value has no id. */
  other: { id: string | null; label: string }
  belief: MemoryBelief | null
  record: MemoryRecord
}
export interface EntityPage {
  schema_version: number
  entity: MemoryEntity
  items: EntityClaim[]
  complete: boolean
  partial_reason: string | null
  next_cursor: string | null
}
export interface EntitySearchPage {
  schema_version: number
  items: MemoryEntity[]
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
  constructor(private settings: SettingsState) {}
  async overview(signal?: AbortSignal): Promise<Overview> {
    return unwrap(await this.read<Overview>('memory_overview', [], signal))
  }
  async setupPreview(): Promise<SetupPreview> {
    return unwrap(await this.read<SetupPreview>('memory_inbox_setup_prepare', []))
  }
  async setupCommit(preview_digest: string): Promise<SetupPreview> {
    return unwrap(await this.read<SetupPreview>('memory_inbox_setup_commit', [{ preview_digest }]))
  }
  async search(query: string): Promise<SearchResult> {
    return unwrap(await this.read<SearchResult>('memory_search', [{ query }]))
  }
  async watches(signal?: AbortSignal): Promise<WatchPage> {
    const items = new Map<string, RecordWatch>()
    const cursors = new Set<string>()
    let cursor: string | null = null
    let partial: string | null = null
    while (true) {
      const envelope: Envelope<WatchPage> = await this.read<WatchPage>(
        'memory_watches',
        cursor ? [{ cursor, limit: 50 }] : [],
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
      await this.read<{ schema_version: number; watch: RecordWatch }>('memory_watch', [
        { operation_id, record_id }
      ])
    ).watch
  }
  async cancelWatch(id: string): Promise<RecordWatch> {
    return unwrap(
      await this.read<{ schema_version: number; watch: RecordWatch }>('memory_watch_cancel', [id])
    ).watch
  }
  async prepareChange(input: ChangeInput): Promise<ChangeView> {
    return unwrap(await this.read<ChangeView>('memory_change_prepare', [input]))
  }
  async commitChange(id: string, preview_digest: string): Promise<ChangeView> {
    return unwrap(await this.read<ChangeView>('memory_change_commit', [id, { preview_digest }]))
  }
  async changeStatus(id: string): Promise<ChangeView> {
    return unwrap(await this.read<ChangeView>('memory_change_status', [id]))
  }
  async discardChange(id: string): Promise<void> {
    unwrap(
      await this.read<{ schema_version: number; discarded: boolean }>('memory_change_discard', [id])
    )
  }

  async record(id: string): Promise<MemoryRecord> {
    return unwrap(
      await this.read<{ schema_version: number; record: MemoryRecord }>('memory_record', [id])
    ).record
  }
  async records(cursor: string | null = null, signal?: AbortSignal): Promise<RecordPage> {
    const envelope = await this.read<RecordPage>('memory_records', [{ cursor, limit: 20 }], signal)
    return { ...unwrap(envelope), next_cursor: envelope.next_cursor ?? null }
  }
  /** One entity's claims, newest first. A null id is the caller. */
  async entity(
    id: string | null,
    cursor: string | null = null,
    signal?: AbortSignal
  ): Promise<EntityPage> {
    const envelope = await this.read<EntityPage>(
      'memory_entity',
      [{ id, cursor, limit: 20 }],
      signal
    )
    return { ...unwrap(envelope), next_cursor: envelope.next_cursor ?? null }
  }
  async entitySearch(query: string, signal?: AbortSignal): Promise<EntitySearchPage> {
    return unwrap(await this.read<EntitySearchPage>('memory_entity_search', [{ query }], signal))
  }
  async activity(
    cursor: string | null = null,
    signal?: AbortSignal,
    conversation: string | null = null
  ): Promise<ActivityPage> {
    const envelope = await this.read<ActivityPage>(
      'memory_activity',
      [{ conversation, cursor, limit: 20 }],
      signal
    )
    return { ...unwrap(envelope), next_cursor: envelope.next_cursor ?? null }
  }
  /**
   * One daemon RPC under the caller's own identity. A failed call is never
   * retried over another transport, and a cancelled read is not sent.
   */
  private async read<T>(
    method: string,
    params: unknown[],
    signal?: AbortSignal
  ): Promise<Envelope<T>> {
    if (!getClientPlatform() && !this.settings.token) throw new Error('unauthorized')
    signal?.throwIfAborted()
    return daemonRpc<Envelope<T>>(method, params, this.settings)
  }
}
