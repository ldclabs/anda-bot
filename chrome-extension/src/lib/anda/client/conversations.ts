import { getMessage } from '$lib/i18n'
import type {
  ActionApiOutput,
  ChatAttachment,
  ChatAction,
  ChatActionChoice,
  ChatActionDetail,
  ChatMessage,
  ContentPart,
  Conversation,
  ExternalUserMessageInfo,
  Json,
  Message,
  MessageGroup,
  Resource
} from './types'

export function conversationToGroup(conversation: Conversation): MessageGroup {
  const messages = (conversation.messages || []).flatMap((message, index) =>
    normalizeMessages(message, {
      conversation: conversation._id,
      index,
      fallbackTimestamp: conversation.updated_at
    })
  )

  if (conversation.status === 'failed') {
    const text = failureReasonMessage(conversation.failed_reason)
    messages.push({
      id: `m-${conversation._id}-failed`,
      conversation: conversation._id,
      role: 'system',
      text,
      timestamp: conversation.updated_at || Date.now()
    })
  }

  return {
    _id: conversation._id,
    status: conversation.status,
    ancestors: conversation.ancestors || [],
    createdAt: conversation.created_at,
    updatedAt: conversation.updated_at,
    messages,
    current: false
  }
}

export function applyActionResponseToGroups(
  groups: MessageGroup[],
  output: ActionApiOutput,
  fallbackRespondedAt = Date.now()
): MessageGroup[] {
  let changed = false
  const nextGroups = groups.map((group) => {
    if (output.conversation && group._id !== output.conversation) {
      return group
    }

    let groupChanged = false
    const messages = group.messages.map((message) => {
      if (!message.actions?.some((action) => action.id === output.action_id)) {
        return message
      }

      groupChanged = true
      return {
        ...message,
        actions: message.actions.map((action) =>
          action.id === output.action_id
            ? {
                ...action,
                status: output.status,
                response: output.response,
                respondedAt: output.responded_at || fallbackRespondedAt
              }
            : action
        )
      }
    })

    if (!groupChanged) {
      return group
    }
    changed = true
    return {
      ...group,
      updatedAt: Math.max(group.updatedAt || 0, output.responded_at || fallbackRespondedAt),
      messages
    }
  })

  return changed ? nextGroups : groups
}

export function failureReasonMessage(reason?: string | null): string {
  const trimmed = reason?.trim()
  return trimmed
    ? getMessage('conversationFailed', [trimmed])
    : getMessage('conversationFailedNoReason')
}

export function normalizeMessage(
  raw: Message,
  context: { conversation: number; index: number; fallbackTimestamp?: number }
): ChatMessage | null {
  return normalizeMessages(raw, context)[0] || null
}

export function normalizeMessages(
  raw: Message,
  context: { conversation: number; index: number; fallbackTimestamp?: number }
): ChatMessage[] {
  const attachments = resourcesToAttachments(messageResources(raw))
  const content = contentToMessageContent(raw.content, {
    hideSystemRuntimeText: Boolean(raw.name?.startsWith('$') && attachments.length > 0)
  })
  const externalUser = content.externalUser || externalUserFromName(raw.name)
  if (
    raw.name?.startsWith('$') &&
    attachments.length === 0 &&
    content.actions.length === 0 &&
    !externalUser &&
    !content.runtimeToolText
  ) {
    return []
  }
  if (
    !content.text &&
    !content.thinkingText &&
    !content.runtimeToolText &&
    attachments.length === 0 &&
    content.actions.length === 0
  ) {
    return []
  }
  const timestamp = raw.timestamp || context.fallbackTimestamp || Date.now()
  // The id must stay stable across poll ticks: `fallbackTimestamp` follows
  // `conversation.updated_at`, so embedding it would regenerate every message
  // id on each delta and force keyed re-renders, scroll jumps, and lost
  // expanded-details state.
  const baseId = `m-${context.conversation}-${context.index}`
  const messages: ChatMessage[] = []
  if (content.text || content.thinkingText || attachments.length > 0 || content.actions.length > 0) {
    messages.push({
      id: baseId,
      conversation: context.conversation,
      role: externalUser ? 'external_user' : raw.role,
      text: content.text,
      externalUser,
      thinkingText: content.thinkingText,
      attachments: attachments.length ? attachments : undefined,
      actions: content.actions.length ? content.actions : undefined,
      timestamp
    })
  }

  if (content.runtimeToolText) {
    messages.push({
      id: `${baseId}-tool`,
      conversation: context.conversation,
      role: 'tool',
      text: '',
      thinkingText: content.runtimeToolText,
      timestamp
    })
  }

  return messages
}

