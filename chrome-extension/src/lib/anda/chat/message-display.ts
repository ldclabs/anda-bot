import type { ChatMessage, ChatToolCall } from '../client/types'
import { isRecord, parseJsonString, runtimeNotices } from './tool-view'

export type ActivityDetail = { id: string; messageId: string } & (
  | { kind: 'thought'; text: string }
  | { kind: 'tool'; tool: ChatToolCall }
  | { kind: 'notice'; name: string; text: string }
)

export type TranscriptRow = { id: string; lastMessageId?: string } & (
  { kind: 'message'; message: ChatMessage } | { kind: 'activity'; details: ActivityDetail[] }
)

export type ActivityRow = Extract<TranscriptRow, { kind: 'activity' }>

interface Transcript {
  messages: ChatMessage[]
  rows: TranscriptRow[]
  lastPromptId?: string
}

// A poll replaces the changed group's array. Older conversations retain their
// paired calls, row keys and open disclosures without rebuilding their history.
const cache = new WeakMap<ChatMessage[], Transcript>()

export function chatTranscript(source: ChatMessage[]): Transcript {
  const cached = cache.get(source)
  if (cached) return cached
  const messages = pairToolResults(source)
  const lastPromptId = messages.findLast((m) => m.role === 'user' || m.role === 'external_user')?.id
  const transcript = { messages, rows: transcriptRows(messages), lastPromptId }
  cache.set(source, transcript)
  return transcript
}

/** Use the target for display and keep the outer wire call for result pairing. */
function displayToolCall(call: ChatToolCall): ChatToolCall {
  if (call.name.toLowerCase() !== 'tools_call') return call
  const payload = parseJsonString(call.args)
  if (!isRecord(payload) || typeof payload.name !== 'string') return call
  const name = payload.name.trim()
  if (!name || Object.keys(payload).some((key) => key !== 'name' && key !== 'arguments'))
    return call
  const args = payload.arguments
  if (args == null || (typeof args === 'string' && !args.trim())) return { ...call, name, args: {} }
  if (typeof args === 'string') {
    const parsed = parseJsonString(args)
    return { ...call, name, args: isRecord(parsed) ? parsed : args }
  }
  return isRecord(args) ? { ...call, name, args } : call
}

function pairToolResults(messages: ChatMessage[]): ChatMessage[] {
  const output: ChatMessage[] = []
  const pending: { message: number; index: number; call: ChatToolCall; turn: number }[] = []
  let turn = 0
  for (let message of messages) {
    const tools = message.tools
    if (output.at(-1)?.conversation !== message.conversation) pending.length = 0
    if (message.role === 'user' || message.role === 'external_user') turn++
    if (message.role === 'tool' && tools?.length) {
      const unpaired = tools.filter((result) => {
        // A steering prompt can arrive before an earlier tool result. Its call
        // ID still matches; a name alone is only meaningful within one turn.
        const exact = result.callId
          ? pending.findIndex(({ call }) => call.callId === result.callId)
          : -1
        const at =
          exact >= 0
            ? exact
            : pending.findIndex(
                (item) =>
                  item.turn === turn &&
                  (!item.call.callId || !result.callId) &&
                  item.call.name === result.name
              )
        if (at < 0) return true
        const paired = pending.splice(at, 1)[0]!
        output[paired.message]!.tools![paired.index] = {
          ...output[paired.message]!.tools![paired.index]!,
          output: result.output ?? null
        }
        return false
      })
      if (unpaired.length !== tools.length) {
        if (!unpaired.length && !hasBody(message) && !message.thinkingText?.trim()) continue
        message = { ...message, tools: unpaired }
      }
    } else if (tools?.length) {
      message = { ...message, tools: tools.map(displayToolCall) }
      tools.forEach((call, index) => {
        if (call.output === undefined) pending.push({ message: output.length, index, call, turn })
      })
    }
    output.push(message)
  }
  return output
}

function hasBody(message: ChatMessage): boolean {
  return Boolean(message.text.trim() || message.attachments?.length || message.actions?.length)
}

export function isProcessStep(message: ChatMessage): boolean {
  return message.role === 'assistant' && Boolean(message.tools?.length)
}

/**
 * The work still in progress: while a turn runs, the activity that ends the
 * transcript. Hosts show it on the working line, below the plan, so finished
 * messages, the plan and the running work read from top to bottom. The newest
 * group can be an empty placeholder, so this looks at the last group with rows.
 */
export function liveActivity(
  groups: { rows: TranscriptRow[] }[],
  working: boolean
): ActivityRow | undefined {
  if (!working) return undefined
  const last = groups.findLast((group) => group.rows.length)?.rows.at(-1)
  return last?.kind === 'activity' ? last : undefined
}

function transcriptRows(messages: ChatMessage[]): TranscriptRow[] {
  const rows: TranscriptRow[] = []
  let activity: ActivityRow | undefined
  function add(detail: ActivityDetail) {
    if (!activity) {
      activity = { kind: 'activity', id: detail.id, details: [] }
      rows.push(activity)
    }
    activity.details.push(detail)
  }

  let conversation: number | undefined
  for (const message of messages) {
    if (conversation !== message.conversation) activity = undefined
    conversation = message.conversation
    const { id, role, tools, noticeKind } = message
    const thought = message.thinkingText?.trim()
    if (role !== 'assistant' && role !== 'tool') activity = undefined
    if (thought && role !== 'tool') {
      add({ kind: 'thought', id: id + ':thinking', messageId: id, text: thought })
    }
    if (hasBody(message)) {
      rows.push({ kind: 'message', id, message })
      activity = undefined
    }
    tools?.forEach((tool, index) => {
      add({ kind: 'tool', id: id + ':tool:' + index, messageId: id, tool })
    })
    if (thought && role === 'tool') {
      const notices = noticeKind ? [{ kind: noticeKind, body: thought }] : runtimeNotices(thought)
      notices.forEach((notice, index) => {
        // Other notices can carry recovery instructions. Keep them individually
        // discoverable even between a long series of background updates.
        const background = notice.kind === 'background shell' || notice.kind === 'background task'
        if (!background) activity = undefined
        add({
          kind: 'notice',
          id: id + ':notice:' + index,
          messageId: id,
          name: notice.kind,
          text: notice.body
        })
        if (!background) activity = undefined
      })
    }
    // Edited-file summaries follow the last visible row of their turn.
    const last = rows.at(-1)
    if (last) last.lastMessageId = id
  }
  return rows
}

/** Memory links can point to a tool-only message inside a closed disclosure. */
export function transcriptAnchor(groups: Transcript[], messageId: string): string {
  for (const { messages, rows } of groups) {
    const message = messages.find((item) => item.id === messageId)
    if (!message) continue
    if (hasBody(message)) return messageId
    const activity = rows.find(
      (row) =>
        row.kind === 'activity' && row.details.some((detail) => detail.messageId === messageId)
    )
    return activity?.id ?? messageId
  }
  return messageId
}
