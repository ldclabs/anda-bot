<script lang="ts">
	import { Button } from '$lib/components/ui/button';
	import type { LandingCopy, OsKey } from '$lib/content/landing';
	import { ArrowRight, CheckCircle, Copy, Monitor, Sparkles, Terminal } from '@lucide/svelte';

	let { copy, detectedOs }: { copy: LandingCopy['install']; detectedOs: OsKey | null } = $props();

	const installOrder: OsKey[] = ['windows', 'macos', 'linux'];
	const osIcons = { windows: Monitor, macos: Sparkles, linux: Terminal } as const;

	let chosenOs = $state<OsKey | null>(null);
	let copyState = $state<'idle' | 'copied' | 'failed'>('idle');
	let copyResetTimer: ReturnType<typeof setTimeout> | undefined;
	let activeOs = $derived<OsKey>(chosenOs ?? detectedOs ?? 'windows');
	let active = $derived(copy.options[activeOs]);

	$effect(() => () => {
		if (copyResetTimer) clearTimeout(copyResetTimer);
	});

	function selectOs(os: OsKey) {
		chosenOs = os;
		if (copyResetTimer) clearTimeout(copyResetTimer);
		copyState = 'idle';
	}

	function resetCopyFeedback(state: typeof copyState) {
		copyState = state;
		if (copyResetTimer) clearTimeout(copyResetTimer);
		copyResetTimer = setTimeout(() => {
			copyState = 'idle';
		}, 1800);
	}

	function copyWithFallback(text: string) {
		const textarea = document.createElement('textarea');
		textarea.value = text;
		textarea.setAttribute('readonly', '');
		textarea.style.position = 'fixed';
		textarea.style.opacity = '0';
		document.body.appendChild(textarea);
		textarea.select();
		const copied = document.execCommand('copy');
		document.body.removeChild(textarea);
		if (!copied) throw new Error('copy failed');
	}

	async function copyCommand() {
		if (!active.command) return;
		try {
			if (navigator.clipboard?.writeText) {
				await navigator.clipboard.writeText(active.command);
			} else {
				copyWithFallback(active.command);
			}
			resetCopyFeedback('copied');
		} catch {
			resetCopyFeedback('failed');
		}
	}

	function feedback(fallback: string) {
		return copyState === 'copied'
			? copy.copied
			: copyState === 'failed'
				? copy.copyFailed
				: fallback;
	}
</script>