function contentToMessageContent(
  content: ContentPart[] | string,
  options: { hideSystemRuntimeText?: boolean } = {}
): {
  text: string
  thinkingText: string
  runtimeToolText?: string
  externalUser?: ExternalUserMessageInfo
  actions: ChatAction[]
} {
  if (typeof content === 'string') {
    const externalUserMessage = parseExternalUserPrompt(content)
    if (externalUserMessage) {
      return {
        text: externalUserMessage.body,
        thinkingText: '',
        externalUser: externalUserMessage.externalUser,
        actions: []
      }
    }
    if (shouldHideTextPart(content, options)) {
      return { text: '', thinkingText: '', actions: [] }
    }
    if (isSystemRuntimeText(content)) {
      return { text: '', thinkingText: '', runtimeToolText: content.trim(), actions: [] }
    }
    return { ...splitLegacyThoughtText(content), actions: [] }
  }
  if (!Array.isArray(content)) {
    return { text: '', thinkingText: '', actions: [] }
  }
  const textParts: string[] = []
  const thinkingParts: string[] = []
  const runtimeToolParts: string[] = []
  const actions: ChatAction[] = []
  let externalUser: ExternalUserMessageInfo | undefined
  for (const part of content) {
    if (typeof part === 'string') {
      const text = part as string
      const externalUserMessage = parseExternalUserPrompt(text)
      if (externalUserMessage) {
        externalUser ||= externalUserMessage.externalUser
        if (externalUserMessage.body) {
          textParts.push(externalUserMessage.body)
        }
        continue
      }
      if (shouldHideTextPart(text, options)) {
        continue
      }
      if (isSystemRuntimeText(text)) {
        runtimeToolParts.push(text.trim())
        continue
      }
      const split = splitLegacyThoughtText(text)
      if (split.text) {
        textParts.push(split.text)
      }
      if (split.thinkingText) {
        thinkingParts.push(split.thinkingText)
      }
      continue
    }

    switch (part.type) {
      case 'Text':
        const externalUserMessage = parseExternalUserPrompt(part.text)
        if (externalUserMessage) {
          externalUser ||= externalUserMessage.externalUser
          if (externalUserMessage.body) {
            textParts.push(externalUserMessage.body)
          }
          continue
        }
        if (shouldHideTextPart(part.text, options)) {
          continue
        }
        if (isSystemRuntimeText(part.text)) {
          runtimeToolParts.push(part.text.trim())
          continue
        }
        const split = splitLegacyThoughtText(part.text)
        if (split.text) {
          textParts.push(split.text)
        }
        if (split.thinkingText) {
          thinkingParts.push(split.thinkingText)
        }

        continue
      case 'Reasoning':
        thinkingParts.push(part.text)
        continue
      case 'ToolOutput':
        thinkingParts.push(formatToolDetail('Tool output', part.output))
        continue
      case 'ToolCall':
        thinkingParts.push(
          formatToolDetail(`Tool call${part.name ? `: ${part.name}` : ''}`, part.args)
        )
        continue
      case 'Action':
        actions.push(normalizeAction(part))
        continue
      case 'Resource':
        continue
      default:
        continue
    }
  }

  return {
    text: textParts.filter(Boolean).join('\n\n').trim(),
    thinkingText: thinkingParts.filter(Boolean).join('\n\n').trim(),
    runtimeToolText: runtimeToolParts.filter(Boolean).join('\n\n---\n\n').trim(),
    externalUser,
    actions
  }
}

