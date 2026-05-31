import type {
  ChatAttachment,
  ChatMessage,
  ContentPart,
  Conversation,
  Message,
  MessageGroup,
  Resource
} from './types'

export function conversationToGroup(conversation: Conversation): MessageGroup {
  const messages = (conversation.messages || [])
    .map((message, index) =>
      normalizeMessage(message, {
        conversation: conversation._id,
        index,
        fallbackTimestamp: conversation.updated_at
      })
    )
    .filter((message) => !!message)

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
    ? chrome.i18n.getMessage('conversationFailed', [trimmed])
    : chrome.i18n.getMessage('conversationFailedNoReason')
}

export function normalizeMessage(
  raw: Message,
  context: { conversation: number; index: number; fallbackTimestamp?: number }
): ChatMessage | null {
  const attachments = resourcesToAttachments(messageResources(raw))
  if (raw.name?.startsWith('$') && attachments.length === 0) {
    return null
  }
  const content = contentToMessageContent(raw.content, {
    hideSystemRuntimeText: Boolean(raw.name?.startsWith('$') && attachments.length > 0)
  })
  if (!content.text && !content.thinkingText && attachments.length === 0) {
    return null
  }
  const timestamp = raw.timestamp || context.fallbackTimestamp || Date.now()
  return {
    id: `m-${context.conversation}-${context.index}-${timestamp}`,
    conversation: context.conversation,
    role: raw.role,
    text: content.text,
    thinkingText: content.thinkingText,
    attachments: attachments.length ? attachments : undefined,
    timestamp
  }
}

function contentToMessageContent(
  content: ContentPart[],
  options: { hideSystemRuntimeText?: boolean } = {}
): { text: string; thinkingText: string } {
  if (typeof content === 'string') {
    if (shouldHideTextPart(content, options)) {
      return { text: '', thinkingText: '' }
    }
    return splitLegacyThoughtText(content)
  }
  if (!Array.isArray(content)) {
    return { text: '', thinkingText: '' }
  }
  const textParts: string[] = []
  const thinkingParts: string[] = []
  for (const part of content) {
    if (typeof part === 'string') {
      if (shouldHideTextPart(part, options)) {
        continue
      }
      const split = splitLegacyThoughtText(part)
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
        if (shouldHideTextPart(part.text, options)) {
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
    thinkingText: thinkingParts.filter(Boolean).join('\n\n').trim()
  }
}

function messageResources(raw: Message): Resource[] {
  return resourcesFromContent(raw.content)
}

function resourcesFromContent(content: ContentPart[]): Resource[] {
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