<div class="console">
	<i class="ta lead-lane" data-thread="79.5"></i>
	<i class="ta a-top" data-thread="80"></i>
	<i
		class="ta a-side"
		data-thread="81"
		data-thread-dwell="150"
		data-thread-pose="type"
		data-thread-side="start"
		data-thread-station="install"
	></i>
	<i class="ta a-bottom" data-thread="82"></i>
	<div class="tabs" role="group" aria-label={copy.osLabel}>
		{#each installOrder as os}
			{@const Icon = osIcons[os]}
			<button type="button" aria-pressed={activeOs === os} onclick={() => selectOs(os)}>
				<Icon class="size-4" />
				<span>{copy.options[os].label}</span>
				{#if detectedOs === os}
					<small>{copy.detected.replace('{os}', copy.options[os].label)}</small>
				{/if}
			</button>
		{/each}
	</div>

	<div class="route" data-station="install">
		<span class="sr-only" role="status">{feedback('')}</span>
		<h3>{active.title}</h3>
		<p class="body">{active.body}</p>

		<ol class="steps">
			{#each active.steps as step, index}
				<li style="--k: {index}"><span aria-hidden="true">{index + 1}</span>{step}</li>
			{/each}
		</ol>

		{#if active.command}
			<div class="command">
				<div class="command-bar">
					<span><Terminal class="size-4" />{active.commandLabel}</span>
					<button type="button" aria-label={copy.copyAria} onclick={() => void copyCommand()}>
						{#if copyState === 'copied'}<CheckCircle class="size-3.5" />{:else}<Copy
								class="size-3.5"
							/>{/if}
						{feedback(copy.copy)}
					</button>
				</div>
				{#key activeOs}
					<pre dir="ltr"><code
							><span class="prompt" aria-hidden="true">$ </span>{active.command}</code
						></pre>
				{/key}
			</div>
		{/if}

		<div class="actions">
			{#if active.href}
				<Button
					href={active.href}
					target={active.download ? undefined : '_blank'}
					rel={active.download ? undefined : 'noreferrer'}
					download={active.download}
					size="lg"
				>
					<ArrowRight class="size-4" />
					{active.primaryLabel}
				</Button>
			{:else}
				<Button type="button" size="lg" onclick={() => void copyCommand()}>
					<Copy class="size-4" />
					{feedback(active.primaryLabel)}
				</Button>
			{/if}
			<p>{active.note}</p>
		</div>
	</div>
</div>

<style>
	.console {
		position: relative;
		isolation: isolate;
		overflow: hidden;
		padding: clamp(1rem, 2.4vw, 1.6rem);
		border-radius: 1.6rem;
		/* the resting colour of the 1px ring left around .console::after */
		background: rgb(255 255 255 / 0.1);
		box-shadow: 0 60px 140px -50px rgb(0 0 0 / 0.8);
	}
	/* A slow light circling the border: a rotating conic layer (composited, so it
	   never repaints) seen through the 1px ring. */
	.console::before {
		position: absolute;
		inset: -50%;
		z-index: -2;
		content: '';
		background: conic-gradient(
			transparent 0deg,
			rgb(255 208 138 / 0.75) 40deg,
			transparent 90deg,
			transparent 360deg
		);
		animation: border-spin 9s linear infinite;
	}
	.console::after {
		position: absolute;
		inset: 1px;
		z-index: -1;
		border-radius: calc(1.6rem - 1px);
		content: '';
		background:
			radial-gradient(90% 60% at 0% 0%, rgb(241 166 78 / 0.13), transparent 70%),
			linear-gradient(180deg, rgb(20 38 33), rgb(8 18 16));
		box-shadow: inset 0 1px 0 rgb(255 255 255 / 0.08);
	}
	/* single-column layouts: arrive down the margin lane, clear of the copy */
	.lead-lane {
		display: none;
		top: -2.5rem;
		inset-inline-start: var(--lane-in);
	}
	@media (max-width: 1023px) {
		.lead-lane {
			display: block;
		}
	}
	/* Anda runs down the console's outer edge, never across its content. */
	.a-top {
		top: 0;
		inset-inline-start: -1.5rem;
	}
	.a-side {
		top: 58%;
		inset-inline-start: 0;
	}
	.a-bottom {
		top: 100%;
		inset-inline-start: -1.5rem;
	}
	.tabs {
		display: grid;
		grid-template-columns: repeat(3, minmax(0, 1fr));
		gap: 0.4rem;
		padding: 0.3rem;
		border-radius: 1rem;
		background: rgb(0 0 0 / 0.28);
	}
	.tabs button {
		display: inline-flex;
		min-height: 2.8rem;
		min-width: 0;
		align-items: center;
		justify-content: center;
		gap: 0.45rem;
		border-radius: 0.75rem;
		color: rgb(244 238 227 / 0.66);
		font-size: 0.9rem;
		font-weight: 650;
		transition:
			background 180ms ease,
			color 180ms ease;
	}
	.tabs button:hover {
		color: #fff;
	}
	.tabs button[aria-pressed='true'] {
		background: rgb(241 166 78 / 0.16);
		box-shadow: inset 0 0 0 1px rgb(241 166 78 / 0.45);
		color: #fff;
	}
	.tabs small {
		border-radius: 999px;
		background: rgb(53 178 171 / 0.15);
		padding: 0.1rem 0.4rem;
		color: #35b2ab;
		font-size: 0.62rem;
		font-weight: 800;
		white-space: nowrap;
	}
	.route {
		padding: 1.4rem 0.4rem 0.2rem;
		overflow-wrap: anywhere;
	}
	h3 {
		color: #fff;
		font-family: var(--serif);
		font-size: 1.6rem;
		line-height: 1.25;
	}
	.body {
		margin-top: 0.6rem;
		color: rgb(244 238 227 / 0.66);
		line-height: 1.7;
	}
	.steps {
		display: grid;
		grid-template-columns: repeat(3, minmax(0, 1fr));
		gap: 0.6rem;
		margin-top: 1.4rem;
		counter-reset: step;
	}
	.steps li {
		--on: clamp(0, calc((var(--arrive, 1) - var(--k) * 0.22) * 4), 1);
		display: flex;
		min-width: 0;
		align-items: center;
		gap: 0.55rem;
		padding: 0.7rem 0.75rem;
		border: 1px solid rgb(255 255 255 / 0.09);
		border-radius: 0.8rem;
		background: rgb(255 255 255 / calc(0.03 + var(--on) * 0.03));
		color: rgb(244 238 227 / calc(0.5 + var(--on) * 0.35));
		font-size: 0.88rem;
		font-weight: 600;
	}
	.steps span {
		display: grid;
		width: 1.5rem;
		aspect-ratio: 1;
		flex-shrink: 0;
		place-items: center;
		border-radius: 50%;
		background: rgb(241 166 78 / calc(0.1 + var(--on) * 0.9));
		color: #1a1206;
		font-family: var(--mono);
		font-size: 0.72rem;
	}
	.command {
		margin-top: 1.1rem;
		overflow: hidden;
		border: 1px solid rgb(255 255 255 / 0.1);
		border-radius: 0.9rem;
		background: #06100e;
	}
	.command-bar {
		display: flex;
		align-items: center;
		justify-content: space-between;
		gap: 0.75rem;
		padding: 0.6rem 0.9rem;
		border-bottom: 1px solid rgb(255 255 255 / 0.08);
		color: rgb(244 238 227 / 0.55);
		font-size: 0.82rem;
	}
	.command-bar span {
		display: inline-flex;
		min-width: 0;
		align-items: center;
		gap: 0.45rem;
	}
	.command-bar :global(svg) {
		color: #35b2ab;
	}
	.command-bar button {
		display: inline-flex;
		min-width: max-content;
		align-items: center;
		gap: 0.35rem;
		border: 1px solid rgb(255 255 255 / 0.12);
		border-radius: 0.55rem;
		padding: 0.25rem 0.55rem;
		color: rgb(244 238 227 / 0.8);
		font-size: 0.74rem;
		font-weight: 700;
	}
	.command-bar button:hover {
		border-color: rgb(255 208 138 / 0.35);
		color: #ffd08a;
	}
	pre {
		padding: 1rem 1.1rem 1.1rem;
		color: #ffd08a;
		font-family: var(--mono);
		font-size: clamp(0.86rem, 1.4vw, 1rem);
		line-height: 1.75;
		text-align: left;
		white-space: pre-wrap;
		overflow-wrap: anywhere;
		animation: type-in 900ms steps(40) both;
	}
	:global(.thread-ready) pre {
		opacity: calc(0.3 + min(1, var(--arrive, 1) * 1.5) * 0.7);
	}
	.prompt {
		color: #35b2ab;
	}
	.actions {
		display: flex;
		flex-direction: column;
		gap: 0.9rem;
		margin-top: 1.2rem;
	}
	.actions :global([data-slot='button']) {
		height: auto;
		min-height: 2.9rem;
		align-self: flex-start;
		white-space: normal;
	}
	.actions p {
		color: rgb(244 238 227 / 0.62);
		font-size: 0.88rem;
		line-height: 1.6;
	}
	@keyframes border-spin {
		to {
			rotate: 360deg;
		}
	}
	@keyframes type-in {
		from {
			clip-path: inset(0 100% 0 0);
		}
	}
	@media (max-width: 640px) {
		/* tall and narrow: the rotating layer must still cover every corner */
		.console::before {
			inset: -100%;
		}
		.tabs {
			grid-template-columns: 1fr;
		}
		.tabs button {
			justify-content: flex-start;
			padding-inline: 0.8rem;
		}
		.steps {
			grid-template-columns: 1fr;
		}
	}
</style>
