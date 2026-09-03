import type { Resource, RpcOutput, ToolOutput } from './types'

/**
 * The daemon transport, as seen by the feature modules built on top of it.
 *
 * This is the seam that keeps skills, bookmarks, and everything else off the
 * `chrome.*` surface: the side panel client is the production adapter (it
 * tunnels calls through the service worker's WebSocket), while tests supply a
 * three-line object. A module that takes a `DaemonApi` never needs a fake
 * `chrome` global.
 *
 * Both verbs reject when the daemon reports an error, so callers only handle
 * the success shape. `authorized` is false until a token is configured; calling
 * `rpc` or `toolCall` before then throws.
 */
export interface DaemonApi {
  readonly authorized: boolean

  /** Invokes a daemon JSON-RPC method with positional arguments. */
  rpc<Result>(method: string, tupleArgs: unknown[]): Promise<Result>

  /** Invokes a daemon tool, rejecting when the tool reports an error. */
  toolCall<Result>(
    name: string,
    args: Record<string, unknown>,
    resources?: Resource[],
    meta?: Record<string, unknown>
  ): Promise<ToolOutput<Result>>
}

/**
 * Unwraps the `{ output: { result } }` envelope every `*_api` daemon tool
 * returns. Feature modules call this instead of repeating the destructuring at
 * every call site.
 */
export async function apiResult<Result>(
  daemon: DaemonApi,
  name: string,
  args: Record<string, unknown>
): Promise<Result> {
  const {
    output: { result }
  } = await daemon.toolCall<RpcOutput<Result>>(name, args)
  return result
}

/** Same as `apiResult`, but guarantees an array even when the daemon sends null. */
export async function apiResultList<Item>(
  daemon: DaemonApi,
  name: string,
  args: Record<string, unknown>
): Promise<Item[]> {
  const result = await apiResult<Item[] | null | undefined>(daemon, name, args)
  return Array.isArray(result) ? result : []
}

/** Unwraps a cursor-paged `*_api` list into its items plus the next cursor. */
export async function apiResultPage<Item>(
  daemon: DaemonApi,
  name: string,
  args: Record<string, unknown>
): Promise<{ items: Item[]; nextCursor: string | null }> {
  const {
    output: { result, next_cursor }
  } = await daemon.toolCall<RpcOutput<Item[]>>(name, args)
  return { items: result || [], nextCursor: next_cursor || null }
}
