import type { ChatMessage, ContentPart, Conversation, Message, MessageGroup } from './types'

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
		conversation: conversation,
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
	if (raw.name?.startsWith('$')) {
		return null
	}
	const content = contentToMessageContent(raw.content)
	if (!content.text && !content.thinkingText) {
		return null
	}
	return {
		id: `m-${context.conversation}-${context.index}`,
		conversation: context.conversation,
		role: raw.role,
		text: content.text,
		thinkingText: content.thinkingText,
		timestamp: raw.timestamp || context.fallbackTimestamp
	}
}

function contentToMessageContent(content: ContentPart[]): { text: string; thinkingText: string } {
	if (typeof content === 'string') {
		return splitLegacyThoughtText(content)
	}
	if (!Array.isArray(content)) {
		return { text: '', thinkingText: '' }
	}
	const textParts: string[] = []
	const thinkingParts: string[] = []
	for (const part of content) {
		if (typeof part === 'string') {
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
			default:
				continue
		}
	}

	return {
		text: textParts.filter(Boolean).join('\n\n').trim(),
		thinkingText: thinkingParts.filter(Boolean).join('\n\n').trim()
	}
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
