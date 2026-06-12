import { getMessage } from '$lib/i18n'
import type {
  ChatAttachment,
  ChatMessage,
  ContentPart,
  Conversation,
  ExternalUserMessageInfo,
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
    !externalUser &&
    !content.runtimeToolText
  ) {
    return []
  }
  if (
    !content.text &&
    !content.thinkingText &&
    !content.runtimeToolText &&
    attachments.length === 0
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
  if (content.text || content.thinkingText || attachments.length > 0) {
    messages.push({
      id: baseId,
      conversation: context.conversation,
      role: externalUser ? 'external_user' : raw.role,
      text: content.text,
      externalUser,
      thinkingText: content.thinkingText,
      attachments: attachments.length ? attachments : undefined,
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
} {
  if (typeof content === 'string') {
    const externalUserMessage = parseExternalUserPrompt(content)
    if (externalUserMessage) {
      return {
        text: externalUserMessage.body,
        thinkingText: '',
        externalUser: externalUserMessage.externalUser
      }
    }
    if (shouldHideTextPart(content, options)) {
      return { text: '', thinkingText: '' }
    }
    if (isSystemRuntimeText(content)) {
      return { text: '', thinkingText: '', runtimeToolText: content.trim() }
    }
    return splitLegacyThoughtText(content)
  }
  if (!Array.isArray(content)) {
    return { text: '', thinkingText: '' }
  }
  const textParts: string[] = []
  const thinkingParts: string[] = []
  const runtimeToolParts: string[] = []
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
    externalUser
  }
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
