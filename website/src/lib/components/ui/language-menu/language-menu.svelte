<script lang="ts">
	import { localeMeta, localeOrder, type Locale } from '$lib/content/landing';
	import { cn } from '$lib/utils.js';
	import { Check, ChevronDown, Languages } from '@lucide/svelte';
	import { tick } from 'svelte';

	// Language picker: a menu button with radio items. Colours come from
	// `--menu-fg`, `--menu-panel` and `--menu-accent` so each page can theme it.
	let {
		value,
		label,
		onselect,
		class: className = ''
	}: {
		value: Locale;
		label: string;
		onselect: (locale: Locale) => void;
		class?: string;
	} = $props();

	const id = $props.id();
	let open = $state(false);
	let root: HTMLElement;
	let trigger: HTMLButtonElement;
	let items: HTMLButtonElement[] = $state([]);

	async function show(focus: 'current' | 'first' | 'last' = 'current') {
		open = true;
		await tick();
		const current = Math.max(0, localeOrder.indexOf(value));
		items[focus === 'first' ? 0 : focus === 'last' ? items.length - 1 : current]?.focus();
	}

	function hide(returnFocus = true) {
		if (!open) return;
		open = false;
		if (returnFocus) trigger.focus();
	}

	function choose(locale: Locale) {
		hide();
		if (locale !== value) onselect(locale);
	}

	function handleTriggerKey(event: KeyboardEvent) {
		if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
			event.preventDefault();
			void show(event.key === 'ArrowDown' ? 'current' : 'last');
		}
	}

	function handleMenuKey(event: KeyboardEvent) {
		const index = items.indexOf(document.activeElement as HTMLButtonElement);
		const focusAt = (next: number) => items[(next + items.length) % items.length]?.focus();
		switch (event.key) {
			case 'ArrowDown':
				focusAt(index + 1);
				break;
			case 'ArrowUp':
				focusAt(index - 1);
				break;
			case 'Home':
				focusAt(0);
				break;
			case 'End':
				focusAt(items.length - 1);
				break;
			case 'Escape':
				hide();
				break;
			case 'Tab':
				hide(false);
				return;
			default: {
				// Type-ahead on either the native or the English name.
				if (event.key.length !== 1 || event.metaKey || event.ctrlKey) return;
				const key = event.key.toLowerCase();
				for (let step = 1; step <= items.length; step += 1) {
					const locale = localeOrder[(index + step) % items.length];
					const { nativeName, label } = localeMeta[locale];
					if (nativeName.toLowerCase().startsWith(key) || label.toLowerCase().startsWith(key)) {
						focusAt(index + step);
						break;
					}
				}
				return;
			}
		}
		event.preventDefault();
	}

	function handleFocusOut(event: FocusEvent) {
		if (!root.contains(event.relatedTarget as Node | null)) hide(false);
	}

	$effect(() => {
		if (!open) return;
		const handlePointer = (event: PointerEvent) => {
			if (!root.contains(event.target as Node)) hide(false);
		};
		document.addEventListener('pointerdown', handlePointer, true);
		return () => document.removeEventListener('pointerdown', handlePointer, true);
	});
</script>

<div
	bind:this={root}
	class={cn('language-menu', className)}
	data-open={open || undefined}
	onfocusout={handleFocusOut}
