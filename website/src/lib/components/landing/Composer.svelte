<script lang="ts">
	import Anda from '$lib/components/ui/icons/anda.svelte';
	import type { LandingCopy } from '$lib/content/landing';
	import { ArrowUp } from '@lucide/svelte';

	type Bubble = { id: number; role: 'user' | 'anda'; text: string; note?: string };

	let { copy, onmemory }: { copy: LandingCopy['demo']['composer']; onmemory?: () => void } =
		$props();

	let el: HTMLElement;
	let bubbles = $state<Bubble[]>([]);
	let draft = $state('');
	let thinking = $state(false);

	// Replays the conversation: type, send, think, reply. Each reply that saves
	// memory makes Anda cheer. The replay pauses while the card is offscreen.
	$effect(() => {
		const turns = copy.turns;
		let seq = 0;
		if (window.matchMedia('(prefers-reduced-motion: reduce)').matches) {
			bubbles = turns.flatMap((turn) => [
				{ id: ++seq, role: 'user' as const, text: turn.user },
				{ id: ++seq, role: 'anda' as const, text: turn.reply, note: turn.note }
			]);
			draft = '';
			return;
		}

		let cancelled = false;
		let visible = true;
		let resume: (() => void) | null = null;
		const observer = new IntersectionObserver(([entry]) => {
			visible = entry.isIntersecting;
			if (visible && resume) {
				resume();
				resume = null;
			}
		});
		observer.observe(el);
		const timers = new Set<ReturnType<typeof setTimeout>>();
		const wait = (ms: number) =>
			new Promise<void>((resolve) => {
				const timer = setTimeout(() => {
					timers.delete(timer);
					if (visible) resolve();
					else resume = resolve;
				}, ms);
				timers.add(timer);
			});

		async function play() {
			while (!cancelled) {
				bubbles = [];
				draft = '';
				await wait(700);
				for (const turn of turns) {
					for (const char of Array.from(turn.user)) {
						if (cancelled) return;
						draft += char;
						await wait(char === ' ' ? 16 : 26 + Math.random() * 34);
					}
					await wait(420);
					if (cancelled) return;
					bubbles = [...bubbles, { id: ++seq, role: 'user', text: turn.user }];
					draft = '';
					await wait(360);
					thinking = true;
					await wait(1150);
					if (cancelled) return;
					thinking = false;
					bubbles = [...bubbles, { id: ++seq, role: 'anda', text: turn.reply, note: turn.note }];
					onmemory?.();
					await wait(2700);
				}
				await wait(2600);
			}
		}

		void play();
		return () => {
			cancelled = true;
			thinking = false;
			observer.disconnect();
			for (const timer of timers) clearTimeout(timer);
		};
	});
</script>

