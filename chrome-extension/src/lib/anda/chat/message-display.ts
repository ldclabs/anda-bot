import type { ChatMessage, ChatToolCall } from '../client/types'

/**
 * Folds an agent turn's raw steps into what the transcript shows, so a turn
 * reads as one flow instead of a card per LLM round:
 *
 * - a tool result is paired back onto the call that made it (by `callId`,
 *   else by tool name), so a call and its output are one row and messages
 *   that only carried results disappear;
 * - a step without prose (only tool calls or reasoning) joins the step before
 *   it, so a run of calls reads as one list under the narration that led to
 *   them.
 *
 * A fold keeps the id of its first message, which stays stable while the turn
 * grows. Source messages are never mutated; folded ones are copies.
 */
export function displayMessages(messages: ChatMessage[]): ChatMessage[] {
  const output: ChatMessage[] = []
  const owned = new Set<number>()
  const pendingCalls: Array<{ index: number; tool: number; callId?: string; name: string }> = []
  let openStep = -1

  const own = (index: number): ChatMessage => {
    if (!owned.has(index)) {
      const message = output[index]!
      output[index] = { ...message, tools: [...(message.tools || [])] }
      owned.add(index)
    }
    return output[index]!
  }

  const registerCalls = (index: number, tools: ChatToolCall[], offset: number) => {
    tools.forEach((tool, position) => {
      if (tool.output === undefined) {
        pendingCalls.push({ index, tool: offset + position, callId: tool.callId, name: tool.name })
      }
    })
  }

  const attachResult = (result: ChatToolCall): boolean => {
    const at = pendingCalls.findIndex((call) =>
      call.callId && result.callId ? call.callId === result.callId : call.name === result.name
    )
    if (at < 0) {
      return false
    }
    const [call] = pendingCalls.splice(at, 1)
    const target = own(call!.index)
    const tools = target.tools!
    tools[call!.tool] = { ...tools[call!.tool]!, output: result.output ?? null }
    return true
  }

  for (const source of messages) {
    let message = source
    if (message.role === 'tool' && message.tools?.length) {
      const unpaired = message.tools.filter((result) => !attachResult(result))
      if (unpaired.length !== message.tools.length) {
        message = { ...message, tools: unpaired.length ? unpaired : undefined }
        if (!message.tools && !hasProse(message) && !message.thinkingText?.trim()) {
          continue
        }
      }
    }

    if (openStep >= 0 && isBareStep(message)) {
      const target = own(openStep)
      const offset = target.tools!.length
      target.tools!.push(...(message.tools || []))
      target.thinkingText = [target.thinkingText?.trim(), message.thinkingText?.trim()]
        .filter(Boolean)
        .join('\n\n')
      target.timestamp = message.timestamp ?? target.timestamp
      registerCalls(openStep, message.tools || [], offset)
      continue
    }

    output.push(message)
    const index = output.length - 1
    registerCalls(index, message.tools || [], 0)
    openStep = isBareStep(message) || (isAssistant(message) && message.tools?.length) ? index : -1
  }

  for (const index of owned) {
    if (!output[index]!.tools!.length) {
      output[index]!.tools = undefined
    }
  }
  return output
}

/** An assistant step whose narration led to tool calls; the turn continues after it. */
export function isProcessStep(message: ChatMessage): boolean {
  return isAssistant(message) && Boolean(message.tools?.length)
}

function isAssistant(message: ChatMessage): boolean {
  return !['user', 'external_user', 'system', 'tool'].includes(message.role)
}

function hasProse(message: ChatMessage): boolean {
  return Boolean(message.text.trim() || message.attachments?.length || message.actions?.length)
}

/** Only tool calls/results or reasoning. Runtime notices (tool role, no calls) stand alone. */
function isBareStep(message: ChatMessage): boolean {
  if (hasProse(message)) {
    return false
  }
  if (message.role === 'tool') {
    return Boolean(message.tools?.length)
  }
  return isAssistant(message) && Boolean(message.tools?.length || message.thinkingText?.trim())
}
