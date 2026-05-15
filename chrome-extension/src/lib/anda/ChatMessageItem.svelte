<script lang="ts" module>
	const expandedDetailMessageIds = new Set<string>()
</script>

<script lang="ts">
	import type { ChatMessage } from '$lib/anda/client'
	import { renderMarkdown } from '$lib/utils/markdown'
	import { Check, Clipboard, Wrench } from '@lucide/svelte'
	import { onMount, tick } from 'svelte'

	let { message }: { message: ChatMessage } = $props()

	let copied = $state(false)
	let detailsExpanded = $state(false)
	const isUser = $derived(message.role === 'user')
	const isSystem = $derived(message.role === 'system')
	const isTool = $derived(message.role === 'tool')
	const mainText = $derived(message.text.trim())
	const thinkingText = $derived((message.thinkingText || '').trim())
	const hasMainText = $derived(Boolean(mainText))
	const hasThinkingText = $derived(Boolean(thinkingText))
	const messageTimeLabel = $derived(timeLabel(message.timestamp))
	const detailLabel = $derived(isTool ? 'tool output' : 'thinking and tools')
	const [html, hook] = $derived.by(() => renderMarkdown(mainText))
	const [thinkingHtml, thinkingHook] = $derived.by(() => renderMarkdown(thinkingText))

	async function copyMessage() {
		if (!navigator.clipboard || !mainText) {
			return
		}
		await navigator.clipboard.writeText(mainText)
		copied = true
		window.setTimeout(() => {
			copied = false
		}, 1200)
	}

	async function toggleDetails() {
		setDetailsExpanded(!detailsExpanded)
		if (detailsExpanded) {
			await tick()
			thinkingHook()
		}
	}

	function setDetailsExpanded(expanded: boolean) {
		detailsExpanded = expanded
		if (expanded) {
			expandedDetailMessageIds.add(message.id)
			return
		}
		expandedDetailMessageIds.delete(message.id)
	}

	function timeLabel(value: string | number | null | undefined): string {
		if (!value) {
			return ''
		}
		const date = new Date(value)
		if (Number.isNaN(date.getTime())) {
			return ''
		}
		return date.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' })
	}

	function fileSizeLabel(size: number | undefined): string {
		if (!size) {
			return ''
		}
		if (size < 1024) {
			return `${size} B`
		}
		if (size < 1024 * 1024) {
			return `${(size / 1024).toFixed(1)} KB`
		}
		return `${(size / 1024 / 1024).toFixed(1)} MB`
	}

	onMount(() => {
		if (expandedDetailMessageIds.has(message.id)) {
			detailsExpanded = true
		}
		hook()
		thinkingHook()
	})
</script>

<article
	class="group grid w-full gap-1 {isUser
		? 'justify-items-end'
		: isTool || !hasMainText
			? 'justify-items-center'
			: 'justify-items-start'}"
>
	{#if hasThinkingText && (isTool || !hasMainText)}
		<button
			type="button"
			class="inline-flex items-center gap-1.5 rounded-full border border-stone-200 bg-white/70 px-3 py-1 text-[11px] font-semibold text-stone-500 shadow-sm transition hover:border-emerald-200 hover:text-emerald-700"
			onclick={toggleDetails}
		>
			<Wrench class="size-3" />
			<span>{detailsExpanded ? `Hide ${detailLabel}` : `Show ${detailLabel}`}</span>
		</button>
	{/if}

	{#if hasMainText}
		<div
			class="relative max-w-[92%] min-w-0 rounded-lg border px-3 py-2 text-[13px] leading-relaxed shadow-2xs {isUser
				? ' rounded-br-none bg-sky-50 text-slate-950'
				: isSystem
					? 'rounded-bl-none border-amber-200 bg-amber-50 text-amber-950'
					: isTool
						? 'border-stone-200 bg-stone-50 text-stone-800'
						: 'rounded-bl-none border-stone-100 bg-white text-stone-950'}"
		>
			<button
				type="button"
				class="absolute -top-3 -right-3 grid size-7 place-items-center rounded-md border border-stone-200 bg-white text-stone-500 opacity-0 shadow-sm transition group-hover:opacity-100 hover:text-emerald-700"
				aria-label="Copy message"
				title="Copy message"
				onclick={copyMessage}
			>
				{#if copied}
					<Check class="size-3.5" />
				{:else}
					<Clipboard class="size-3.5" />
				{/if}
			</button>

			<div class="md-content w-full min-w-0 text-pretty wrap-break-word">{@html html}</div>

			{#if message.attachments?.length}
				<div class="mt-2 flex flex-wrap gap-1.5">
					{#each message.attachments as attachment (attachment.id)}
						<span
							class="inline-flex max-w-full items-center gap-1 rounded-md border border-stone-200 bg-white/70 px-2 py-1 text-[11px] text-stone-600"
							title={attachment.name}
						>
							<span class="truncate">{attachment.name}</span>
							{#if fileSizeLabel(attachment.size)}
								<span class="shrink-0 text-stone-400">{fileSizeLabel(attachment.size)}</span>
							{/if}
						</span>
					{/each}
				</div>
			{/if}

			{#if hasThinkingText || messageTimeLabel}
				<div
					class="mt-1.5 flex min-h-5 items-center gap-2 {hasThinkingText
						? 'border-t border-stone-200/70 pt-1.5'
						: ''}"
				>
					{#if hasThinkingText}
						<button
							type="button"
							class="inline-flex min-w-0 items-center gap-1.5 rounded-md px-1.5 py-0.5 text-[11px] font-semibold text-stone-400 transition hover:bg-stone-100 hover:text-stone-600"
							onclick={toggleDetails}
						>
							<Wrench class="size-3 shrink-0" />
							<span class="truncate">
								{detailsExpanded ? 'Hide thinking and tools' : 'Show thinking and tools'}
							</span>
						</button>
					{/if}
					{#if messageTimeLabel}
						<div class="ml-auto shrink-0 text-[10px] leading-none text-stone-400">
							{messageTimeLabel}
						</div>
					{/if}
				</div>
			{/if}

			{#if hasThinkingText && detailsExpanded}
				<div
					class="md-content mt-1 w-full min-w-0 text-[12px] leading-relaxed text-pretty wrap-break-word text-stone-500 opacity-80"
				>
					{@html thinkingHtml}
				</div>
			{/if}
		</div>
	{/if}

	{#if hasThinkingText && !hasMainText && detailsExpanded}
		<div
			class="relative max-w-[92%] min-w-0 rounded-lg border border-dashed border-stone-200 bg-stone-50/70 px-3 py-2 text-[12px] leading-relaxed text-stone-500 shadow-2xs"
		>
			<div class="md-content w-full min-w-0 text-pretty wrap-break-word opacity-80">
				{@html thinkingHtml}
			</div>
			{#if messageTimeLabel}
				<div class="mt-1 text-right text-[10px] text-stone-400">{messageTimeLabel}</div>
			{/if}
		</div>
	{/if}
</article>