>
	<button
		bind:this={trigger}
		type="button"
		class="trigger"
		aria-label={label}
		aria-haspopup="menu"
		aria-expanded={open}
		aria-controls="{id}-menu"
		onclick={() => (open ? hide() : void show())}
		onkeydown={handleTriggerKey}
	>
		<Languages class="size-4 shrink-0" />
		<span class="name" lang={localeMeta[value].htmlLang}>{localeMeta[value].nativeName}</span>
		<span class="code" aria-hidden="true">{value.toUpperCase()}</span>
		<ChevronDown class="chevron size-3.5 shrink-0" />
	</button>

	<div
		id="{id}-menu"
		class="panel"
		role="menu"
		aria-label={label}
		tabindex="-1"
		onkeydown={handleMenuKey}
	>
		{#each localeOrder as locale, index}
			{@const meta = localeMeta[locale]}
			<button
				bind:this={items[index]}
				type="button"
				role="menuitemradio"
				aria-checked={locale === value}
				tabindex="-1"
				class="item"
				style="--i: {index}"
				onclick={() => choose(locale)}
			>
				<Check class="check size-3.5" />
				<bdi class="native" lang={meta.htmlLang}>{meta.nativeName}</bdi>
				{#if meta.label !== meta.nativeName}<span class="english">{meta.label}</span>{/if}
			</button>
		{/each}
	</div>
</div>

<style>
	.language-menu {
		--fg: var(--menu-fg, #f4eee3);
		--panel: var(--menu-panel, rgb(12 26 22 / 0.97));
		--accent: var(--menu-accent, #ffd08a);
		position: relative;
		color: var(--fg);
	}
	.trigger {
		display: inline-flex;
		height: 2.15rem;
		align-items: center;
		gap: 0.45rem;
		padding-inline: 0.65rem 0.55rem;
		border: 1px solid color-mix(in oklab, var(--fg) 16%, transparent);
		border-radius: 0.7rem;
		background: color-mix(in oklab, var(--fg) 5%, transparent);
		color: inherit;
		font-size: 0.82rem;
		font-weight: 600;
		backdrop-filter: blur(16px);
		transition:
			background 160ms ease,
			border-color 160ms ease;
	}
	.trigger:hover,
	[data-open] .trigger {
		border-color: color-mix(in oklab, var(--fg) 28%, transparent);
		background: color-mix(in oklab, var(--fg) 10%, transparent);
	}
	.trigger:focus-visible {
		outline: 2px solid var(--accent);
		outline-offset: 3px;
	}
	.code {
		display: none;
		font-family: ui-monospace, 'SF Mono', Menlo, monospace;
		font-size: 0.72rem;
	}
	.trigger :global(.chevron) {
		opacity: 0.65;
		transition: rotate 220ms cubic-bezier(0.3, 1.4, 0.5, 1);
	}
	[data-open] .trigger :global(.chevron) {
		rotate: 180deg;
	}

	.panel {
		position: absolute;
		top: calc(100% + 0.45rem);
		inset-inline-end: 0;
		z-index: 60;
		display: grid;
		min-width: 13.5rem;
		padding: 0.35rem;
		border: 1px solid color-mix(in oklab, var(--fg) 14%, transparent);
		border-radius: 0.95rem;
		background: var(--panel);
		box-shadow:
			0 24px 60px -18px rgb(0 0 0 / 0.55),
			0 2px 6px rgb(0 0 0 / 0.12);
		backdrop-filter: blur(22px) saturate(1.3);
		transform-origin: top right;
		opacity: 0;
		scale: 0.94;
		translate: 0 -0.35rem;
		visibility: hidden;
		transition:
			opacity 140ms ease,
			scale 180ms ease,
			translate 180ms ease,
			visibility 0s 180ms;
	}
	:global([dir='rtl']) .panel {
		transform-origin: top left;
	}
	[data-open] .panel {
		opacity: 1;
		scale: 1;
		translate: 0;
		visibility: visible;
		transition:
			opacity 160ms ease,
			scale 260ms cubic-bezier(0.25, 1.4, 0.45, 1),
			translate 260ms cubic-bezier(0.25, 1.4, 0.45, 1),
			visibility 0s;
	}
	.panel:focus {
		outline: none;
	}
	.item {
		display: grid;
		grid-template-columns: 1rem minmax(0, 1fr) auto;
		align-items: center;
		gap: 0.6rem;
		min-height: 2.45rem;
		padding-inline: 0.6rem 0.75rem;
		border-radius: 0.6rem;
		color: color-mix(in oklab, var(--fg) 82%, transparent);
		font-size: 0.9rem;
		text-align: start;
		opacity: 0;
		translate: 0 -0.2rem;
		transition:
			background 120ms ease,
			color 120ms ease,
			opacity 120ms ease,
			translate 120ms ease;
	}
	[data-open] .item {
		opacity: 1;
		translate: 0;
		transition:
			background 120ms ease,
			color 120ms ease,
			opacity 200ms calc(var(--i) * 22ms) ease,
			translate 260ms calc(var(--i) * 22ms) cubic-bezier(0.25, 1.3, 0.45, 1);
	}
	.item:hover,
	.item:focus-visible {
		outline: none;
		background: color-mix(in oklab, var(--fg) 9%, transparent);
		color: var(--fg);
	}
	.item[aria-checked='true'] {
		color: var(--accent);
		font-weight: 650;
	}
	.item :global(.check) {
		opacity: 0;
		scale: 0.5;
		transition:
			opacity 160ms ease,
			scale 260ms cubic-bezier(0.3, 1.8, 0.5, 1);
	}
	.item[aria-checked='true'] :global(.check) {
		opacity: 1;
		scale: 1;
	}
	.native {
		/* a right-to-left name still lines up with the others */
		justify-self: start;
		max-width: 100%;
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}
	.english {
		color: color-mix(in oklab, var(--fg) 45%, transparent);
		font-size: 0.74rem;
		font-weight: 500;
	}
	@media (max-width: 400px) {
		.name {
			display: none;
		}
		.code {
			display: inline;
		}
	}
</style>
