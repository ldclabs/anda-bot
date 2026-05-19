import type { ChatAttachment, Resource } from '../client/types'

export function fileSizeLabel(size: number): string {
	if (size < 1024) {
		return `${size} B`
	}
	if (size < 1024 * 1024) {
		return `${(size / 1024).toFixed(1)} KB`
	}
	return `${(size / 1024 / 1024).toFixed(1)} MB`
}

export async function fileToAttachment(file: File): Promise<ChatAttachment> {
	const blob = arrayBufferToBase64(await file.arrayBuffer())
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

function isTextLike(mimeType: string, extension: string | undefined): boolean {
	return (
		mimeType.startsWith('text/') ||
		['md', 'markdown', 'txt', 'json', 'csv', 'ts', 'js', 'rs', 'py', 'html', 'css'].includes(
			extension || ''
		)
	)
}

function arrayBufferToBase64(buffer: ArrayBuffer): string {
	const bytes = new Uint8Array(buffer)
	const chunkSize = 0x8000
	let binary = ''
	for (let index = 0; index < bytes.length; index += chunkSize) {
		binary += String.fromCharCode(...bytes.subarray(index, index + chunkSize))
	}
	return btoa(binary)
}