<div class="composer" bind:this={el} aria-hidden="true">
	<header>
		<Anda class="avatar" />
		<strong>Anda</strong>
		<span class="status"><i></i>{copy.status}</span>
	</header>
	<div class="log">
		{#each bubbles as bubble (bubble.id)}
			<div class="bubble {bubble.role}">
				<p>{bubble.text}</p>
				{#if bubble.note}<span class="note"><i></i>{bubble.note}</span>{/if}
			</div>
		{/each}
		{#if thinking}<div class="bubble anda thinking">
				<span></span><span></span><span></span>
			</div>{/if}
	</div>
	<div class="input">
		<p class:empty={!draft}>{draft || copy.placeholder}<span class="caret"></span></p>
		<span class="send" class:ready={!!draft}><ArrowUp class="size-4" /></span>
	</div>
</div>

<style>
	.composer {
		display: flex;
		flex-direction: column;
		height: clamp(22rem, 52vh, 27rem);
		overflow: hidden;
		border: 1px solid rgb(255 255 255 / 0.12);
		border-radius: 1.4rem;
		background:
			radial-gradient(120% 70% at 100% 0%, rgb(241 166 78 / 0.12), transparent 60%),
			linear-gradient(180deg, rgb(20 38 33 / 0.94), rgb(8 18 16 / 0.96));
		box-shadow:
			0 50px 120px -40px rgb(0 0 0 / 0.7),
			inset 0 1px 0 rgb(255 255 255 / 0.08);
		color: var(--anda-parchment);
		font-size: 0.9rem;
	}
	header {
		display: flex;
		align-items: center;
		gap: 0.6rem;
		padding: 0.9rem 1.1rem;
		border-bottom: 1px solid rgb(255 255 255 / 0.08);
	}
	header :global(.avatar) {
		width: 2rem;
		height: 2rem;
	}
	strong {
		font-weight: 650;
	}
	.status {
		display: inline-flex;
		align-items: center;
		gap: 0.4rem;
		margin-inline-start: auto;
		color: rgb(244 238 227 / 0.55);
		font-family: var(--mono);
		font-size: 0.7rem;
	}
	.status i {
		width: 0.45rem;
		aspect-ratio: 1;
		border-radius: 50%;
		background: #6fd3a6;
		box-shadow: 0 0 0 3px rgb(111 211 166 / 0.18);
	}
	.log {
		display: flex;
		flex: 1;
		flex-direction: column;
		justify-content: flex-end;
		gap: 0.7rem;
		min-height: 0;
		padding: 1rem 1.1rem;
		mask-image: linear-gradient(180deg, transparent, #000 22%);
	}
	.bubble {
		max-width: 86%;
		animation: bubble-in 420ms cubic-bezier(0.2, 0.9, 0.25, 1.15) both;
	}
	.bubble p {
		padding: 0.62rem 0.85rem;
		border-radius: 1rem;
		line-height: 1.5;
	}
	.user {
		align-self: flex-end;
	}
	.user p {
		border-end-end-radius: 0.3rem;
		background: #f1a64e;
		color: #1a1206;
	}
	.anda p {
		border-end-start-radius: 0.3rem;
		background: rgb(255 255 255 / 0.07);
	}
	.note {
		display: inline-flex;
		align-items: center;
		gap: 0.4rem;
		margin-top: 0.45rem;
		color: #ffd08a;
		font-family: var(--mono);
		font-size: 0.7rem;
	}
	.note i {
		width: 0.7rem;
		aspect-ratio: 1;
		border: 1.5px solid currentColor;
		border-radius: 50%;
		animation: knot 900ms ease-out both;
	}
	.thinking {
		display: flex;
		gap: 0.3rem;
		padding: 0.8rem 0.9rem;
		border-radius: 1rem;
		background: rgb(255 255 255 / 0.07);
	}
	.thinking span {
		width: 0.38rem;
		aspect-ratio: 1;
		border-radius: 50%;
		background: rgb(244 238 227 / 0.6);
		animation: dot 1s ease-in-out infinite;
	}
	.thinking span:nth-child(2) {
		animation-delay: 0.15s;
	}
	.thinking span:nth-child(3) {
		animation-delay: 0.3s;
	}
	.input {
		display: flex;
		align-items: flex-end;
		gap: 0.6rem;
		margin: 0 0.8rem 0.8rem;
		padding: 0.7rem 0.7rem 0.7rem 0.95rem;
		border: 1px solid rgb(255 255 255 / 0.1);
		border-radius: 1.05rem;
		background: rgb(0 0 0 / 0.25);
	}
	.input p {
		flex: 1;
		min-height: 1.5em;
		line-height: 1.5;
	}
	.input p.empty {
		color: rgb(244 238 227 / 0.38);
	}
	.caret {
		display: inline-block;
		width: 2px;
		height: 1.05em;
		margin-inline-start: 1px;
		vertical-align: -0.15em;
		background: #ffd08a;
		animation: caret 1s steps(2) infinite;
	}
	.send {
		display: grid;
		width: 2rem;
		aspect-ratio: 1;
		flex-shrink: 0;
		place-items: center;
		border-radius: 0.65rem;
		background: rgb(255 255 255 / 0.08);
		color: rgb(244 238 227 / 0.5);
		transition:
			background 200ms,
			color 200ms;
	}
	.send.ready {
		background: #f1a64e;
		color: #1a1206;
	}
	@keyframes bubble-in {
		from {
			opacity: 0;
			transform: translateY(12px) scale(0.96);
		}
	}
	@keyframes knot {
		from {
			transform: scale(0);
		}
		60% {
			transform: scale(1.4);
		}
	}
	@keyframes dot {
		50% {
			opacity: 0.25;
			transform: translateY(-2px);
		}
	}
	@keyframes caret {
		50% {
			opacity: 0;
		}
	}
</style>
