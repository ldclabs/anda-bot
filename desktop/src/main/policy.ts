import { isAbsolute, relative, resolve } from 'node:path'

export function rendererAssetPath(root: string, raw: string): string {
  const url = new URL(raw)
  if (
    url.protocol !== 'anda-app:' ||
    url.hostname !== 'app' ||
    url.port ||
    url.username ||
    url.password
  )
    throw new Error('Invalid application origin')
  const decoded = decodeURIComponent(url.pathname)
  const segments = decoded.split('/').filter(Boolean)
  const path = resolve(root, ...(segments.length ? segments : ['index.html']))
  const child = relative(root, path)
  if (child.startsWith('..') || isAbsolute(child) || decoded.includes(String.fromCharCode(0)))
    throw new Error('Invalid application asset path')
  return path
}

const rpcMethods = new Set([
  'ping',
  'information',
  'capabilities',
  'model_names',
  'set_model',
  'reload_models',
  'register_workspace',
  'agent_run',
  'tool_call',
  'brain_status',
  'brain_kip_readonly',
  'brain_attention',
  'brain_respond',
  'brain_runtime_status',
  'ui_language'
])
const tools = new Set([
  'anda_bot_api',
  'conversations_api',
  'actions_api',
  'skills_api',
  'bookmarks_api',
  'resources_api',
  'transcribe_audio',
  'synthesize_speech',
  'list_cron_jobs',
  'list_cron_runs',
  'create_cron_job',
  'update_cron_job',
  'manage_cron_job'
])
const memoryMethods = new Set([
  'memory_overview',
  'memory_activity',
  'memory_records',
  'memory_record',
  'memory_search',
  'memory_watches',
  'memory_watch',
  'memory_watch_cancel',
  'memory_inbox_setup_prepare',
  'memory_inbox_setup_commit',
  'memory_change_prepare',
  'memory_change_commit',
  'memory_change_status',
  'memory_change_discard'
])

export function loopbackBaseUrl(raw: string): string {
  const url = new URL(raw)
  if (
    !['http:', 'https:'].includes(url.protocol) ||
    !['localhost', '127.0.0.1', '[::1]'].includes(url.hostname) ||
    url.username ||
    url.password ||
    url.search ||
    url.hash ||
    url.pathname !== '/'
  ) {
    throw new Error('Desktop requires an authenticated local Anda daemon on a loopback address.')
  }
  return url.origin
}

export function validateRpc(method: unknown, params: unknown): asserts params is unknown[] {
  if (typeof method !== 'string' || (!rpcMethods.has(method) && !memoryMethods.has(method)))
    throw new Error('RPC method is not available to the desktop UI')
  if (!Array.isArray(params) || params.length > 8) throw new Error('Invalid RPC arguments')
  if (Buffer.byteLength(JSON.stringify(params)) > 32 * 1024 * 1024)
    throw new Error('Request exceeds 32 MiB')
  if (method === 'tool_call') {
    const input = params[0] as
      | { name?: unknown; args?: unknown; meta?: { source?: unknown } }
      | undefined
    if (
      !input ||
      typeof input.name !== 'string' ||
      !tools.has(input.name) ||
      !input.args ||
      typeof input.args !== 'object'
    )
      throw new Error('Tool is not available to the desktop UI')
    if (input.name === 'actions_api' && String(input.meta?.source || '').includes(':reply_target:'))
      throw new Error('Channel conversations are read-only in the desktop client.')
  }
  if (method === 'agent_run') {
    const input = params[0] as
      | { name?: unknown; prompt?: unknown; meta?: { source?: unknown } }
      | undefined
    if (
      !input ||
      (input.name !== '' && input.name !== undefined) ||
      typeof input.prompt !== 'string'
    )
      throw new Error('Invalid chat submission')
    if (String(input.meta?.source || '').includes(':reply_target:'))
      throw new Error('Channel conversations are read-only. Start a new local chat to respond.')
  }
}

export function externalUrl(raw: unknown): string {
  if (typeof raw !== 'string' || raw.length > 8192) throw new Error('Invalid external URL')
  const url = new URL(raw)
  if (!['https:', 'http:', 'mailto:'].includes(url.protocol) || url.username || url.password)
    throw new Error('URL scheme is not allowed')
  return url.href
}

export function navigationSource(raw: string): string | null {
  try {
    const url = new URL(raw)
    if (url.protocol !== 'anda:' || url.hostname !== 'chat' || url.username || url.password)
      return null
    const source = url.searchParams.get('source')
    return source && source.length <= 2048 ? source : null
  } catch {
    return null
  }
}
/** Only the application's top-level document may request its audio capability. */
export function appPermissionAllowed(
  appId: number,
  requesterId: number | undefined,
  permission: string,
  details: { isMainFrame?: boolean; mediaType?: string }
): boolean {
  if (requesterId !== appId || !details.isMainFrame) return false
  return permission === 'media'
    ? details.mediaType === 'audio'
    : ['speaker-selection', 'clipboard-sanitized-write'].includes(permission)
}