function normalizeAction(part: Extract<ContentPart, { type: 'Action' }>): ChatAction {
  const payload = part.payload || {}
  const id = stringFromJson(payload.id) || part.name
  return {
    id,
    name: part.name,
    kind: stringFromJson(payload.kind),
    status: stringFromJson(payload.status) || 'pending',
    tool: toolFromJson(payload.tool),
    title: stringFromJson(payload.title),
    message: nullableStringFromJson(payload.message),
    summary: stringFromJson(payload.summary),
    details: detailsFromJson(payload.details),
    approval: approvalFromJson(payload.approval),
    command: stringFromJson(payload.command),
    workspace: stringFromJson(payload.workspace),
    background: booleanFromJson(payload.background),
    choices: choicesFromJson(payload.choices),
    response: payload.response,
    createdAt: numberFromJson(payload.created_at),
    expiresAt: numberFromJson(payload.expires_at),
    respondedAt: numberFromJson(payload.responded_at),
    payload
  }
}

function toolFromJson(value: unknown): ChatAction['tool'] {
  if (typeof value === 'string') {
    return { name: value }
  }
  if (!value || typeof value !== 'object' || Array.isArray(value)) {
    return undefined
  }
  const raw = value as Record<string, unknown>
  const name = stringFromJson(raw.name)
  if (!name) {
    return undefined
  }
  return {
    name,
    label: nullableStringFromJson(raw.label)
  }
}

function detailsFromJson(value: unknown): ChatActionDetail[] | undefined {
  if (!Array.isArray(value)) {
    return undefined
  }
  const details: ChatActionDetail[] = []
  for (const item of value) {
    if (!item || typeof item !== 'object' || Array.isArray(item)) {
      continue
    }
    const raw = item as Record<string, unknown>
    const label = stringFromJson(raw.label)
    if (!label) {
      continue
    }
    details.push({
      label,
      value: jsonFromUnknown(raw.value),
      format: nullableStringFromJson(raw.format)
    })
  }
  return details.length ? details : undefined
}

function approvalFromJson(value: unknown): ChatAction['approval'] {
  if (!value || typeof value !== 'object' || Array.isArray(value)) {
    return undefined
  }
  const raw = value as Record<string, unknown>
  return {
    approveLabel: nullableStringFromJson(raw.approve_label ?? raw.approveLabel),
    denyLabel: nullableStringFromJson(raw.deny_label ?? raw.denyLabel)
  }
}

function choicesFromJson(value: unknown): ChatActionChoice[] | undefined {
  if (!Array.isArray(value)) {
    return undefined
  }
  const choices: ChatActionChoice[] = []
  for (const item of value) {
    if (!item || typeof item !== 'object' || Array.isArray(item)) {
      continue
    }
    const raw = item as Record<string, unknown>
    const id = stringFromJson(raw.id)
    const label = stringFromJson(raw.label)
    if (!id || !label) {
      continue
    }
    choices.push({
      id,
      label,
      value: nullableStringFromJson(raw.value),
      description: nullableStringFromJson(raw.description)
    })
  }
  return choices.length ? choices : undefined
}

function stringFromJson(value: unknown): string | undefined {
  return typeof value === 'string' && value.trim() ? value : undefined
}

function nullableStringFromJson(value: unknown): string | null | undefined {
  if (value === null) {
    return null
  }
  return stringFromJson(value)
}

function numberFromJson(value: unknown): number | undefined {
  return typeof value === 'number' && Number.isFinite(value) ? value : undefined
}

function booleanFromJson(value: unknown): boolean | undefined {
  return typeof value === 'boolean' ? value : undefined
}

function jsonFromUnknown(value: unknown): Json {
  if (
    value === null ||
    typeof value === 'string' ||
    typeof value === 'number' ||
    typeof value === 'boolean'
  ) {
    return value
  }
  if (Array.isArray(value)) {
    return value.map(jsonFromUnknown)
  }
  if (value && typeof value === 'object') {
    const output: Record<string, Json> = {}
    for (const [key, nested] of Object.entries(value)) {
      output[key] = jsonFromUnknown(nested)
    }
    return output
  }
  return null
}

function messageResources(raw: Message): Resource[] {
  return resourcesFromContent(raw.content)
}

function resourcesFromContent(content: ContentPart[] | string): Resource[] {
  if (!Array.isArray(content)) {
    return []
  }

  return content
    .map(resourceFromContentPart)
    .filter((resource): resource is Resource => Boolean(resource))
}

function resourceFromContentPart(part: ContentPart): Resource | null {
  if (typeof part === 'string' || part.type !== 'Resource') {
    return null
  }

  const { type: _type, ...resource } = part
  return resource
}

