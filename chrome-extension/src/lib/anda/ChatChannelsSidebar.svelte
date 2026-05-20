<script lang="ts">
	import { Button } from '$lib/components/ui/button/index.js'
	import { ChevronDown, CircleAlert, History, LoaderCircle, Radio } from '@lucide/svelte'
	import type { Channel } from './client/channel.svelte'

	type Props = {
		channels: Channel[]
		activeSource: string | null
		sending?: boolean
		onSelect?: (source: string) => void | Promise<void>
	}

	let { channels = [], activeSource = null, sending = false, onSelect }: Props = $props()
	let viewportWidth = $state(0)
	let collapsedOverride = $state<boolean | null>(null)

	const autoCollapsed = $derived(viewportWidth > 0 && viewportWidth < 760)
	const collapsed = $derived(collapsedOverride ?? autoCollapsed)

	function toggleCollapsed() {
		collapsedOverride = !collapsed
	}

	async function selectChannel(source: string) {
		if (source === activeSource) {
			return
		}
		await onSelect?.(source)
	}

	function channelTitle(source: string): string {
		if (source.startsWith('browser:')) {
			const [, scope] = source.split(':')
			return scope === 'incognito' ? 'Incognito' : 'Chrome'
		}
		if (source.startsWith('cli:')) {
			return `CLI ${lastPathPart(source.slice(4)) || source.slice(4)}`.trim()
		}
		const [kind] = source.split(':')
		return titleCase(kind || source)
	}

	function channelSubtitle(source: string): string {
		if (source.startsWith('browser:')) {
			return source.split(':').slice(1).join(':')
		}
		if (source.startsWith('cli:')) {
			return source.slice(4)
		}
		return source
	}

	function channelMeta(channel: Channel): string {
		const parts = []
		if (channel.conversationId) {
			parts.push(`#${channel.conversationId}`)
		}
		if (channel.messageCount > 0) {
			parts.push(String(channel.messageCount))
		}
		return parts.join(' / ')
	}

	function statusLabel(channel: Channel): string {
		if (channel.sending) {
			return 'sending'
		}
		return channel.status
	}

	function statusDotClass(channel: Channel): string {
		const status = statusLabel(channel)
		if (status === 'failed' || status.includes('failed')) {
			return 'bg-amber-500 shadow-[0_0_0_3px_rgba(245,158,11,0.14)]'
		}
		if (['sending', 'submitted', 'working', 'syncing'].includes(status)) {
			return 'animate-pulse bg-emerald-600 shadow-[0_0_0_3px_rgba(5,150,105,0.14)]'
		}
		return 'bg-stone-300'
	}

	function statusIcon(channel: Channel): 'loader' | 'warning' | 'radio' {
		const status = statusLabel(channel)
		if (['sending', 'submitted', 'working', 'syncing'].includes(status)) {
			return 'loader'
		}
		if (status === 'failed' || status.includes('failed')) {
			return 'warning'
		}
		return 'radio'
	}

	function titleCase(value: string): string {
		if (!value) {
			return ''
		}
		return value.charAt(0).toUpperCase() + value.slice(1)
	}

	function lastPathPart(value: string): string {
		return value.split('/').filter(Boolean).pop() || ''
	}
</script>

<svelte:window bind:innerWidth={viewportWidth} />

<aside
	class={`h-full shrink-0 overflow-hidden border-r border-emerald-900/10 bg-emerald-50/70 backdrop-blur transition-[width] duration-200 ${
		collapsed ? 'w-12' : 'w-64'
	}`}
	aria-label={chrome.i18n.getMessage('channelsLabel')}
>
	<div class="flex h-full min-h-0 flex-col">
		<div class="flex h-12 shrink-0 items-center gap-2 border-b border-emerald-900/10 px-1.5">
			<Button
				variant="ghost"
				size="icon-sm"
				class="grid place-items-center bg-white/50 text-emerald-900 hover:bg-white/80"
				aria-label={chrome.i18n.getMessage(collapsed ? 'expandChannels' : 'collapseChannels')}
				title={chrome.i18n.getMessage(collapsed ? 'expandChannels' : 'collapseChannels')}
				onclick={toggleCollapsed}
			>
				<History class="size-4" />
			</Button>

			{#if !collapsed}
				<div class="min-w-0 flex-1">
					<div class="truncate text-xs font-bold text-stone-800">
						{chrome.i18n.getMessage('channelsLabel')}
					</div>
					<div class="truncate text-[10px] font-medium text-stone-500">
						{channels.length}
					</div>
				</div>
				<ChevronDown class="size-4 shrink-0 rotate-90 text-stone-400" />
			{/if}
		</div>

		<div class="scrollbar-slim flex min-h-0 flex-1 flex-col gap-1 overflow-y-auto p-1.5">
			{#each channels as channel (channel.source)}
				{@const active = channel.source === activeSource}
				{@const icon = statusIcon(channel)}
				<button
					type="button"
					class={`relative flex w-full items-center gap-2 rounded-md border px-2 py-2 text-left transition ${
						active
							? 'border-emerald-900/15 bg-white text-stone-950 shadow-sm'
							: 'border-transparent text-stone-600 hover:border-emerald-900/10 hover:bg-white/60 hover:text-stone-900'
					} ${collapsed ? 'h-9 justify-center px-0' : ''}`}
					aria-current={active ? 'page' : undefined}
					aria-label={`${channelTitle(channel.source)} ${statusLabel(channel)}`}
					title={`${channelTitle(channel.source)}\n${channel.source}`}
					onclick={() => selectChannel(channel.source)}
				>
					<span
						class={`relative grid size-6 shrink-0 place-items-center rounded-md border ${
							active
								? 'border-emerald-900/10 bg-emerald-50 text-emerald-800'
								: 'border-stone-200 bg-white/75 text-stone-500'
						}`}
					>
						{#if icon === 'loader'}
							<LoaderCircle class="size-3.5 animate-spin" />
						{:else if icon === 'warning'}
							<CircleAlert class="size-3.5 text-amber-700" />
						{:else}
							<Radio class="size-3.5" />
						{/if}
						<span
							class={`absolute -right-0.5 -bottom-0.5 size-2 rounded-full ${statusDotClass(channel)}`}
						></span>
					</span>

					{#if !collapsed}
						<span class="min-w-0 flex-1">
							<span class="flex min-w-0 items-center gap-2">
								<span class="truncate text-xs font-bold">{channelTitle(channel.source)}</span>
								{#if active && sending}
									<LoaderCircle class="size-3 shrink-0 animate-spin text-emerald-700" />
								{/if}
								{#if channelMeta(channel)}
									<span
										class="shrink-0 rounded-full bg-stone-100 px-1.5 py-0.5 text-[10px] leading-none font-bold text-stone-500"
									>
										{channelMeta(channel)}
									</span>
								{/if}
							</span>
							<span class="mt-0.5 flex min-w-0 items-center gap-1.5 text-[10px] text-stone-500">
								<span class="shrink-0">{statusLabel(channel)}</span>
								<span class="min-w-0 truncate text-stone-400"
									>{channelSubtitle(channel.source)}</span
								>
							</span>
						</span>
					{/if}
				</button>
			{/each}
		</div>
	</div>
</aside>
