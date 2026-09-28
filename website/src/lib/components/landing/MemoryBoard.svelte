<script lang="ts">
	import type { LandingCopy } from '$lib/content/landing';
	import { Link2, MessageSquareQuote, RefreshCw } from '@lucide/svelte';

	let { copy }: { copy: LandingCopy['demo']['memory'] } = $props();

	const indents = ['0rem', '2.5rem', '1rem', '3.25rem', '1.75rem'];
	const tones = ['warm', 'teal', 'lichen', 'clay', 'teal'];
</script>

<!-- The thread ties a knot at each card, then swings back to revise the decision. -->
<div class="board" aria-hidden="true">
	<i class="ta a-top" data-thread="30"></i>
	{#each copy.items as item, index}
		<div class="slot" style="--indent: {indents[index]}">
			<i
				data-thread={31 + index}
				data-thread-loop="7"
				data-thread-dwell="70"
				data-thread-pose="remember"
				data-thread-side="start"
				data-thread-station={`mem-${index}`}
				class="ta a-knot"
			></i>
			{#if index === 3}
				<i
					data-thread="37"
					data-thread-dir="up"
					data-thread-loop="9"
					data-thread-dwell="140"
					data-thread-pose="revise"
					data-thread-side="end"
					data-thread-station="mem-revise"
					class="ta a-revise"
				></i>
			{/if}
			<article class="card {tones[index]}" data-station={`mem-${index}`}>
				<header>
					<span class="kind">{copy.kinds[index]}</span>
					{#if index === 0}
						<span class="source"><MessageSquareQuote class="size-3" />{copy.source}</span>
					{/if}
					{#if index === 3}
						<span class="updated" data-station="mem-revise"
							><RefreshCw class="size-3" />{copy.updated}</span
						>
					{/if}
				</header>
				{#if index === 3}
					<p class="text revision" data-station="mem-revise">
						<span class="old">{item}</span>
						<span class="new">{copy.revised}</span>
					</p>
				{:else}
					<p class="text">{item}</p>
				{/if}
				{#if index > 0}
					<span class="link" data-station="mem-link"><Link2 class="size-3" />{copy.linked}</span>
					<span class="edge" data-station="mem-link"></span>
				{/if}
			</article>
		</div>
	{/each}
	<span class="rail" data-station="mem-link"></span>
	<i class="ta a-bottom" data-thread="36" data-thread-dir="h" data-thread-station="mem-link"></i>
	<i class="ta a-exit" data-thread="38"></i>
</div>

<style>
	.board {
		--card-w: 19rem;
		--rail: 24rem;
		position: relative;
		display: flex;
		flex-direction: column;
		gap: 2.6rem;
		width: calc(var(--rail) + 1rem);
		max-width: 100%;
		padding-block: 3rem 8rem;
		margin-inline: auto;
	}
	.slot {
		--knot-x: var(--indent);
		position: relative;
		padding-inline-start: var(--knot-x);
	}
	.a-top {
		inset-inline-start: 0;
		top: 0;
	}
	.a-knot {
		inset-inline-start: var(--knot-x);
		top: 1.6rem;
	}
	.a-revise {
		inset-inline-start: calc(var(--knot-x) + min(var(--card-w), 100%));
		top: 60%;
	}
	.a-bottom {
		inset-inline-start: 1.75rem;
		bottom: 4rem;
	}
	.a-exit {
		inset-inline-start: var(--rail);
		bottom: -3rem;
	}
	.card {
		position: relative;
		width: min(var(--card-w), 100%);
		padding: 0.95rem 1.1rem 1.05rem;
		border: 1px solid var(--line);
		border-radius: 0.9rem;
		background: var(--card);
		box-shadow:
			0 1px 0 rgb(255 255 255 / 0.7) inset,
			0 22px 40px -30px rgb(60 40 10 / 0.55);
	}
	:global(.thread-ready) .card {
		opacity: calc(var(--arrive, 0) * 1.4 - 0.1);
		transform: translateX(calc((1 - min(1, var(--arrive, 0) * 1.6)) * -1.4rem))
			rotate(calc((1 - min(1, var(--arrive, 0) * 1.6)) * -2deg));
	}
	:global(.landing[dir='rtl'].thread-ready) .card {
		transform: translateX(calc((1 - min(1, var(--arrive, 0) * 1.6)) * 1.4rem))
			rotate(calc((1 - min(1, var(--arrive, 0) * 1.6)) * 2deg));
	}
	.card::before {
		position: absolute;
		inset-block: 0.8rem;
		inset-inline-start: -1px;
		width: 3px;
		border-radius: 3px;
		content: '';
		background: var(--tone);
	}
	.warm {
		--tone: #c0701c;
	}
	.teal {
		--tone: var(--accent-2);
	}
	.lichen {
		--tone: #6f8f66;
	}
	.clay {
		--tone: var(--accent);
	}
	header {
		display: flex;
		align-items: center;
		gap: 0.5rem;
	}
	.kind {
		color: var(--tone);
		font-family: var(--mono);
		font-size: 0.68rem;
		font-weight: 600;
		letter-spacing: 0.08em;
		text-transform: uppercase;
	}
	.source,
	.updated {
		display: inline-flex;
		align-items: center;
		gap: 0.3rem;
		margin-inline-start: auto;
		color: var(--muted);
		font-size: 0.72rem;
	}
	.updated {
		color: var(--accent);
	}
	:global(.thread-ready) .updated {
		opacity: var(--arrive, 0);
		transform: scale(calc(0.8 + var(--arrive, 0) * 0.2));
	}
	.text {
		margin-top: 0.4rem;
		color: var(--fg);
		font-family: var(--serif);
		font-size: 1.14rem;
		line-height: 1.35;
	}
	.revision {
		display: grid;
	}
	.revision span {
		grid-area: 1 / 1;
	}
	.old {
		text-decoration: line-through;
		text-decoration-color: var(--accent);
	}
	:global(.thread-ready) .old {
		text-decoration-color: color-mix(
			in oklab,
			var(--accent) calc(min(1, var(--arrive, 0) * 3) * 100%),
			transparent
		);
	}
	.new {
		opacity: 0;
	}
	:global(.thread-ready) .revision .old {
		opacity: calc(1 - var(--arrive, 0) * 1.6);
		transform: translateY(calc(var(--arrive, 0) * -0.5rem));
	}
	:global(.thread-ready) .revision .new {
		opacity: calc(var(--arrive, 0) * 1.8 - 0.6);
		transform: translateY(calc((1 - var(--arrive, 0)) * 0.5rem));
	}
	:global(.landing:not(.thread-ready)) .old {
		display: none;
	}
	:global(.landing:not(.thread-ready)) .new {
		opacity: 1;
	}
	.link {
		display: inline-flex;
		align-items: center;
		gap: 0.3rem;
		margin-top: 0.55rem;
		padding: 0.15rem 0.5rem;
		border-radius: 999px;
		background: color-mix(in srgb, var(--accent-2) 12%, transparent);
		color: var(--accent-2);
		font-size: 0.7rem;
		font-weight: 600;
	}
	:global(.thread-ready) .link {
		opacity: calc(var(--arrive, 0) * 3 - 0.4);
		transform: translateY(calc((1 - min(1, var(--arrive, 0) * 2)) * 0.4rem));
	}
	.edge {
		position: absolute;
		top: 50%;
		inset-inline-start: 100%;
		width: calc(5rem - var(--indent));
		height: 1px;
		background: repeating-linear-gradient(90deg, var(--accent-2) 0 4px, transparent 4px 8px);
		transform-origin: 0 50%;
	}
	:global(.landing[dir='rtl']) .edge {
		transform-origin: 100% 50%;
	}
	.edge::after {
		position: absolute;
		inset-inline-end: -4px;
		top: -3.5px;
		width: 8px;
		aspect-ratio: 1;
		border: 1.5px solid var(--accent-2);
		border-radius: 50%;
		content: '';
		background: var(--paper);
	}
	:global(.thread-ready) .edge {
		transform: scaleX(min(1, var(--arrive, 0) * 1.5));
	}
	.rail {
		position: absolute;
		top: 6.2rem;
		bottom: 11.8rem;
		inset-inline-start: var(--rail);
		width: 1px;
		background: var(--accent-2);
		opacity: 0.5;
		transform-origin: 50% 0;
	}
	:global(.thread-ready) .rail {
		transform: scaleY(var(--arrive, 0));
	}
	@media (max-width: 1023px) {
		.board {
			--rail: 21rem;
			margin-inline: 0;
		}
		.a-top,
		.a-bottom,
		.a-exit {
			inset-inline-start: var(--lane-in);
		}
	}
	@media (max-width: 640px) {
		.board {
			width: 100%;
			gap: 1.4rem;
		}
		.slot {
			--knot-x: calc(var(--indent) * 0.35);
		}
		.edge,
		.rail {
			display: none;
		}
	}
</style>
