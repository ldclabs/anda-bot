import { getMessage } from '$lib/i18n'
import { bytesToBase64 } from '$lib/utils/base64'
import { formatFileSize } from '$lib/utils/format'
import type { ChatAttachment, McpResourceAttachment, Resource } from '../client/types'

/**
 * The largest file a message can carry. The bytes travel base64-encoded in one
 * WebSocket message (64 MiB at most) and the daemon reads media up to 10 MiB,
 * so a bigger file is refused here with a reason instead of dropping the
 * connection.
 */
export const maxAttachmentBytes = 20 * 1024 * 1024

export async function fileToAttachment(file: File): Promise<ChatAttachment> {
  if (file.size > maxAttachmentBytes) {
    throw new Error(
      getMessage('attachmentTooLarge', [file.name, formatFileSize(maxAttachmentBytes)]) ||
        `${file.name} is larger than ${formatFileSize(maxAttachmentBytes)}.`
    )
  }
  const blob = bytesToBase64(new Uint8Array(await file.arrayBuffer()))
  const extension = file.name.includes('.') ? file.name.split('.').pop()?.toLowerCase() : ''
  const primaryType = file.type.includes('/') ? file.type.split('/')[0] : ''
  const tags = Array.from(
    new Set(
      [primaryType, extension, isTextLike(file.type, extension) ? 'text' : ''].filter(
        Boolean
      ) as string[]
    )
  )
  const resource: Resource = {
    _id: 0,
    tags,
    name: file.name,
    mime_type: file.type || undefined,
    blob,
    size: file.size,
    metadata: {
      source: file.webkitRelativePath || 'chrome_extension',
      last_modified: file.lastModified
    }
  }
  return {
    id: `${file.name}-${file.size}-${file.lastModified}`,
    name: file.name,
    type: file.type,
    size: file.size,
    resource
  }
}

/** A resource an MCP server returned, attached like a file the user picked. */
export function mcpResourceToAttachment(
  serverId: string,
  item: McpResourceAttachment
): ChatAttachment {
  const mimeType = item.mime_type || ''
  const extension = item.name.includes('.') ? item.name.split('.').pop()?.toLowerCase() : ''
  const primaryType = mimeType.includes('/') ? mimeType.split('/')[0] : ''
  const tags = Array.from(
    new Set(
      [primaryType, extension, item.text || isTextLike(mimeType, extension) ? 'text' : ''].filter(
        Boolean
      ) as string[]
    )
  )
  const resource: Resource = {
    _id: 0,
    tags,
    name: item.name,
    uri: item.uri,
    mime_type: mimeType || undefined,
    blob: item.blob,
    size: item.size,
    metadata: { source: `mcp:${serverId}` }
  }
  return {
    id: `mcp-${serverId}-${item.uri}`,
    name: item.name,
    type: mimeType,
    size: item.size,
    resource
  }
}

function isTextLike(mimeType: string, extension: string | undefined): boolean {
  return (
    mimeType.startsWith('text/') ||
    ['md', 'markdown', 'txt', 'json', 'csv', 'ts', 'js', 'rs', 'py', 'html', 'css'].includes(
      extension || ''
    )
  )
}
