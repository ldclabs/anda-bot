<script lang="ts" module>
	import type { ChatAttachment, ResourceInput } from '$lib/anda/client'

	export interface ComposerSubmitPayload {
		text: string
		attachments: ChatAttachment[]
	}
</script>

<script lang="ts">
	import { Button } from '$lib/components/ui/button/index.js'
	import {
		FileText,
		Keyboard,
		LoaderCircle,
		Mic,
		Paperclip,
		SendHorizontal,
		X
	} from '@lucide/svelte'
	import { tick } from 'svelte'

	let {
		disabled = false,
		sending = false,
		placeholder = 'Message Anda',
		onSend
	}: {
		disabled?: boolean
		sending?: boolean
		placeholder?: string
		onSend: (payload: ComposerSubmitPayload) => Promise<void> | void
	} = $props()

	let text = $state('')
	let attachments = $state<ChatAttachment[]>([])
	let attachmentError = $state('')
	let preparingAttachments = $state(false)
	let inputMode = $state<'text' | 'voice'>('text')
	let textareaElement: HTMLTextAreaElement | null = $state(null)
	let fileInputElement: HTMLInputElement | null = $state(null)

	const canSend = $derived(
		(Boolean(text.trim()) || attachments.length > 0) &&
			!disabled &&
			!sending &&
			!preparingAttachments
	)
	const submitTitle = $derived(
		isMacPlatform() ? 'Send with Command Enter' : 'Send with Control Enter'
	)

	function isSubmitEvent(event: KeyboardEvent): boolean {
		if (event.shiftKey || disabled || sending || preparingAttachments || event.isComposing) {
			return false
		}
		if (event.keyCode === 229) {
			return false
		}
		const isEnter = event.key === 'Enter' || event.code === 'Enter' || event.keyCode === 13
		if (!isEnter) {
			return false
		}
		return isMacPlatform() ? event.metaKey : event.ctrlKey
	}

	async function submitMessage() {
		if (!canSend) {
			return
		}
		const payload: ComposerSubmitPayload = {
			text: text.trim(),
			attachments
		}
		await onSend(payload)
		text = ''
		attachments = []
		attachmentError = ''
		inputMode = 'text'
		await tick()
		resizeTextarea()
	}

	function handleKeydown(event: KeyboardEvent) {
		if (isSubmitEvent(event)) {
			event.preventDefault()
			void submitMessage()
		}
	}

	function resizeTextarea() {
		if (!textareaElement) {
			return
		}
		textareaElement.style.height = 'auto'
		textareaElement.style.height = `${Math.min(textareaElement.scrollHeight, 150)}px`
	}

	function openFileDialog() {
		if (disabled || preparingAttachments) {
			return
		}
		fileInputElement?.click()
	}

	async function handleFileInput(event: Event) {
		const input = event.currentTarget as HTMLInputElement
		await addFiles(input.files)
		input.value = ''
	}

	async function handleDrop(event: DragEvent) {
		event.preventDefault()
		if (disabled) {
			return
		}
		await addFiles(event.dataTransfer?.files || null)
	}

	function handleDragover(event: DragEvent) {
		if (!disabled) {
			event.preventDefault()
		}
	}

	async function addFiles(fileList: FileList | null) {
		if (!fileList || fileList.length === 0) {
			return
		}
		attachmentError = ''
		preparingAttachments = true
		try {
			const nextAttachments: ChatAttachment[] = []
			for (const file of Array.from(fileList)) {
				nextAttachments.push(await fileToAttachment(file))
			}
			const existingIds = new Set(attachments.map((attachment) => attachment.id))
			attachments = [
				...attachments,
				...nextAttachments.filter((attachment) => !existingIds.has(attachment.id))
			]
		} catch (error) {
			attachmentError = error instanceof Error ? error.message : String(error)
		} finally {
			preparingAttachments = false
		}
	}

	function removeAttachment(id: string) {
		attachments = attachments.filter((attachment) => attachment.id !== id)
	}

	function toggleInputMode() {
		inputMode = inputMode === 'text' ? 'voice' : 'text'
		void tick().then(() => textareaElement?.focus())
	}

	function isMacPlatform(): boolean {
		if (typeof navigator === 'undefined') {
			return false
		}
		return /mac|iphone|ipad|ipod/i.test(navigator.platform || navigator.userAgent)
	}

	function fileSizeLabel(size: number): string {
		if (size < 1024) {
			return `${size} B`
		}
		if (size < 1024 * 1024) {
			return `${(size / 1024).toFixed(1)} KB`
		}
		return `${(size / 1024 / 1024).toFixed(1)} MB`
	}

	async function fileToAttachment(file: File): Promise<ChatAttachment> {
		const blob = arrayBufferToBase64(await file.arrayBuffer())
		const extension = file.name.includes('.') ? file.name.split('.').pop()?.toLowerCase() : ''
		const primaryType = file.type.includes('/') ? file.type.split('/')[0] : ''
		const tags = Array.from(
			new Set(
				[primaryType, extension, isTextLike(file.type, extension) ? 'text' : 'file'].filter(
					Boolean
				) as string[]
			)
		)
		const resource: ResourceInput = {
			_id: 0,
			tags,
			name: file.name,
			description: 'Chrome extension attachment',
			mime_type: file.type || undefined,
			blob,
			size: file.size,
			metadata: {
				source: 'chrome_extension',
				last_modified: file.lastModified
			}
		}
		return {
			id: `${file.name}-${file.size}-${file.lastModified}`,
			name: file.name,
			type: file.type || extension || undefined,
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
</script>

<form
	class="rounded-lg border border-stone-200 bg-white p-2 shadow-[0_10px_30px_rgba(36,45,39,0.08)]"
	onsubmit={(event) => {
		event.preventDefault()
		void submitMessage()
	}}
	ondrop={handleDrop}
	ondragover={handleDragover}
>
	<input
		bind:this={fileInputElement}
		type="file"
		multiple
		class="hidden"
		onchange={handleFileInput}
	/>

	{#if attachments.length}
		<div class="mb-2 flex flex-wrap gap-1.5">
			{#each attachments as attachment (attachment.id)}
				<span
					class="inline-flex max-w-full items-center gap-1.5 rounded-md border border-stone-200 bg-stone-50 px-2 py-1 text-[11px] text-stone-600"
					title={attachment.name}
				>
					<FileText class="size-3 shrink-0 text-emerald-700" />
					<span class="truncate">{attachment.name}</span>
					<span class="shrink-0 text-stone-400">{fileSizeLabel(attachment.size || 0)}</span>
					<button
						type="button"
						class="grid size-4 shrink-0 place-items-center rounded-sm text-stone-400 hover:bg-stone-200 hover:text-stone-700"
						aria-label="Remove attachment"
						title="Remove attachment"
						onclick={() => removeAttachment(attachment.id)}
					>
						<X class="size-3" />
					</button>
				</span>
			{/each}
		</div>
	{/if}

	{#if attachmentError}
		<div
			class="mb-2 rounded-md border border-amber-200 bg-amber-50 px-2 py-1 text-[11px] text-amber-800"
		>
			{attachmentError}
		</div>
	{/if}

	<div class="grid gap-2">
		<textarea
			bind:this={textareaElement}
			bind:value={text}
			rows="1"
			placeholder={inputMode === 'voice' ? 'Voice input' : placeholder}
			spellcheck="true"
			disabled={disabled || sending}
			class="max-h-38 min-h-20 w-full resize-none border-0 bg-transparent px-2 py-2 text-[13px] leading-5 text-stone-950 outline-none placeholder:text-stone-400 disabled:cursor-not-allowed disabled:opacity-60"
			onkeydown={handleKeydown}
			oninput={resizeTextarea}
		></textarea>

		<div class="flex items-center justify-between gap-2">
			<div class="flex items-center gap-1">
				<Button
					type="button"
					variant="ghost"
					size="icon-sm"
					class="text-stone-500 hover:text-emerald-700"
					disabled={disabled || preparingAttachments}
					aria-label="Attach files"
					title="Attach files"
					onclick={openFileDialog}
				>
					{#if preparingAttachments}
						<LoaderCircle class="size-4 animate-spin" />
					{:else}
						<Paperclip class="size-4" />
					{/if}
				</Button>

				<Button
					type="button"
					variant={inputMode === 'voice' ? 'secondary' : 'ghost'}
					size="icon-sm"
					class="hidden text-stone-500 hover:text-emerald-700"
					{disabled}
					aria-label={inputMode === 'voice' ? 'Switch to keyboard input' : 'Switch to voice input'}
					title={inputMode === 'voice' ? 'Keyboard input' : 'Voice input'}
					onclick={toggleInputMode}
				>
					{#if inputMode === 'voice'}
						<Keyboard class="size-4" />
					{:else}
						<Mic class="size-4" />
					{/if}
				</Button>
			</div>

			<Button
				type="submit"
				size="icon-sm"
				disabled={!canSend}
				aria-label="Send"
				title={submitTitle}
			>
				{#if sending}
					<LoaderCircle class="size-4 animate-spin" />
				{:else}
					<SendHorizontal class="size-4" />
				{/if}
			</Button>
		</div>
	</div>
</form>
