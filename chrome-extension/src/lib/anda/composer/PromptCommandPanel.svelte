<script lang="ts">
	import type { PromptCommandSuggestion } from '$lib/anda/composer/prompt-commands'
	import { tick } from 'svelte'

	let {
		title,
		suggestions,
		activeIndex,
		onApply
	}: {
		title: string
		suggestions: PromptCommandSuggestion[]
		activeIndex: number
		onApply: (suggestion: PromptCommandSuggestion) => void | Promise<void>
	} = $props()

	let listElement: HTMLDivElement | null = $state(null)

	$effect(() => {
		activeIndex
		suggestions.length
		void tick().then(scrollActivePromptCommandIntoView)
	})

	function scrollActivePromptCommandIntoView() {
		const activeOption = listElement?.querySelector<HTMLElement>(
			`[data-prompt-command-index="${activeIndex}"]`
		)
		activeOption?.scrollIntoView({ block: 'nearest' })
	}
</script>

<div class="prompt-command-panel" role="listbox" aria-label={title}>
	<div class="prompt-command-title">{title}</div>
	<div class="prompt-command-list" bind:this={listElement}>
		{#each suggestions as suggestion, index (suggestion.id)}
			{#if suggestion.disabled}
				<div class="prompt-command-status">{suggestion.description}</div>
			{:else}
				<button
					type="button"
					class="prompt-command-option"
					class:active={index === activeIndex}
					data-prompt-command-index={index}
					role="option"
					aria-selected={index === activeIndex}
					onmousedown={(event) => event.preventDefault()}
					onclick={() => void onApply(suggestion)}
				>
					<span class="prompt-command-main">
						<span class="prompt-command-label">{suggestion.label}</span>
						{#if suggestion.detail}
							<span class="prompt-command-detail">{suggestion.detail}</span>
						{/if}
					</span>
					<span class="prompt-command-description">{suggestion.description}</span>
				</button>
			{/if}
		{/each}
	</div>
</div>

<style>
	.prompt-command-panel {
		position: absolute;
		left: 0;
		right: 0;
		bottom: calc(100% + 8px);
		z-index: 30;
		max-height: min(260px, 45vh);
		overflow: hidden;
		border: 1px solid rgba(120, 113, 108, 0.18);
		border-radius: 8px;
		background: rgba(255, 255, 255, 0.98);
		box-shadow:
			0 18px 48px rgba(36, 45, 39, 0.16),
			0 0 0 1px rgba(255, 255, 255, 0.7) inset;
		backdrop-filter: blur(14px);
	}

	.prompt-command-title {
		padding: 7px 9px 5px;
		border-bottom: 1px solid rgba(231, 229, 228, 0.9);
		font-size: 10px;
		font-weight: 700;
		letter-spacing: 0;
		text-transform: uppercase;
		color: #78716c;
	}

	.prompt-command-list {
		max-height: 218px;
		overflow-y: auto;
		padding: 4px;
	}

	.prompt-command-option {
		width: 100%;
		min-width: 0;
		border: 0;
		border-radius: 6px;
		background: transparent;
		padding: 7px 8px;
		text-align: left;
		transition:
			background 140ms ease-out,
			box-shadow 140ms ease-out;
	}

	.prompt-command-option:hover,
	.prompt-command-option.active {
		background: #ecfdf5;
		box-shadow: inset 0 0 0 1px rgba(16, 185, 129, 0.16);
	}

	.prompt-command-main {
		display: flex;
		min-width: 0;
		align-items: center;
		gap: 6px;
	}

	.prompt-command-label {
		flex: 0 0 auto;
		font-family:
			ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, 'Liberation Mono', monospace;
		font-size: 12px;
		font-weight: 750;
		color: #065f46;
	}

	.prompt-command-detail,
	.prompt-command-description {
		min-width: 0;
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}

	.prompt-command-detail {
		font-size: 10px;
		font-weight: 600;
		color: #a16207;
	}

	.prompt-command-description {
		display: block;
		margin-top: 2px;
		font-size: 11px;
		color: #57534e;
	}

	.prompt-command-status {
		padding: 9px 8px;
		font-size: 11px;
		color: #78716c;
	}
</style>