function resourcesToAttachments(resources: Resource[]): ChatAttachment[] {
  return resources
    .map((resource, index) => resourceToAttachment(resource, index))
    .filter((attachment): attachment is ChatAttachment => !!attachment)
}

function resourceToAttachment(resource: Resource, index: number): ChatAttachment | null {
  const name =
    resource.name?.trim() || resource.uri?.trim() || `resource-${resource._id || index + 1}`
  if (!name) {
    return null
  }

  return {
    id: resource._id ? `resource-${resource._id}` : `${name}-${resource.size || 0}-${index}`,
    name,
    type: resource.mime_type,
    size: resource.size,
    resource
  }
}

function shouldHideTextPart(text: string, options: { hideSystemRuntimeText?: boolean }): boolean {
  return Boolean(options.hideSystemRuntimeText && text.trimStart().startsWith('[$system:'))
}

function isSystemRuntimeText(text: string): boolean {
  return text.trimStart().startsWith('[$system:')
}

function parseExternalUserPrompt(
  text: string
): { externalUser: ExternalUserMessageInfo; body: string } | null {
  const trimmed = text.trimStart()
  if (!trimmed.startsWith('[$external_user:')) {
    return null
  }

  const headerEnd = trimmed.indexOf(']')
  if (headerEnd < 0) {
    return null
  }

  const header = trimmed.slice('[$external_user:'.length, headerEnd)
  const bodyText = trimmed.slice(headerEnd + 1).trim()
  return {
    externalUser: parseExternalUserHeader(header),
    body: extractExternalUserBody(bodyText)
  }
}

function parseExternalUserHeader(header: string): ExternalUserMessageInfo {
  const externalUser: ExternalUserMessageInfo = {}
  const fieldPattern = /(\w+)\s*=\s*("(?:\\.|[^"\\])*"|[^,\s]+)/g
  for (const match of header.matchAll(fieldPattern)) {
    const key = match[1]
    const value = decodeQuotedString(match[2] || '').trim()
    if (!value) {
      continue
    }
    if (key === 'channel' || key === 'sender' || key === 'space') {
      externalUser[key] = value
    }
  }
  return externalUser
}

function extractExternalUserBody(text: string): string {
  const separator = text.lastIndexOf('\n\n')
  const body = (separator >= 0 ? text.slice(separator + 2) : text).trim()
  return decodeQuotedString(body).trim()
}

function decodeQuotedString(value: string): string {
  const trimmed = value.trim()
  if (trimmed.length >= 2 && trimmed.startsWith('"') && trimmed.endsWith('"')) {
    try {
      return JSON.parse(trimmed)
    } catch (_error) {
      return trimmed.slice(1, -1).replace(/\\"/g, '"').replace(/\\\\/g, '\\')
    }
  }
  return trimmed
}

function externalUserFromName(name: string | undefined): ExternalUserMessageInfo | undefined {
  const trimmed = name?.trim()
  if (!trimmed?.startsWith('$external_user')) {
    return undefined
  }

  const externalUser: ExternalUserMessageInfo = {}
  const scopeMatch = trimmed.match(/^\$external_user:(.+)$/)
  if (scopeMatch?.[1]) {
    externalUser.scope = decodeQuotedString(scopeMatch[1])
  }
  return externalUser
}

function fencedJson(value: unknown): string {
  if (value === undefined || value === null) {
    return ''
  }
  if (typeof value === 'string') {
    return value
  }
  return `\`\`\`json\n${JSON.stringify(value, null, 2)}\n\`\`\``
}

function formatToolDetail(title: string, value: unknown): string {
  const body = fencedJson(value)
  return body ? `**${title}**\n\n${body}` : `**${title}**`
}

export function splitLegacyThoughtText(content: string): { text: string; thinkingText: string } {
  const thinkingParts: string[] = []
  const text = content
    .replace(/<think(?:ing)?\b[^>]*>([\s\S]*?)<\/think(?:ing)?>/gi, (_match, thinking) => {
      if (typeof thinking === 'string' && thinking.trim()) {
        thinkingParts.push(thinking.trim())
      }
      return ''
    })
    .trim()
  return { text, thinkingText: thinkingParts.join('\n\n').trim() }
}
